//! Lower rich cell bodies and resolve cell-specific font metrics.
use crate::compiler::capture::{Capture, Kind, Leaf};
use crate::compiler::diagnostics::{self, Origin};
use crate::ir::*;
use crate::lower::Options;
use crate::lower::pictures::native_image;
use crate::lower::structure::structure;
use crate::lower::text::native_fragment;
use anyhow::{Result, bail};
use std::collections::{BTreeMap, HashSet};
use typst::foundations::{StyleChain, StyledElem};
use typst::layout::FrameItem;
use typst::text::{TextEdgeBounds, TextElem, TextItem};

pub(super) fn cell_font_metrics(text: &TextItem) -> (f64, f64) {
    // Office fits the horizontal font metrics into the nominal em before
    // applying exact cell line spacing. Typst can instead use OS/2 typo
    // metrics and cap height (notably for CJK fonts).
    let hhea = &text.font.ttf().tables().hhea;
    let ascent = f64::from(hhea.ascender).max(0.0);
    let descent = -f64::from(hhea.descender).min(0.0);
    let total = (ascent + descent).max(1.0);
    (
        text.size.to_pt() * ascent / total,
        text.size.to_pt() * descent / total,
    )
}

pub(super) fn cell_text_edges(capture: &Capture, leaf: &Leaf, text: &TextItem) -> (f64, f64) {
    let styled = capture
        .nearest(leaf, Kind::Paragraph)
        .and_then(|p| capture.nodes[p].content.location())
        .and_then(|loc| capture.resolved.get(&loc))
        .and_then(|c| c.to_packed::<StyledElem>());
    let styles = styled.map_or_else(StyleChain::default, |s| StyleChain::new(&s.styles));
    text.glyphs
        .iter()
        .fold((0.0_f64, 0.0_f64), |(top, bottom), g| {
            let (t, b) = text.font.edges(
                styles.get(TextElem::top_edge),
                styles.get(TextElem::bottom_edge),
                text.size,
                TextEdgeBounds::Glyph(g.id),
            );
            (top.max(t.to_pt()), bottom.max(b.to_pt()))
        })
}

/// Lower a rich cell body, preserving semantic paragraphs and nested tables.
pub(super) fn container(
    capture: &Capture,
    idx: usize,
    page: usize,
    options: &Options,
) -> Result<Vec<Element>> {
    let ids = &capture.nodes[idx].pages[&page].leaves;
    let mut used = HashSet::new();
    let mut output = BTreeMap::<usize, Vec<Element>>::new();
    for kind in [
        Kind::Table,
        Kind::List,
        Kind::Enum,
        Kind::Heading,
        Kind::Paragraph,
        Kind::Equation,
    ] {
        for n in capture.descendants(idx) {
            if capture.nodes[n].kind != kind {
                continue;
            }
            let Some(np) = capture.nodes[n].pages.get(&page) else {
                continue;
            };
            if np.leaves.is_empty() || np.leaves.iter().any(|id| used.contains(id)) {
                continue;
            }
            let mut parent = capture.nodes[n].parent;
            let mut owned = false;
            while let Some(p) = parent.filter(|&p| p != idx) {
                owned |= matches!(capture.nodes[p].kind, Kind::Table | Kind::List | Kind::Enum);
                parent = capture.nodes[p].parent;
            }
            if owned {
                continue;
            }
            let element = match kind {
                Kind::Table => structure(capture, n, page, &np.leaves, options)?,
                Kind::List | Kind::Enum => structure(capture, n, page, &np.leaves, options)?,
                _ => {
                    // A paragraph containing an image needs separate objects.
                    // Its text still uses source line breaks and tab advances.
                    match structure(capture, n, page, &np.leaves, options) {
                        Ok(element) => element,
                        Err(_) => continue,
                    }
                }
            };
            used.extend(np.leaves.iter().copied());
            output.insert(np.leaves[0], vec![element]);
        }
    }
    let mut i = 0;
    while i < ids.len() {
        let id = ids[i];
        if used.contains(&id) {
            i += 1;
            continue;
        }
        let leaf = &capture.pages[page][id];
        match &leaf.item {
            FrameItem::Text(_) => {
                let start = i;
                i += 1;
                while i < ids.len() && !used.contains(&ids[i]) {
                    let next = &capture.pages[page][ids[i]];
                    if !matches!(next.item, FrameItem::Text(_))
                        || next.frame != leaf.frame
                        || (next.position.1 - leaf.position.1).abs() > 0.1
                    {
                        break;
                    }
                    i += 1;
                }
                output.insert(
                    id,
                    vec![Element::Text(native_fragment(
                        capture,
                        page,
                        &ids[start..i],
                    )?)],
                );
            }
            FrameItem::Shape(..) => {
                output.insert(
                    id,
                    crate::graphics::convert_with_options(leaf, options)
                        .map_err(|error| diagnostics::at(error, Origin::leaf(capture, page, id)))?,
                );
                i += 1;
            }
            FrameItem::Image(image, size, _) => {
                output.insert(
                    id,
                    native_image(leaf, image, *size, options.image_dpi)
                        .map_err(|error| diagnostics::at(error, Origin::leaf(capture, page, id)))?,
                );
                i += 1;
            }
            FrameItem::Link(..) => {
                i += 1;
            }
            _ => bail!("unsupported content in table cell"),
        }
    }
    Ok(output.into_values().flatten().collect())
}
