//! Resolve text baselines, scripts, line spacing, and raw-block sizing.
use crate::compiler::capture::{Capture, Kind, Leaf};
use crate::lower::structure::boolean;
use typst::foundations::{StyleChain, StyledElem};
use typst::layout::FrameItem;
use typst::model::ParElem;
use typst::text::{ScriptKind, SubElem, SuperElem, TextEdgeBounds, TextElem, TextItem};

pub(super) fn is_raw_block(capture: &Capture, page: usize, ids: &[usize]) -> bool {
    let mut text = ids.iter().filter_map(|&id| {
        let leaf = &capture.pages[page][id];
        matches!(leaf.item, FrameItem::Text(_)).then_some(leaf)
    });
    let Some(first) = text.next() else {
        return false;
    };
    first.ancestors.iter().rev().any(|&idx| {
        let node = &capture.nodes[idx];
        node.content.elem().name() == "raw"
            && boolean(node, "block")
            && text.clone().all(|leaf| leaf.ancestors.contains(&idx))
    })
}

/// A one-line item has no observed line-to-line distance. Recover its line
/// height from the same resolved edges and leading that Typst uses, including
/// for the final item, which has no following item to measure against.
pub(in crate::lower) fn single_line_spacing(
    capture: &Capture,
    page: usize,
    ids: &[usize],
) -> Option<f64> {
    let first = ids.iter().find_map(|&id| {
        let leaf = &capture.pages[page][id];
        matches!(leaf.item, FrameItem::Text(_)).then_some(leaf)
    })?;
    let par = capture.nearest(first, Kind::Paragraph)?;
    let styled = capture.nodes[par]
        .content
        .location()
        .and_then(|loc| capture.resolved.get(&loc))?
        .to_packed::<StyledElem>()?;
    let styles = StyleChain::new(&styled.styles);
    let mut top: f64 = 0.0;
    let mut bottom: f64 = 0.0;
    for &id in ids {
        let leaf = &capture.pages[page][id];
        let FrameItem::Text(text) = &leaf.item else {
            continue;
        };
        for glyph in &text.glyphs {
            let (t, b) = text.font.edges(
                styles.get(TextElem::top_edge),
                styles.get(TextElem::bottom_edge),
                text.size,
                TextEdgeBounds::Glyph(glyph.id),
            );
            top = top.max(t.to_pt() * leaf.scale());
            bottom = bottom.max(b.to_pt() * leaf.scale());
        }
    }
    Some(top + bottom + styles.resolve(ParElem::leading).to_pt() * first.scale())
}

pub(in crate::lower) fn script_metrics(
    capture: &Capture,
    page: usize,
    leaf: &Leaf,
    text: &TextItem,
) -> (f64, f64, f64) {
    let regular = (text.size.to_pt(), 0.0, leaf.position.1);
    if capture.is_svg_math(leaf) {
        return regular;
    }
    if let Some(eq) = capture.nearest(leaf, Kind::Equation)
        && let Some(np) = capture.nodes[eq].pages.get(&page)
    {
        let size = capture.nodes[eq]
            .content
            .location()
            .and_then(|loc| capture.resolved.get(&loc))
            .and_then(|c| c.to_packed::<StyledElem>())
            .map(|c| StyleChain::new(&c.styles).resolve(TextElem::size).to_pt())
            .unwrap_or_else(|| {
                np.leaves
                    .iter()
                    .filter_map(|&id| match &capture.pages[page][id].item {
                        FrameItem::Text(t) => Some(t.size.to_pt()),
                        _ => None,
                    })
                    .fold(text.size.to_pt(), f64::max)
            });
        let baseline = np
            .baseline
            .or_else(|| {
                // Flattened math may have no enclosing frame. A glyph at the
                // equation's base size retains its baseline (scripts are smaller).
                np.leaves.iter().find_map(|&id| {
                    let l = &capture.pages[page][id];
                    match &l.item {
                        FrameItem::Text(t) if (t.size.to_pt() - size).abs() < 0.01 => {
                            Some(l.position.1)
                        }
                        _ => None,
                    }
                })
            })
            .or_else(|| {
                // For a lone fraction every glyph can be script-sized. The
                // fraction rule lies on the font's mathematical axis.
                let content = &capture.nodes[eq].content;
                if !crate::math::single_fraction(content) {
                    return None;
                }
                let axis = text
                    .font
                    .ttf()
                    .tables()
                    .math?
                    .constants?
                    .axis_height()
                    .value;
                let offset = f64::from(axis) / f64::from(text.font.ttf().units_per_em())
                    * size
                    * leaf.scale();
                np.leaves
                    .iter()
                    .filter_map(|&id| {
                        let l = &capture.pages[page][id];
                        let FrameItem::Shape(s, _) = &l.item else {
                            return None;
                        };
                        let typst::visualize::Geometry::Line(to) = &s.geometry else {
                            return None;
                        };
                        (to.y.to_pt().abs() < 0.01)
                            .then_some((to.x.to_pt().abs(), l.position.1 + offset))
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, y)| y)
            })
            .unwrap_or(leaf.position.1);
        return (size, 0.0, baseline);
    }
    let Some((node, kind)) = leaf.ancestors.iter().rev().find_map(|&i| {
        let node = &capture.nodes[i];
        let kind = match node.content.elem().name() {
            "super" => ScriptKind::Super,
            "sub" => ScriptKind::Sub,
            _ => return None,
        };
        Some((node, kind))
    }) else {
        return regular;
    };
    let Some(styled) = node
        .content
        .location()
        .and_then(|loc| capture.resolved.get(&loc))
        .and_then(|c| c.to_packed::<StyledElem>())
    else {
        return regular;
    };
    let styles = StyleChain::new(&styled.styles);
    let parent_size = styles.resolve(TextElem::size);
    let metrics = kind.read_metrics(text.font.metrics());
    let (typographic, baseline) = match kind {
        ScriptKind::Super => {
            let elem = node.content.to_packed::<SuperElem>().unwrap();
            (elem.typographic.get(styles), elem.baseline.get(styles))
        }
        ScriptKind::Sub => {
            let elem = node.content.to_packed::<SubElem>().unwrap();
            (elem.typographic.get(styles), elem.baseline.get(styles))
        }
    };
    let synthesized = !typographic || (text.size.to_pt() - parent_size.to_pt()).abs() > 0.01;
    let shift = if synthesized {
        baseline
            .custom()
            .map(|v| -v.at(parent_size).to_pt())
            .unwrap_or(metrics.vertical_offset.at(parent_size).to_pt())
    } else {
        metrics.vertical_offset.at(parent_size).to_pt()
    };
    let native_size = if synthesized {
        text.size.to_pt()
    } else {
        metrics.height.at(parent_size).to_pt()
    };
    let logical_baseline = leaf.position.1
        + if synthesized {
            shift * leaf.scale()
        } else {
            0.0
        };
    (native_size, shift, logical_baseline)
}

pub(super) fn code_font_scale(
    capture: &Capture,
    page: usize,
    ids: &[usize],
    left: f64,
    width: f64,
) -> f64 {
    // Office quantizes glyph advances. Reserve the upper eighth-point bound
    // for each glyph on a realized code line to estimate the fitting ratio.
    // Source line boundaries are stored separately as native line breaks;
    // PowerPoint applies this ratio through its native fitting property.
    let mut baseline = f64::NAN;
    let mut line = 0.0;
    let mut longest = width;
    for &id in ids {
        let leaf = &capture.pages[page][id];
        let FrameItem::Text(text) = &leaf.item else {
            continue;
        };
        if (leaf.position.1 - baseline).abs() > 0.1 || baseline.is_nan() {
            baseline = leaf.position.1;
            line = leaf.position.0 - left;
        }
        for glyph in &text.glyphs {
            let advance = glyph.x_advance.at(text.size).to_pt() * leaf.scale();
            line += (advance * 8.).ceil() / 8.;
        }
        longest = longest.max(line);
    }
    (width / longest).clamp(0.9, 1.0)
}
