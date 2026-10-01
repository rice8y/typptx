//! Bind links to their source objects. Only empty link areas remain independent.
use crate::{
    compiler::capture::{Capture, Leaf},
    geometry::paths,
    ir::*,
};
use typst::layout::FrameItem;

fn region(leaf: &Leaf, target: LinkTarget) -> Option<SlideLink> {
    let FrameItem::Link(_, size) = &leaf.item else {
        return None;
    };
    let t = leaf.transform;
    let point = |x: f64, y: f64| {
        [
            leaf.position.0 + t.sx.get() * x + t.kx.get() * y,
            leaf.position.1 + t.ky.get() * x + t.sy.get() * y,
        ]
    };
    let mut area = vec![vec![
        point(0., 0.),
        point(size.x.to_pt(), 0.),
        point(size.x.to_pt(), size.y.to_pt()),
        point(0., size.y.to_pt()),
    ]];
    for clip in &leaf.clips {
        area = paths::intersect(&area, clip);
    }
    let region = paths::shape(
        &paths::path(&area)?,
        [0., 0.],
        Some(Brush::Solid {
            color: [0, 0, 0, 0],
        }),
    )?;
    Some(SlideLink { target, region })
}

fn marker(capture: &Capture, leaf: &Leaf) -> Option<usize> {
    leaf.ancestors
        .iter()
        .rev()
        .copied()
        .find(|i| capture.link_targets.contains_key(i))
}

fn linked(target: LinkTarget, mut elements: Vec<Element>) -> Vec<Element> {
    if elements.is_empty() {
        return elements;
    }
    let element = if elements.len() == 1 {
        elements.pop().unwrap()
    } else {
        Element::group(elements)
    };
    vec![Element::Linked {
        target,
        element: Box::new(element),
    }]
}

pub(super) fn empty_region(capture: &Capture, page: usize, id: usize) -> Option<SlideLink> {
    let leaf = &capture.pages[page][id];
    let node = marker(capture, leaf)?;
    if capture.nodes[node].pages[&page]
        .leaves
        .iter()
        .any(|&i| capture.pages[page][i].is_drawable())
    {
        return None;
    }
    region(leaf, capture.link_targets[&node].clone())
}

/// The leaf IDs are the exact semantic or display-list source of these objects;
/// nearby geometry must never cause a link to attach to an unrelated object.
pub(super) fn attach(
    capture: &Capture,
    page: usize,
    ids: &[usize],
    mut elements: Vec<Element>,
) -> Vec<Element> {
    if elements.is_empty() {
        return elements;
    }
    let leaves = &capture.pages[page];
    let drawable: Vec<_> = ids
        .iter()
        .map(|&i| &leaves[i])
        .filter(|l| l.is_drawable())
        .collect();
    if let Some(target) = drawable.first().and_then(|l| capture.link_target(l))
        && drawable
            .iter()
            .all(|l| capture.link_target(l) == Some(target))
    {
        // Run hyperlinks already follow text editing and wrapping. Do not add
        // a second click target over their old layout coordinates.
        let text_only = elements.iter().all(|e| {
            matches!(e, Element::Text(t)
            if t.paragraphs.iter().flat_map(|p| &p.runs).all(|r| r.math.is_none()))
        });
        if text_only {
            return elements;
        }
        return linked(target.clone(), elements);
    }
    // A flattened unsupported block or an inline Office equation cannot carry
    // separate native run links. Keep its click regions inside the same group
    // so moving or deleting the block includes those regions.
    let drawing = elements
        .iter()
        .flat_map(Element::walk)
        .any(|e| matches!(e, Element::Drawing { .. }));
    let math = elements.iter().flat_map(Element::walk).any(|e| {
        let paragraphs: Vec<_> = match e {
            Element::Text(t) => t.paragraphs.iter().collect(),
            Element::Table(t) => t.cells.iter().flat_map(|c| &c.paragraphs).collect(),
            Element::MathSvg { .. } => return true,
            _ => Vec::new(),
        };
        paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .any(|r| r.math.is_some() && r.hyperlink.is_some())
    });
    let nodes: std::collections::HashSet<_> = drawable
        .iter()
        .filter(|l| {
            drawing
                || (math
                    && capture
                        .nearest(l, crate::compiler::capture::Kind::Equation)
                        .is_some())
        })
        .filter_map(|l| marker(capture, l))
        .collect();
    let mut overlays = Vec::new();
    for &id in ids {
        let leaf = &leaves[id];
        let Some(node) = marker(capture, leaf) else {
            continue;
        };
        let link = if nodes.contains(&node) {
            region(leaf, capture.link_targets[&node].clone())
        } else {
            empty_region(capture, page, id)
        };
        if let Some(link) = link {
            overlays.extend(linked(link.target, vec![Element::Shape(link.region)]));
        }
    }
    if !overlays.is_empty() {
        elements.extend(overlays);
        return vec![Element::group(elements)];
    }
    elements
}
