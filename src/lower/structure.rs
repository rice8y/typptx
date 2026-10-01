//! Dispatch semantic blocks and preserve their source ownership and transforms.
use crate::compiler::capture::{Capture, Kind, Node};
use crate::compiler::diagnostics::{self, Origin};
use crate::ir::*;
use crate::lower::Options;
use crate::lower::bibliography::bibliography;
use crate::lower::lists::list;
use crate::lower::table::table;
use anyhow::{Result, anyhow, ensure};
use typst::foundations::Value;
use typst::layout::{Abs, FrameItem};

pub(super) fn structure(
    capture: &Capture,
    idx: usize,
    page: usize,
    ids: &[usize],
    options: &Options,
) -> Result<Element> {
    structure_inner(capture, idx, page, ids, options)
        .map_err(|error| diagnostics::at(error, Origin::node(capture, idx, page)))
}

fn structure_inner(
    capture: &Capture,
    idx: usize,
    page: usize,
    ids: &[usize],
    options: &Options,
) -> Result<Element> {
    let kind = capture.nodes[idx].kind;
    let has_table = kind == Kind::Table
        || capture
            .descendants(idx)
            .iter()
            .any(|&i| capture.nodes[i].kind == Kind::Table);
    let lower = |capture: &Capture| match kind {
        Kind::Table => table(capture, idx, page, options),
        Kind::Bibliography => bibliography(capture, idx, page).map(Element::Text),
        Kind::List | Kind::Enum => list(capture, idx, page, options),
        _ => super::text::with_shapes(capture, idx, page, ids, options),
    };
    if let Some(first) = ids.iter().map(|&i| &capture.pages[page][i]).find(|l| {
        matches!(l.item, FrameItem::Text(_)) || has_table && matches!(l.item, FrameItem::Shape(..))
    }) {
        let t = first.transform;
        if (t.sx.get() - 1.).abs() > 1e-6
            || (t.sy.get() - 1.).abs() > 1e-6
            || t.kx.get().abs() > 1e-6
            || t.ky.get().abs() > 1e-6
        {
            let same = ids
                .iter()
                .map(|&i| &capture.pages[page][i])
                .filter(|l| matches!(l.item, FrameItem::Text(_)))
                .all(|l| {
                    let s = l.transform;
                    (t.sx.get() - s.sx.get()).abs() < 1e-6
                        && (t.sy.get() - s.sy.get()).abs() < 1e-6
                        && (t.kx.get() - s.kx.get()).abs() < 1e-6
                        && (t.ky.get() - s.ky.get()).abs() < 1e-6
                });
            if same {
                ensure!(
                    (t.sx.get() * t.kx.get() + t.ky.get() * t.sy.get()).abs() < 1e-6,
                    "PowerPoint cannot shear editable text"
                );
                let mut ts = t;
                ts.tx = Abs::zero();
                ts.ty = Abs::zero();
                let mut normalized = capture
                    .untransformed(page, ts)
                    .ok_or_else(|| anyhow!("singular text transform"))?;
                normalized.grouped_tables |= t.kx.get().abs() >= 1e-6
                    || t.ky.get().abs() >= 1e-6
                    || t.sx.get() < 0.
                    || t.sy.get() < 0.;
                let reflected = t.sx.get() * t.sy.get() - t.kx.get() * t.ky.get() < 0.;
                if reflected && has_table {
                    ensure!(
                        t.sx.get() < 0.
                            && t.sy.get() > 0.
                            && t.kx.get().abs() < 1e-6
                            && t.ky.get().abs() < 1e-6,
                        "editable tables only support horizontal reflection without rotation"
                    );
                }
                let mut element = lower(&normalized)?;
                if reflected && has_table {
                    mirror_table_text(&mut element)?;
                }
                return Ok(crate::geometry::transforms::apply(
                    vec![element],
                    [t.sx.get(), t.ky.get(), t.kx.get(), t.sy.get(), 0., 0.],
                ));
            }
        }
    }
    lower(capture)
}

fn mirror_table_text(element: &mut Element) -> Result<()> {
    match element {
        Element::Text(text) => {
            ensure!(
                text.vertical.is_none()
                    && text.paragraphs.iter().all(|p| p.lines.is_empty()
                        && p.tab_stops.is_empty()
                        && p.bullet.is_none()
                        && p.indent == 0.
                        && p.runs.iter().all(|r| r.math.is_none())
                        && matches!(p.alignment.as_str(), "l" | "r" | "ctr")),
                "reflected tables require simple text cells"
            );
            // Office reflects the box but keeps its text upright and aligned
            // to the original edge. Reverse the edge and mirror the glyphs.
            text.mirror_x = !text.mirror_x;
            for p in &mut text.paragraphs {
                std::mem::swap(&mut p.margin_left, &mut p.margin_right);
                p.alignment = match p.alignment.as_str() {
                    "l" => "r",
                    "r" => "l",
                    _ => "ctr",
                }
                .into();
            }
        }
        Element::Group(group) => {
            ensure!(
                group.rotation.abs() < 1e-6 && !group.flip_x && !group.flip_y,
                "nested transforms in reflected tables require a drawing"
            );
            for child in &mut group.elements {
                mirror_table_text(child)?;
            }
        }
        Element::Linked { element, .. } => mirror_table_text(element)?,
        _ => {}
    }
    Ok(())
}

pub(super) fn parent_structure(capture: &Capture, idx: usize) -> Option<usize> {
    let mut parent = capture.nodes[idx].parent;
    while let Some(i) = parent {
        if matches!(
            capture.nodes[i].kind,
            Kind::Table | Kind::List | Kind::Enum | Kind::Bibliography
        ) {
            return Some(i);
        }
        parent = capture.nodes[i].parent;
    }
    None
}

pub(super) fn string(node: &Node, field: &str) -> Option<String> {
    match node.content.field_by_name(field).ok()? {
        Value::Str(v) => Some(v.to_string()),
        _ => None,
    }
}

pub(super) fn integer(node: &Node, field: &str) -> Option<i64> {
    match node.content.field_by_name(field).ok()? {
        Value::Int(v) => Some(v),
        _ => None,
    }
}

pub(super) fn boolean(node: &Node, field: &str) -> bool {
    matches!(node.content.field_by_name(field), Ok(Value::Bool(true)))
}
