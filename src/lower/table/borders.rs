//! Map resolved table rules onto the known semantic cell grid.
//! Typst has already resolved explicit rules and adjacent-cell precedence.
use crate::{
    compiler::capture::{Capture, Kind},
    graphics,
    ir::Stroke,
};
use anyhow::{Result, bail, ensure};
use typst::{layout::FrameItem, visualize::Geometry};

type Edges = Vec<Vec<Option<Stroke>>>;

pub fn resolve(
    capture: &Capture,
    table: usize,
    page: usize,
    xs: &[f64],
    ys: &[f64],
) -> Result<(Edges, Edges)> {
    let mut horizontal = vec![vec![None; xs.len() - 1]; ys.len()];
    let mut vertical = vec![vec![None; xs.len()]; ys.len() - 1];
    for &id in &capture.nodes[table].pages[&page].leaves {
        let leaf = &capture.pages[page][id];
        // Cell contents (including underlines and equations) are not rules.
        if leaf
            .ancestors
            .iter()
            .any(|&i| i > table && capture.nodes[i].kind == Kind::Cell)
        {
            continue;
        }
        let FrameItem::Shape(shape, _) = &leaf.item else {
            continue;
        };
        let Some(stroke) = &shape.stroke else {
            continue;
        };
        ensure!(
            stroke
                .dash
                .as_ref()
                .is_none_or(|d| d.phase.to_pt().abs() < 1e-6),
            "table border dash offsets are unsupported"
        );
        let Geometry::Line(end) = &shape.geometry else {
            bail!("table rule is not a straight border");
        };
        let start = leaf.position;
        let end = (start.0 + end.x.to_pt(), start.1 + end.y.to_pt());
        let stroke = graphics::shape(leaf)?.stroke.unwrap();
        if (start.1 - end.1).abs() < 0.01 {
            let row = edge(ys, start.1)?;
            for col in segments(xs, start.0, end.0, stroke.width)? {
                horizontal[row][col] = Some(stroke.clone());
            }
        } else if (start.0 - end.0).abs() < 0.01 {
            let col = edge(xs, start.0)?;
            for row in segments(ys, start.1, end.1, stroke.width)? {
                vertical[row][col] = Some(stroke.clone());
            }
        } else {
            bail!("diagonal table rules are unsupported");
        }
    }
    Ok((horizontal, vertical))
}

fn edge(edges: &[f64], at: f64) -> Result<usize> {
    edges
        .iter()
        .position(|&v| (v - at).abs() < 0.02)
        .ok_or_else(|| anyhow::anyhow!("table rule does not lie on a cell boundary"))
}

fn segments(edges: &[f64], a: f64, b: f64, width: f64) -> Result<Vec<usize>> {
    let (start, end) = (a.min(b), a.max(b));
    // Typst extends strokes by half a line width at border intersections.
    let tolerance = width / 2. + 0.02;
    let mut result = Vec::new();
    for (i, pair) in edges.windows(2).enumerate() {
        if start <= pair[0] + tolerance && end >= pair[1] - tolerance {
            result.push(i);
        } else {
            ensure!(
                end <= pair[0] + tolerance || start >= pair[1] - tolerance,
                "table rule covers only part of a cell side"
            );
        }
    }
    ensure!(!result.is_empty(), "table rule has no matching cell side");
    Ok(result)
}
