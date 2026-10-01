//! Keep text surrounding native graphics in one semantic paragraph.
use crate::compiler::capture::{Capture, Kind};
use crate::ir::Element;
use crate::lower::{Options, links};
use anyhow::Result;
use typst::layout::FrameItem;

pub(in crate::lower) fn with_shapes(
    capture: &Capture,
    idx: usize,
    page: usize,
    ids: &[usize],
    options: &Options,
) -> Result<Element> {
    let shapes: Vec<_> = ids
        .iter()
        .copied()
        .filter(|&id| {
            let leaf = &capture.pages[page][id];
            matches!(leaf.item, FrameItem::Shape(..))
                && capture.nearest(leaf, Kind::Equation).is_none()
                && !leaf.ancestors.iter().any(|&i| {
                    matches!(
                        capture.nodes[i].content.elem().name(),
                        "underline" | "strike"
                    )
                })
        })
        .collect();
    if shapes.is_empty() {
        return super::text_block_ids(capture, idx, page, ids).map(Element::Text);
    }
    let mut prepared = capture.clone();
    let mut elements = Vec::new();
    for &id in &shapes {
        let leaf = &capture.pages[page][id];
        let graphics = crate::graphics::convert_with_options(leaf, options)?;
        elements.extend(links::attach(capture, page, &[id], graphics));
        if let Some(owner) = capture.nearest(leaf, Kind::Paragraph) {
            // A tab is added only when the next text leaf has a real gap.
            // Backgrounds such as highlights consume no horizontal advance.
            prepared.horizontal_spaces[page].push((id + 1, Some(owner)));
        }
    }
    // A graphic at the start of a paragraph has no preceding text from which
    // to infer a gap. Give it a tab placeholder, as for an inline picture.
    // Backgrounds overlap their text and must not consume an advance.
    let leading = ids
        .iter()
        .copied()
        .find(|&id| matches!(capture.pages[page][id].item, FrameItem::Text(_)))
        .and_then(|first| {
            let before: Vec<_> = shapes
                .iter()
                .copied()
                .take_while(|&id| id < first)
                .collect();
            let bounds = before
                .iter()
                .filter_map(|&id| capture.pages[page][id].ink_bounds())
                .reduce(crate::ir::Rect::union)?;
            let leaf = &capture.pages[page][first];
            (leaf.position.0 >= bounds.right() - 0.01 && leaf.position.0 > bounds.x + 0.01)
                .then_some((before[0], first, bounds.x))
        });
    if let Some((id, first, x)) = leading {
        use typst::{
            layout::{Em, Transform},
            syntax::Span,
            text::Glyph,
        };
        let first = &capture.pages[page][first];
        let FrameItem::Text(template) = &first.item else {
            unreachable!()
        };
        let mut text = template.clone();
        text.size *= first.scale();
        text.text = "\t".into();
        text.glyphs = vec![Glyph {
            id: 0,
            x_advance: Em::new((first.position.0 - x) / text.size.to_pt()),
            x_offset: Em::zero(),
            y_advance: Em::zero(),
            y_offset: Em::zero(),
            range: 0..1,
            span: (Span::detached(), 0),
        }];
        let leaf = &mut prepared.pages[page][id];
        leaf.item = FrameItem::Text(text);
        leaf.position = (x, first.position.1);
        leaf.transform = Transform::identity();
        prepared.inline_objects.insert((page, id));
    }
    let removed =
        |id: &usize| shapes.contains(id) && leading.is_none_or(|(keep, _, _)| keep != *id);
    let text: Vec<_> = ids.iter().copied().filter(|id| !removed(id)).collect();
    prepared.nodes[idx]
        .pages
        .get_mut(&page)
        .unwrap()
        .leaves
        .retain(|id| !removed(id));
    let extent = text
        .iter()
        .filter_map(|&id| {
            let leaf = &prepared.pages[page][id];
            let FrameItem::Text(t) = &leaf.item else {
                return None;
            };
            Some((
                leaf.position.0,
                leaf.position.0 + t.width().to_pt() * leaf.scale(),
            ))
        })
        .reduce(|(l, r), (x, right)| (l.min(x), r.max(right)));
    let np = prepared.nodes[idx].pages.get_mut(&page).unwrap();
    if let (Some((left, right)), Some((_, layout))) = (extent, np.layout)
        && layout.width < right - left - 0.01
    {
        // A flattened paragraph can capture the inline box as its only
        // nested layout frame. The surrounding paragraph context retains
        // the actual wrapping width.
        np.layout = None;
    }
    if text
        .iter()
        .any(|&id| matches!(capture.pages[page][id].item, FrameItem::Text(_)))
    {
        elements.push(Element::Text(super::text_block_ids(
            &prepared, idx, page, &text,
        )?));
    }
    // Inline graphics occupy reserved gaps; backgrounds must paint behind the
    // complete paragraph, including text that precedes the highlighted span.
    Ok(Element::group(elements))
}
