//! Build native text blocks from captured paragraphs and display-list fragments.
mod metrics;
mod paragraph;
mod shapes;
mod shaping;

use crate::compiler::capture::{Capture, Kind, Leaf};
use crate::compiler::diagnostics::{self, Origin};
use crate::ir::*;
use crate::lower::structure::boolean;
use crate::lower::text::metrics::{code_font_scale, is_raw_block};
use anyhow::{Result, anyhow, bail, ensure};
pub(super) use metrics::{script_metrics, single_line_spacing};
pub(super) use paragraph::{paragraph, push_run};
pub(super) use shapes::with_shapes;
use typst::layout::FrameItem;

pub(super) fn fragment_owner(capture: &Capture, leaf: &Leaf) -> Option<usize> {
    leaf.ancestors.iter().rev().copied().find(|&i| {
        matches!(
            capture.nodes[i].content.elem().name(),
            "repeat" | "raw" | "cell" | "par" | "entry" | "context" | "place"
        )
    })
}

pub(super) fn native_fragment(capture: &Capture, page: usize, ids: &[usize]) -> Result<TextBlock> {
    let mut p = paragraph(capture, page, ids)?;
    // Preserve intentional gaps between independently laid-out labels. A
    // paragraph already has its whitespace in the compiler's text items.
    let mut runs = Vec::new();
    let mut previous_right: Option<f64> = None;
    for &id in ids {
        let leaf = &capture.pages[page][id];
        let FrameItem::Text(text) = &leaf.item else {
            continue;
        };
        let mut single = paragraph(capture, page, &[id])?.paragraph.runs;
        if let Some(right) = previous_right.filter(|_| !p.paragraph.rtl) {
            let gap = leaf.position.0 - right;
            if gap > text.size.to_pt() * 0.1
                && let Some(last) = runs.last_mut()
            {
                let last: &mut Run = last;
                last.text.push_str(
                    &" ".repeat((gap / (text.size.to_pt() * 0.25)).round().clamp(1., 32.) as usize),
                );
            }
        }
        previous_right = Some(leaf.position.0 + text.width().to_pt() * leaf.scale());
        for run in single.drain(..) {
            push_run(&mut runs, run);
        }
    }
    p.paragraph.runs = runs;
    Ok(TextBlock {
        mirror_x: false,
        vertical: None,
        source_id: format!("display-list:{}", ids[0]),
        clip: text_clip(capture, page, ids)?,
        font_scale: None,
        role: "text".into(),
        bounds: Rect {
            x: p.x,
            y: p.first_baseline - p.ascent,
            width: (p.right - p.x + 2.).max(1.),
            height: p.ascent + p.descent + 2.,
        },
        paragraphs: vec![p.paragraph],
        wrap: false,
    })
}

pub(super) fn validate_text_leaves(capture: &Capture, page: usize, ids: &[usize]) -> Result<()> {
    text_clip(capture, page, ids)?;
    for &id in ids {
        let leaf = &capture.pages[page][id];
        if capture.is_svg_math(leaf) {
            continue;
        }
        (|| -> Result<()> {
            match &leaf.item {
                FrameItem::Text(text) => {
                    ensure!(
                        leaf.unclipped_transform(),
                        "text transform could not be represented by native text"
                    );
                    ensure!(
                        !crate::graphics::gradients::nonlinear(&crate::graphics::brush(
                            &text.fill
                        )?),
                        "PowerPoint cannot preserve this gradient on editable text"
                    );
                    if let Some(stroke) = &text.stroke {
                        ensure!(
                            !crate::graphics::gradients::nonlinear(&crate::graphics::brush(
                                &stroke.paint
                            )?),
                            "PowerPoint cannot preserve this gradient on an editable text outline"
                        );
                    }
                }
                FrameItem::Link(..) => {}
                FrameItem::Shape(..)
                    if leaf.ancestors.iter().any(|&i| {
                        matches!(
                            capture.nodes[i].content.elem().name(),
                            "underline" | "strike" | "equation"
                        )
                    }) => {}
                _ => bail!("block contains non-text content"),
            }
            Ok(())
        })()
        .map_err(|error| diagnostics::at(error, Origin::leaf(capture, page, id)))?;
    }
    Ok(())
}

pub(super) fn text_clip(capture: &Capture, page: usize, ids: &[usize]) -> Result<Option<Rect>> {
    let leaves: Vec<_> = ids
        .iter()
        .map(|&id| &capture.pages[page][id])
        .filter(|l| matches!(l.item, FrameItem::Text(_)) && !capture.is_svg_math(l))
        .collect();
    let affected = |leaf: &&Leaf| {
        leaf.clipped
            && leaf.ink_bounds().is_some_and(|b| {
                leaf.clips
                    .iter()
                    .any(|c| !crate::geometry::paths::contains_rect(c, b))
            })
    };
    let Some(first) = leaves.iter().copied().find(affected) else {
        return Ok(None);
    };
    let region = |leaf: &Leaf| {
        leaf.clips
            .iter()
            .cloned()
            .reduce(|a, b| crate::geometry::paths::intersect(&a, &b))
            .and_then(|c| crate::geometry::paths::rectangular(&c))
    };
    let clip = region(first).ok_or_else(|| anyhow!("editable text requires a rectangular clip"))?;
    let mut lines: Vec<(f64, Rect)> = Vec::new();
    for leaf in leaves {
        ensure!(
            region(leaf) == Some(clip)
                || (!affected(&leaf)
                    && leaf.ink_bounds().is_some_and(|b| {
                        b.x >= clip.x - 0.01
                            && b.y >= clip.y - 0.01
                            && b.right() <= clip.right() + 0.01
                            && b.bottom() <= clip.bottom() + 0.01
                    })),
            "different clipping regions inside one paragraph cannot be represented by a single editable text box"
        );
        if let (FrameItem::Text(text), Some(ink)) = (&leaf.item, leaf.ink_bounds()) {
            let (_, _, baseline) = script_metrics(capture, page, leaf, text);
            if let Some((_, bounds)) = lines.iter_mut().find(|(b, _)| (b - baseline).abs() < 0.1) {
                *bounds = bounds.union(ink);
            } else {
                lines.push((baseline, ink));
            }
        }
    }
    for (_, ink) in lines {
        crate::geometry::paths::check_text_clip(ink, clip, false)?;
    }
    Ok(Some(clip))
}

pub(super) fn text_block_ids(
    capture: &Capture,
    idx: usize,
    page: usize,
    ids: &[usize],
) -> Result<TextBlock> {
    let node = &capture.nodes[idx];
    let np = &node.pages[&page];
    let mut p = paragraph(capture, page, ids)?;
    // A display equation has its own layout frame. The first painted glyph can
    // be a limit or a prescript, so it is not the equation's left/top edge.
    if node.kind == Kind::Equation
        && boolean(node, "block")
        && !capture.has_fixed_math_layout(page, ids)
        && p.paragraph.runs.len() == 1
        && p.paragraph.runs[0].math.is_some()
        && let Some(bounds) = np.layout.map(|(_, bounds)| bounds).or_else(|| {
            ids.iter()
                .filter_map(|&id| capture.pages[page][id].ink_bounds())
                .reduce(Rect::union)
        })
    {
        p.paragraph.alignment = "ctr".into();
        return Ok(TextBlock {
            mirror_x: false,
            vertical: None,
            source_id: node.id(),
            clip: text_clip(capture, page, ids)?,
            role: "equation".into(),
            font_scale: None,
            bounds,
            paragraphs: vec![p.paragraph],
            wrap: false,
        });
    }
    if let Some(alignment) = node
        .content
        .location()
        .and_then(|loc| capture.paragraph_alignment.get(&loc))
    {
        p.paragraph.alignment.clone_from(alignment);
    }
    if boolean(node, "justify") {
        p.paragraph.alignment = "just".into();
    }
    let mut region = if ids.len() != np.leaves.len() {
        // A paragraph that crosses a page may enclose full-width header/footer
        // frames in the tag stream. They must not enlarge its wrapping width.
        ids.iter()
            .map(|&id| capture.pages[page][id].frame)
            .reduce(Rect::union)
            .unwrap()
    } else {
        np.layout.map(|(_, b)| b).unwrap_or(np.context)
    };
    if region == capture.page_bounds[page]
        && let Some(resolved) = node
            .content
            .location()
            .and_then(|loc| capture.resolved.get(&loc))
        && let Some(area) = crate::compiler::semantics::page_area(resolved, region, page)
    {
        region = area;
    }
    if let Some((left, right)) = capture.grid_cell_extent(idx, page) {
        let edge = region.right().min(right);
        region.x = region.x.max(left).max(np.origin.0);
        region.width = edge - region.x;
    }
    if !p.paragraph.tab_stops.is_empty() {
        if p.paragraph.rtl {
            for tab in &mut p.paragraph.tab_stops {
                *tab += region.right() - p.right;
            }
        } else {
            p.paragraph.alignment = "l".into();
        }
    }
    let aligned = matches!(p.paragraph.alignment.as_str(), "ctr" | "r");
    let left = if aligned { region.x } else { p.x };
    let width = if aligned {
        region.width
    } else {
        (region.right() - left).max(p.right - left + 1.0)
    };
    let raw = is_raw_block(capture, page, ids);
    p.paragraph.break_latin = raw;
    let intrinsic_single_line = (p.last_baseline - p.first_baseline).abs() < 0.1
        && (region.width - (p.right - p.x)).abs() < 0.1;
    Ok(TextBlock {
        mirror_x: false,
        vertical: None,
        source_id: node.id(),
        clip: text_clip(capture, page, ids)?,
        font_scale: raw.then(|| code_font_scale(capture, page, ids, left, width)),
        role: if node.kind == Kind::Heading {
            "heading"
        } else {
            "paragraph"
        }
        .into(),
        bounds: Rect {
            x: left,
            y: p.first_baseline - p.ascent,
            width,
            height: p.last_baseline - p.first_baseline + p.ascent + p.descent + 2.0,
        },
        // A standalone equation is one native mathematical object. Office's
        // math layout can be wider than Typst's, so it must not introduce soft
        // line breaks within a box measured with a different math font.
        wrap: !(capture.has_fixed_math_layout(page, ids)
            || raw
            || intrinsic_single_line
            || p.paragraph.runs.len() == 1 && p.paragraph.runs[0].math.is_some()),
        paragraphs: vec![p.paragraph],
    })
}
