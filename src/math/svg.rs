//! Render equation subtrees from the original display list. Layout placeholders
//! let the semantic exporter keep the enclosing paragraphs, lists, and tables.
use crate::compiler::capture::{Capture, Kind, filter_frame};
use crate::ir::{Element, Rect};
use anyhow::Result;
use std::collections::{BTreeMap, HashSet};
use typst::foundations::{Smart, StyleChain, StyledElem, Value};
use typst::introspection::{Tag, TagFlags};
use typst::layout::{Abs, Em, Frame, FrameItem, Point, Size, Transform};
use typst::syntax::Span;
use typst::text::{Glyph, TextElem, TextItem};
use typst::visualize::{Color, Paint};
use typst_layout::PagedDocument;

pub fn prepare(
    capture: &mut Capture,
    document: &PagedDocument,
    dpi: Option<u32>,
) -> Result<Vec<BTreeMap<usize, Element>>> {
    let mut images: Vec<BTreeMap<usize, Element>> =
        document.pages().iter().map(|_| BTreeMap::new()).collect();
    // Keep the outer equation only: nested math and hidden animation content
    // are already represented correctly by its realized display-list leaves.
    let equations: Vec<_> = capture
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(idx, node)| {
            if node.kind != Kind::Equation {
                return None;
            }
            let mut parent = node.parent;
            while let Some(i) = parent {
                if capture.nodes[i].kind == Kind::Equation {
                    return None;
                }
                // A mathematical list label is rendered as one picture bullet.
                // Keep its equations in that picture instead of extracting
                // independently positioned math images from the marker.
                if capture.nodes[i].kind == Kind::Label {
                    return None;
                }
                parent = capture.nodes[i].parent;
            }
            Some(idx)
        })
        .collect();
    for idx in equations {
        let node = &capture.nodes[idx];
        let location = node.content.location().unwrap();
        let font_size = capture
            .resolved
            .get(&location)
            .and_then(|c| c.to_packed::<StyledElem>())
            .map(|c| StyleChain::new(&c.styles).resolve(TextElem::size));
        let block = matches!(node.content.field_by_name("block"), Ok(Value::Bool(true)));
        for (&page_idx, np) in &node.pages {
            let leaves = &capture.pages[page_idx];
            let Some(&first) = np.leaves.first() else {
                continue;
            };
            let Some(mut bounds) = np
                .leaves
                .iter()
                .filter_map(|&i| leaves[i].ink_bounds())
                .reduce(Rect::union)
            else {
                continue;
            };
            // Leave a small transparent border for antialiasing and glyph
            // outlines. No page background or neighboring text is included.
            bounds.x -= 0.5;
            bounds.y -= 0.5;
            bounds.width += 1.;
            bounds.height += 1.;
            let wanted: HashSet<_> = np.leaves.iter().copied().collect();
            let original = &document.pages()[page_idx];
            let content = filter_frame(&original.frame, &|id| wanted.contains(&id), &mut 0);
            let mut page = original.clone();
            page.fill = Smart::Custom(None);
            page.bleed = Default::default();
            page.frame = Frame::hard(Size::new(Abs::pt(bounds.width), Abs::pt(bounds.height)));
            page.frame
                .push_frame(Point::new(Abs::pt(-bounds.x), Abs::pt(-bounds.y)), content);
            let svg = crate::assets::svg::page(&page)?;
            let png = crate::assets::images::render_fallback(&page, dpi)?;
            images[page_idx].insert(
                first,
                Element::MathSvg {
                    source_id: node.id(),
                    bounds,
                    svg,
                    png,
                },
            );

            let template = leaves
                .iter()
                .enumerate()
                .filter(|(_, l)| !l.ancestors.contains(&idx))
                .filter_map(|(i, l)| {
                    if let FrameItem::Text(t) = &l.item {
                        Some((i, t))
                    } else {
                        None
                    }
                })
                .min_by_key(|(i, _)| i.abs_diff(first))
                .map(|(_, t)| t.clone())
                .or_else(|| {
                    np.leaves.iter().find_map(|&i| {
                        if let FrameItem::Text(t) = &leaves[i].item {
                            Some(t.clone())
                        } else {
                            None
                        }
                    })
                });
            let mut placeholder = template.unwrap_or_else(empty_text);
            placeholder.size = font_size.unwrap_or(placeholder.size);
            placeholder.text = "\t".into();
            placeholder.fill = Paint::Solid(Color::BLACK);
            placeholder.stroke = None;
            let mut left = if block {
                np.layout.map_or(bounds.x, |(_, b)| b.x)
            } else {
                np.origin.0
            };
            let end = np.end.unwrap_or(np.origin);
            let baseline = if block {
                np.baseline.unwrap_or(end.1)
            } else {
                end.1
            };
            let right = if !block && (end.0 - left).abs() > 0.01 {
                let start = left;
                left = left.min(end.0);
                start.max(end.0)
            } else {
                np.layout.map_or(bounds.right(), |(_, b)| b.right())
            };
            left = left.min(right - 0.01);
            placeholder.glyphs = vec![Glyph {
                id: 0,
                x_advance: Em::new((right - left).max(0.01) / placeholder.size.to_pt()),
                x_offset: Em::zero(),
                y_advance: Em::zero(),
                y_offset: Em::zero(),
                range: 0..1,
                span: (Span::detached(), 0),
            }];
            // Preserve leaf indices so filtering the original page for image
            // fallback still selects exactly the same source content.
            let leaves = &mut capture.pages[page_idx];
            for &id in &np.leaves {
                let leaf = &mut leaves[id];
                leaf.item = FrameItem::Tag(Tag::End(
                    location,
                    0,
                    TagFlags {
                        introspectable: false,
                        tagged: false,
                    },
                ));
                leaf.transform = Transform::identity();
                leaf.clipped = false;
            }
            leaves[first].item = FrameItem::Text(placeholder);
            leaves[first].position = (left, baseline);
        }
        capture.svg_math.insert(idx);
        // Wrapping belongs to the enclosing text box/cell. Keep every
        // paragraph there on its source lines, including non-math list items.
        capture.fix_inline_layout(idx);
        capture.svg_math.extend(
            capture
                .descendants(idx)
                .into_iter()
                .filter(|&i| capture.nodes[i].kind == Kind::Equation),
        );
    }
    Ok(images)
}

pub(crate) fn empty_text() -> TextItem {
    let (font, _) = typst_kit::fonts::embedded().next().expect("embedded fonts");
    TextItem {
        font: font.instantiate(Default::default(), Abs::pt(12.), &Default::default()),
        size: Abs::pt(12.),
        fill: Paint::Solid(Color::BLACK),
        stroke: None,
        lang: typst::text::Lang::ENGLISH,
        region: None,
        text: "\t".into(),
        glyphs: Vec::new(),
    }
}
