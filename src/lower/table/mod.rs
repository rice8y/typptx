//! Build editable tables from resolved tracks, cell content, and borders.
mod borders;
mod cells;
mod layout;
use crate::compiler::capture::{Capture, Kind};
use crate::graphics::rgba;
use crate::ir::*;
use crate::lower::Options;
use crate::lower::lists::{list, owner_list};
use crate::lower::table::cells::{cell_font_metrics, cell_text_edges, container};
use crate::lower::text::{paragraph, script_metrics, validate_text_leaves};
use anyhow::{Result, anyhow, ensure};
use std::collections::{BTreeMap, HashSet};
use typst::foundations::{Smart, Value};
use typst::layout::{Abs, Alignment, FrameItem, HAlignment, Length, Rel, Sides, VAlignment};
use typst::visualize::Paint;

pub(super) fn table(
    capture: &Capture,
    idx: usize,
    page: usize,
    options: &Options,
) -> Result<Element> {
    let node = &capture.nodes[idx];
    let np = &node.pages[&page];
    ensure!(
        np.leaves.iter().all(|&id| {
            let leaf = &capture.pages[page][id];
            // Equation decorations (e.g. diagonal cancellation strokes)
            // are rebuilt by Office Math, not emitted as cell graphics.
            (matches!(leaf.item, FrameItem::Shape(..))
                && capture.nearest(leaf, Kind::Equation).is_some())
                || (leaf.plain_transform() && (leaf.scale() - 1.0).abs() < 1e-6)
        }),
        "transformed tables require a drawing"
    );
    let layout = crate::lower::table::layout::resolve(capture, idx, page)?;
    let bounds = layout.bounds;
    let mut cells = Vec::new();
    let mut children = Vec::new();
    for placement in &layout.cells {
        let cell_idx = placement.node;
        let cell = &capture.nodes[cell_idx];
        let cp = &cell.pages[&page];
        let column = placement.column;
        let row = placement.row;
        let column_span = placement.column_span;
        let row_span = placement.row_span;
        ensure!(column_span > 0 && row_span > 0, "invalid table cell span");
        let complex = capture.descendants(cell_idx).iter().any(|&n| {
            capture.nodes[n].kind == Kind::Table
                || capture.nodes[n].content.is::<typst::layout::GridElem>()
        }) || validate_text_leaves(capture, page, &cp.leaves).is_err();
        if complex {
            children.extend(container(capture, cell_idx, page, options)?);
        }
        let cell_lists: Vec<_> = capture
            .descendants(cell_idx)
            .into_iter()
            .filter(|&i| {
                !complex
                    && matches!(capture.nodes[i].kind, Kind::List | Kind::Enum)
                    && owner_list(capture, i).is_none()
                    && capture.nodes[i].pages.contains_key(&page)
            })
            .collect();
        let list_leaves: HashSet<_> = cell_lists
            .iter()
            .flat_map(|&i| capture.nodes[i].pages[&page].leaves.iter().copied())
            .collect();
        let text_ids: Vec<_> = cp
            .leaves
            .iter()
            .copied()
            .filter(|&id| {
                !complex
                    && !list_leaves.contains(&id)
                    && matches!(capture.pages[page][id].item, FrameItem::Text(_))
            })
            .collect();
        let mut parts = BTreeMap::new();
        for i in cell_lists {
            let list = list(capture, i, page, options)?;
            let ids = &capture.nodes[i].pages[&page].leaves;
            let baselines: Vec<_> = ids
                .iter()
                .filter_map(|&id| {
                    let leaf = &capture.pages[page][id];
                    if capture.nearest(leaf, Kind::Label).is_some() {
                        return None;
                    }
                    if let FrameItem::Text(text) = &leaf.item {
                        Some(script_metrics(capture, page, leaf, text).2)
                    } else {
                        None
                    }
                })
                .collect();
            let first = baselines.first().copied().unwrap_or(0.0);
            let last = baselines.last().copied().unwrap_or(first);
            parts.insert(ids[0], (list.paragraphs, first, last));
        }
        if !complex {
            validate_text_leaves(capture, page, &cp.leaves)?;
        }
        if !text_ids.is_empty() {
            let mut groups: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
            for &id in &text_ids {
                let lookup_id = if capture.inline_objects.contains(&(page, id)) {
                    text_ids
                        .iter()
                        .copied()
                        .filter(|i| !capture.inline_objects.contains(&(page, *i)))
                        .min_by_key(|i| i.abs_diff(id))
                        .unwrap_or(id)
                } else {
                    id
                };
                let par = capture.pages[page][lookup_id]
                    .ancestors
                    .iter()
                    .copied()
                    .find(|&p| p > cell_idx && capture.nodes[p].kind == Kind::Paragraph);
                if let Some((_, ids)) = groups.last_mut().filter(|(p, ids)| {
                    *p == par && !(ids[ids.len() - 1] + 1..id).any(|id| list_leaves.contains(&id))
                }) {
                    ids.push(id);
                } else {
                    groups.push((par, vec![id]));
                }
            }
            for (_, ids) in groups {
                let mut measured = paragraph(capture, page, &ids)?;
                if (measured.last_baseline - measured.first_baseline).abs() < 0.1 {
                    measured.paragraph.line_spacing = measured.ascent + measured.descent;
                }
                parts.insert(
                    ids[0],
                    (
                        vec![measured.paragraph],
                        measured.first_baseline,
                        measured.last_baseline,
                    ),
                );
            }
        }
        let mut parts: Vec<_> = parts.into_values().collect();
        for i in 0..parts.len().saturating_sub(1) {
            let gap = parts[i + 1].1 - parts[i].2;
            if let Some(next) = parts[i + 1].0.first() {
                let after = (gap - next.line_spacing - next.space_before).max(0.0);
                if let Some(last) = parts[i].0.last_mut() {
                    last.space_after = after;
                }
            }
        }
        let mut paragraphs: Vec<_> = parts.into_iter().flat_map(|p| p.0).collect();
        let sides = cell
            .content
            .field_by_name("inset")
            .ok()
            .and_then(|v| v.cast::<Smart<Sides<Option<Rel<Length>>>>>().ok())
            .ok_or_else(|| anyhow!("unresolved cell inset"))?
            .unwrap_or_default();
        let mut inset = [0.0; 4];
        for (i, side) in [sides.top, sides.right, sides.bottom, sides.left]
            .into_iter()
            .enumerate()
        {
            if let Some(value) = side {
                let basis = if i % 2 == 0 {
                    placement.bounds.height
                } else {
                    placement.bounds.width
                };
                inset[i] =
                    value.abs.at(Abs::pt(layout.font_size)).to_pt() + value.rel.get() * basis;
            }
        }
        let fill = match cell.content.field_by_name("fill") {
            Ok(Value::Color(c)) => Some(Brush::Solid { color: rgba(c) }),
            Ok(Value::None) | Err(_) => None,
            Ok(v) => {
                let paint = v.cast::<Paint>().map_err(|e| anyhow!("{e:?}"))?;
                if matches!(paint, Paint::Tiling(_)) {
                    None
                } else {
                    Some(crate::graphics::brush(&paint)?)
                }
            }
        };
        let alignment = cell
            .content
            .field_by_name("align")
            .ok()
            .and_then(|v| v.cast::<Smart<Alignment>>().ok())
            .unwrap_or(Smart::Auto);
        let mut vertical_alignment = "t";
        if let Smart::Custom(align) = alignment {
            for paragraph in &mut paragraphs {
                paragraph.alignment = match align.x().unwrap_or_default() {
                    HAlignment::Center => "ctr",
                    HAlignment::Right => "r",
                    HAlignment::Start if paragraph.rtl => "r",
                    HAlignment::End if !paragraph.rtl => "r",
                    _ => "l",
                }
                .into();
            }
            vertical_alignment = match align.y() {
                Some(VAlignment::Horizon) => "ctr",
                Some(VAlignment::Bottom) => "b",
                _ => "t",
            };
        }
        // Typst's cell padding starts at its text top edge (usually cap
        // height). Office includes the font ascent/descent in the text body.
        // Copying vertical padding verbatim makes rows grow and cover content
        // below the table, even when the same font is installed.
        let mut text_inset = inset;
        let cell_text: Vec<_> = cp
            .leaves
            .iter()
            .filter_map(|&id| {
                if complex {
                    return None;
                }
                let leaf = &capture.pages[page][id];
                if capture.nearest(leaf, Kind::Label).is_some() {
                    return None;
                }
                if let FrameItem::Text(text) = &leaf.item {
                    Some((leaf, text))
                } else {
                    None
                }
            })
            .collect();
        if let Some((first, text)) = cell_text.first() {
            let (size, _, baseline) = script_metrics(capture, page, first, text);
            let (ascent, _) = cell_font_metrics(text);
            let ascent = ascent * size / text.size.to_pt();
            let leading = paragraphs
                .first()
                .map_or(0.0, |p| (p.line_spacing - size).max(0.0));
            if vertical_alignment == "t" {
                // Office applies exact line spacing to the first line too.
                // Its extra leading must not move the first baseline or grow
                // the row. DrawingML cell margins are signed coordinates.
                text_inset[0] = baseline - ascent - cp.origin.1 - leading;
            } else {
                // Center/bottom alignment must keep the same font-edge
                // compensation on both sides, without absorbing the blank
                // space introduced by the vertical alignment itself.
                let (top, _) = cell_text_edges(capture, first, text);
                text_inset[0] = inset[0] + top - ascent - leading;
            }
        }
        if let Some((last, text)) = cell_text.last() {
            let (_, descent) = cell_font_metrics(text);
            let (_, bottom) = cell_text_edges(capture, last, text);
            // Office's minimum row height rounds line metrics upwards. Leave
            // a point below the text body so it does not enlarge source rows.
            text_inset[2] = inset[2] + bottom - descent - 1.0;
        }
        // An equation is one inline object. Its script/numerator baselines
        // do not imply that the surrounding cell contains multiple lines.
        let mut seen_equations = HashSet::new();
        let baselines: Vec<_> = cell_text
            .iter()
            .filter_map(|(leaf, text)| {
                if let Some(eq) = capture.nearest(leaf, Kind::Equation) {
                    if !seen_equations.insert(eq) {
                        return None;
                    }
                    Some(
                        capture.nodes[eq].pages[&page]
                            .baseline
                            .unwrap_or(leaf.position.1),
                    )
                } else {
                    Some(script_metrics(capture, page, leaf, text).2)
                }
            })
            .collect();
        let has_svg_math = capture.has_fixed_math_layout(page, &cp.leaves);
        let wrap = !has_svg_math
            && (paragraphs.len() > 1
                || paragraphs.iter().any(|p| p.bullet.is_some())
                || baselines.windows(2).any(|b| (b[1] - b[0]).abs() > 0.1));
        cells.push(TableCell {
            wrap,
            row,
            column,
            row_span,
            column_span,
            paragraphs,
            inset,
            text_inset,
            fill,
            vertical_alignment: vertical_alignment.into(),
        });
    }
    ensure!(!cells.is_empty(), "table has no cells");
    let columns = layout.xs.len() - 1;
    let rows = layout.ys.len() - 1;
    for row in 0..rows {
        for column in 0..columns {
            let count = cells
                .iter()
                .filter(|c| {
                    c.row <= row
                        && row < c.row + c.row_span
                        && c.column <= column
                        && column < c.column + c.column_span
                })
                .count();
            ensure!(count <= 1, "overlapping table grid");
            if count == 0 {
                ensure!(
                    layout.gutter_columns.contains(&column) || layout.gutter_rows.contains(&row),
                    "incomplete table grid"
                );
                cells.push(TableCell {
                    row,
                    column,
                    row_span: 1,
                    column_span: 1,
                    wrap: false,
                    paragraphs: vec![],
                    inset: [0.; 4],
                    text_inset: [0.; 4],
                    fill: None,
                    vertical_alignment: "t".into(),
                });
            }
        }
    }
    let resolved =
        crate::lower::table::borders::resolve(capture, idx, page, &layout.xs, &layout.ys);
    let mut use_overlay = resolved.is_err();
    let (mut horizontal_borders, mut vertical_borders) = resolved.unwrap_or_else(|_| {
        (
            vec![vec![None; columns]; rows + 1],
            vec![vec![None; columns + 1]; rows],
        )
    });
    // Office has a single border per merged side. Keep differing segments as
    // native lines in the table's group instead of extending or deleting them.
    for cell in &cells {
        let right = cell.column + cell.column_span;
        let bottom = cell.row + cell.row_span;
        for r in [cell.row, bottom] {
            let edges: Vec<_> = (cell.column..right)
                .filter(|&c| layout.xs[c + 1] - layout.xs[c] > 0.01)
                .map(|c| &horizontal_borders[r][c])
                .collect();
            use_overlay |= edges.windows(2).any(|e| e[0] != e[1]);
        }
        for c in [cell.column, right] {
            let edges: Vec<_> = (cell.row..bottom)
                .filter(|&r| layout.ys[r + 1] - layout.ys[r] > 0.01)
                .map(|r| &vertical_borders[r][c])
                .collect();
            use_overlay |= edges.windows(2).any(|e| e[0] != e[1]);
        }
    }
    if use_overlay {
        for row in &mut horizontal_borders {
            row.fill(None);
        }
        for row in &mut vertical_borders {
            row.fill(None);
        }
        for &id in &np.leaves {
            let leaf = &capture.pages[page][id];
            if leaf
                .ancestors
                .iter()
                .any(|&i| i > idx && capture.nodes[i].kind == Kind::Cell)
            {
                continue;
            }
            if matches!(&leaf.item, FrameItem::Shape(shape, _) if shape.stroke.is_some()) {
                children.extend(crate::graphics::convert_with_options(leaf, options)?);
            }
        }
    }
    let column_widths = layout.xs.windows(2).map(|p| p[1] - p[0]).collect();
    let row_heights = layout.ys.windows(2).map(|p| p[1] - p[0]).collect();
    let table = Element::Table(Table {
        source_id: node.id(),
        bounds,
        column_widths,
        row_heights,
        cells,
        horizontal_borders,
        vertical_borders,
    });
    let mut backgrounds = Vec::new();
    for &id in &np.leaves {
        let leaf = &capture.pages[page][id];
        if capture.nearest(leaf, Kind::Table) == Some(idx)
            && !leaf
                .ancestors
                .iter()
                .any(|&n| n > idx && capture.nodes[n].kind == Kind::Cell)
            && matches!(&leaf.item, FrameItem::Shape(s, _) if matches!(s.fill, Some(Paint::Tiling(_))))
        {
            backgrounds.extend(crate::graphics::convert_with_options(leaf, options)?);
        }
    }
    if children.is_empty() && backgrounds.is_empty() {
        Ok(table)
    } else {
        backgrounds.push(table);
        backgrounds.extend(children);
        Ok(Element::group(backgrounds))
    }
}
