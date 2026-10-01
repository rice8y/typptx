//! Recover table tracks from the compiler's semantic grid and placed cells.
use crate::{
    compiler::capture::{Capture, Kind},
    ir::Rect,
};
use anyhow::{Result, anyhow, ensure};
use std::collections::{BTreeMap, BTreeSet};
use typst::{
    foundations::{StyleChain, StyledElem},
    layout::Sizing,
    model::{TableCell, TableElem},
    text::TextElem,
};

pub struct CellPlacement {
    pub node: usize,
    pub column: usize,
    pub row: usize,
    pub column_span: usize,
    pub row_span: usize,
    pub bounds: Rect,
}

pub struct Layout {
    pub bounds: Rect,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub cells: Vec<CellPlacement>,
    pub gutter_columns: Vec<usize>,
    pub gutter_rows: Vec<usize>,
    pub font_size: f64,
}

fn field(capture: &Capture, cell: usize, name: &str, default: usize) -> usize {
    capture.nodes[cell]
        .content
        .field_by_name(name)
        .ok()
        .and_then(|v| v.cast::<usize>().ok())
        .unwrap_or(default)
}

fn table_bounds(
    capture: &Capture,
    idx: usize,
    page: usize,
    cells: &[usize],
    rtl: bool,
) -> Result<Rect> {
    let np = &capture.nodes[idx].pages[&page];
    let mut bounds = np
        .layout
        .map(|(_, b)| b)
        .ok_or_else(|| anyhow!("table has no layout frame"))?;
    let cells: Vec<_> = cells
        .iter()
        .copied()
        .filter(|&n| capture.nodes[n].pages.contains_key(&page))
        .collect();
    let end_col = cells
        .iter()
        .map(|&n| field(capture, n, "x", 0) + field(capture, n, "colspan", 1))
        .max()
        .unwrap_or(0);
    let end_row = cells
        .iter()
        .map(|&n| field(capture, n, "y", 0) + field(capture, n, "rowspan", 1))
        .max()
        .unwrap_or(0);
    // Continuation frames can be flattened into the page's larger body frame.
    // A cell's enclosing frame still contains its complete allocated region.
    // Edge cells therefore provide tighter outer boundaries; interior text
    // frames do not. The table's closing tag also retains its final bottom.
    let mut right = bounds.right();
    let mut bottom = bounds.bottom();
    for n in cells {
        let cp = &capture.nodes[n].pages[&page];
        let at_right = if rtl {
            field(capture, n, "x", 0) == 0
        } else {
            field(capture, n, "x", 0) + field(capture, n, "colspan", 1) == end_col
        };
        if at_right {
            right = right.min(cp.context.right());
        }
        if field(capture, n, "y", 0) + field(capture, n, "rowspan", 1) == end_row {
            bottom = bottom.min(cp.context.bottom());
        }
    }
    if let Some((_, y)) = np.end.filter(|(_, y)| *y > bounds.y) {
        bottom = bottom.min(y);
    }
    bounds.width = right - bounds.x;
    bounds.height = bottom - bounds.y;
    Ok(bounds)
}

pub fn resolve(capture: &Capture, idx: usize, page: usize) -> Result<Layout> {
    let table = &capture.nodes[idx];
    let np = &table.pages[&page];
    let rtl = table
        .content
        .location()
        .and_then(|loc| capture.paragraph_rtl.get(&loc))
        .copied()
        .unwrap_or(false);
    let grid = table
        .content
        .to_packed::<TableElem>()
        .and_then(|t| t.grid.as_ref())
        .ok_or_else(|| anyhow!("table has no resolved grid"))?;
    let all: Vec<_> = capture
        .descendants(idx)
        .into_iter()
        .filter(|&n| {
            // TableCell and GridCell both have the Typst element name "cell".
            // Layout grids nested in a table body do not define table tracks.
            if !capture.nodes[n].content.is::<TableCell>() {
                return false;
            }
            let mut parent = capture.nodes[n].parent;
            while let Some(i) = parent {
                if capture.nodes[i].kind == Kind::Table {
                    return i == idx;
                }
                parent = capture.nodes[i].parent;
            }
            false
        })
        .collect();
    let bounds = table_bounds(capture, idx, page, &all, rtl)?;
    let page_bounds: BTreeMap<_, _> = table
        .pages
        .keys()
        .filter_map(|&p| {
            table_bounds(capture, idx, p, &all, rtl)
                .ok()
                .map(|b| (p, b))
        })
        .collect();
    let cells: Vec<_> = all
        .iter()
        .copied()
        .filter(|&n| capture.nodes[n].pages.contains_key(&page))
        .collect();
    ensure!(!cells.is_empty(), "table has no cells on this page");
    let factor = if grid.has_gutter { 2 } else { 1 };
    let mut rows = BTreeSet::new();
    for &n in &cells {
        let start = field(capture, n, "y", 0);
        rows.extend(start..start + field(capture, n, "rowspan", 1));
    }
    let rows: Vec<_> = rows.into_iter().collect();
    let row_indices: BTreeMap<_, _> = rows
        .iter()
        .enumerate()
        .map(|(i, &r)| (r, i * factor))
        .collect();
    let row_tracks: Vec<_> = rows
        .iter()
        .enumerate()
        .flat_map(|(i, &r)| {
            let mut tracks = vec![r * factor];
            if factor == 2 && i + 1 < rows.len() {
                tracks.push(r * factor + 1);
            }
            tracks
        })
        .collect();
    let row_sizing: Vec<_> = row_tracks
        .iter()
        .map(|&r| {
            grid.rows
                .get(r)
                .copied()
                .ok_or_else(|| anyhow!("table cell exceeds the resolved row tracks"))
        })
        .collect::<Result<_>>()?;
    // A continued rowspan can be the only cell in later regions. Typst then
    // ends the frame at that cell's right edge, omitting trailing columns.
    // Keep preceding columns: a continuation in column 2 still has its original
    // horizontal offset from the table's origin.
    let column_end = cells
        .iter()
        .map(|&n| {
            (field(capture, n, "x", 0) + field(capture, n, "colspan", 1)) * factor - (factor - 1)
        })
        .max()
        .unwrap();
    let col_sizing = grid
        .cols
        .get(..column_end)
        .ok_or_else(|| anyhow!("table cell exceeds the resolved column tracks"))?;
    let mut x_known = BTreeMap::from([(0, bounds.x), (column_end, bounds.right())]);
    // A column hidden by a merge on one page can be visible on another page.
    for &n in &all {
        for (&p, cp) in &capture.nodes[n].pages {
            let Some(tb) = page_bounds.get(&p) else {
                continue;
            };
            let col = field(capture, n, "x", 0) * factor;
            let edge = if rtl {
                col + field(capture, n, "colspan", 1) * factor - (factor - 1)
            } else {
                col
            };
            if edge <= column_end {
                let offset = if rtl {
                    tb.right() - cp.origin.0
                } else {
                    cp.origin.0 - tb.x
                };
                insert_anchor(&mut x_known, edge, bounds.x + offset)?;
            }
        }
    }
    let mut y_known = BTreeMap::from([(0, bounds.y), (row_sizing.len(), bounds.bottom())]);
    for &n in &cells {
        let row = field(capture, n, "y", 0);
        insert_anchor(
            &mut y_known,
            row_indices[&row],
            capture.nodes[n].pages[&page].origin.1,
        )?;
    }
    let resolved = table
        .content
        .location()
        .and_then(|loc| capture.resolved.get(&loc))
        .and_then(|c| c.to_packed::<StyledElem>());
    let styles = resolved.map_or_else(StyleChain::default, |s| StyleChain::new(&s.styles));
    let font_size = styles.resolve(TextElem::size);
    let context = if np.context == capture.page_bounds[page] {
        table
            .content
            .location()
            .and_then(|loc| capture.resolved.get(&loc))
            .and_then(|c| crate::compiler::semantics::page_area(c, np.context, page))
            .unwrap_or(np.context)
    } else {
        np.context
    };
    let col_spans: Vec<_> = all
        .iter()
        .filter_map(|&n| {
            let start = field(capture, n, "x", 0) * factor;
            (start < column_end).then_some((
                start,
                (start + field(capture, n, "colspan", 1) * factor - (factor - 1)).min(column_end),
            ))
        })
        .collect();
    let row_spans: Vec<_> = cells
        .iter()
        .map(|&n| {
            let start = row_indices[&field(capture, n, "y", 0)];
            (
                start,
                start + field(capture, n, "rowspan", 1) * factor - (factor - 1),
            )
        })
        .collect();
    let xs = tracks(
        col_sizing,
        &x_known,
        &col_spans,
        font_size.to_pt(),
        context.width,
    )?;
    let ys = tracks(
        &row_sizing,
        &y_known,
        &row_spans,
        font_size.to_pt(),
        context.height,
    )?;
    // Typst can leave an auto track at zero when only a spanning cell measures
    // it. Office expands zero-height rows and can lose their merges. Collapse
    // these invisible rows as well as zero gutters, keeping visible cells.
    // Zero-width content columns remain valid Office merge participants.
    let (mut xs, x_map, mut gutter_columns) = remove_empty_tracks(xs, factor, false);
    if rtl {
        xs = xs
            .into_iter()
            .rev()
            .map(|x| bounds.x + bounds.right() - x)
            .collect();
        gutter_columns = gutter_columns
            .into_iter()
            .map(|i| xs.len() - 2 - i)
            .collect();
        gutter_columns.sort_unstable();
    }
    let (ys, y_map, gutter_rows) = remove_empty_tracks(ys, factor, true);
    let mut placements = Vec::new();
    for n in cells {
        let col = field(capture, n, "x", 0) * factor;
        let row = row_indices[&field(capture, n, "y", 0)];
        let right = col + field(capture, n, "colspan", 1) * factor - (factor - 1);
        let bottom = row + field(capture, n, "rowspan", 1) * factor - (factor - 1);
        let (col, right) = if rtl {
            (xs.len() - 1 - x_map[right], xs.len() - 1 - x_map[col])
        } else {
            (x_map[col], x_map[right])
        };
        let (row, bottom) = (y_map[row], y_map[bottom]);
        ensure!(
            right > col && bottom > row,
            "a table cell has zero width or height"
        );
        placements.push(CellPlacement {
            node: n,
            column: col,
            row,
            column_span: right - col,
            row_span: bottom - row,
            bounds: Rect {
                x: xs[col],
                y: ys[row],
                width: xs[right] - xs[col],
                height: ys[bottom] - ys[row],
            },
        });
    }
    Ok(Layout {
        bounds,
        xs,
        ys,
        cells: placements,
        gutter_columns,
        gutter_rows,
        font_size: font_size.to_pt(),
    })
}

fn insert_anchor(known: &mut BTreeMap<usize, f64>, index: usize, at: f64) -> Result<()> {
    if let Some(old) = known.insert(index, at) {
        ensure!(
            (old - at).abs() < 0.03,
            "inconsistent table track boundaries"
        );
    }
    Ok(())
}

fn tracks(
    sizing: &[Sizing],
    anchors: &BTreeMap<usize, f64>,
    spans: &[(usize, usize)],
    em: f64,
    base: f64,
) -> Result<Vec<f64>> {
    let mut widths: Vec<Option<f64>> = sizing
        .iter()
        .enumerate()
        .map(|(i, s)| match s {
            Sizing::Rel(v) => {
                Some(v.abs.at(typst::layout::Abs::pt(em)).to_pt() + v.rel.get() * base)
            }
            // Typst measures a spanning cell at the last auto track it covers.
            // Earlier auto tracks with no measuring cells therefore stay zero.
            Sizing::Auto
                if !spans.iter().any(|&(start, end)| {
                    (start..end).rev().find(|&j| sizing[j] == Sizing::Auto) == Some(i)
                }) =>
            {
                Some(0.)
            }
            _ => None,
        })
        .collect();
    let points: Vec<_> = anchors.iter().map(|(&i, &x)| (i, x)).collect();
    for pair in points.windows(2) {
        let ((start, a), (end, b)) = (pair[0], pair[1]);
        let unknown: Vec<_> = (start..end).filter(|&i| widths[i].is_none()).collect();
        let remaining = b - a - widths[start..end].iter().flatten().sum::<f64>();
        ensure!(
            remaining >= -0.03,
            "table track sizes disagree with placed cells"
        );
        if unknown.len() == 1 {
            widths[unknown[0]] = Some(remaining.max(0.));
        } else if !unknown.is_empty() && unknown.iter().all(|&i| matches!(sizing[i], Sizing::Fr(_)))
        {
            let weight: f64 = unknown
                .iter()
                .map(|&i| match sizing[i] {
                    Sizing::Fr(f) => f.get(),
                    _ => unreachable!(),
                })
                .sum();
            ensure!(weight > 0., "unresolved fractional table tracks");
            for i in unknown {
                if let Sizing::Fr(f) = sizing[i] {
                    widths[i] = Some(remaining.max(0.) * f.get() / weight);
                }
            }
        } else {
            ensure!(
                unknown.is_empty(),
                "table spans hide unresolved auto track boundaries"
            );
            ensure!(
                remaining.abs() < 0.03,
                "table track sizes disagree with placed cells"
            );
        }
    }
    let mut edges = vec![points[0].1];
    for w in widths {
        let w = w.ok_or_else(|| anyhow!("unresolved table track size"))?;
        ensure!(w >= -0.03 && w.is_finite(), "invalid table track size");
        edges.push(edges.last().unwrap() + w.max(0.));
    }
    Ok(edges)
}

fn remove_empty_tracks(
    edges: Vec<f64>,
    factor: usize,
    collapse_content: bool,
) -> (Vec<f64>, Vec<usize>, Vec<usize>) {
    let mut output = vec![edges[0]];
    let mut mapping = vec![0];
    let mut gutters = Vec::new();
    for (i, pair) in edges.windows(2).enumerate() {
        if (pair[1] - pair[0]).abs() >= 1e-6 || !(collapse_content || factor == 2 && i % 2 == 1) {
            if factor == 2 && i % 2 == 1 {
                gutters.push(output.len() - 1);
            }
            output.push(pair[1]);
        }
        mapping.push(output.len() - 1);
    }
    (output, mapping, gutters)
}
