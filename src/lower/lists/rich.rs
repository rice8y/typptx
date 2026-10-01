//! Preserve non-text list bodies as native objects beside native paragraphs.
use super::{belongs_to_list, list_blocks};
use crate::compiler::capture::{Capture, Kind, Leaf};
use crate::compiler::diagnostics::{self, Origin};
use crate::ir::Element;
use crate::lower::{Options, links, pictures, structure::structure};
use anyhow::Result;
use std::collections::{BTreeMap, HashSet};
use typst::layout::FrameItem;

fn body_object(capture: &Capture, leaf: &Leaf) -> bool {
    capture.nearest(leaf, Kind::Label).is_none()
        && capture.nearest(leaf, Kind::Equation).is_none()
        && match leaf.item {
            FrameItem::Image(..) => true,
            FrameItem::Shape(..) => !leaf.ancestors.iter().any(|&i| {
                matches!(
                    capture.nodes[i].content.elem().name(),
                    "underline" | "strike"
                )
            }),
            _ => false,
        }
}

pub(in crate::lower) fn needed(capture: &Capture, idx: usize, page: usize) -> bool {
    let mut previous = None;
    capture.nodes[idx].pages[&page].leaves.iter().any(|&id| {
        let leaf = &capture.pages[page][id];
        // Typst flattens column frames and does not retain a columns tag.
        // A new paragraph that starts before the previous one ends cannot be
        // represented by consecutive paragraphs in one PowerPoint text box.
        let parallel = if let FrameItem::Text(text) = &leaf.item
            && capture.nearest(leaf, Kind::Label).is_none()
        {
            let par = capture.nearest(leaf, Kind::Paragraph);
            let (_, _, baseline) = crate::lower::text::script_metrics(capture, page, leaf, text);
            let parallel = previous.is_some_and(|(p, y)| p != par && baseline <= y + 0.1);
            previous = Some((par, baseline));
            parallel
        } else {
            false
        };
        body_object(capture, leaf)
            || parallel
            || capture.nearest(leaf, Kind::Label).is_none()
                && capture.nearest(leaf, Kind::Paragraph).is_some_and(|p| {
                    capture.nodes[p]
                        .content
                        .location()
                        .and_then(|loc| capture.paragraph_alignment.get(&loc))
                        .is_some_and(|a| a == "ctr")
                })
            || leaf.ancestors.iter().any(|&i| {
                i > idx && matches!(capture.nodes[i].content.elem().name(), "grid" | "columns")
            })
            || capture.nearest(leaf, Kind::Label).is_none()
                && capture
                    .nearest(leaf, Kind::Table)
                    .is_some_and(|table| table > idx)
    })
}

pub(super) fn lower(
    capture: &Capture,
    idx: usize,
    page: usize,
    options: &Options,
) -> Result<Element> {
    let ids = &capture.nodes[idx].pages[&page].leaves;
    let mut removed = HashSet::new();
    let mut output = BTreeMap::<usize, Vec<Element>>::new();
    // A table owns all its cell contents, including lists nested in those cells.
    for table in capture.descendants(idx) {
        if capture.nodes[table].kind != Kind::Table
            || !capture.nodes[table]
                .parent
                .is_some_and(|p| belongs_to_list(capture, p, idx))
        {
            continue;
        }
        let Some(np) = capture.nodes[table]
            .pages
            .get(&page)
            .filter(|p| !p.leaves.is_empty())
        else {
            continue;
        };
        if np.leaves.iter().all(|&id| {
            capture
                .nearest(&capture.pages[page][id], Kind::Label)
                .is_some()
        }) {
            continue;
        }
        let element = structure(capture, table, page, &np.leaves, options)?;
        removed.extend(np.leaves.iter().copied());
        output.insert(
            np.leaves[0],
            links::attach(capture, page, &np.leaves, vec![element]),
        );
    }
    let mut prepared = capture.clone();
    for &id in ids {
        let leaf = &capture.pages[page][id];
        if removed.contains(&id) || !body_object(capture, leaf) {
            continue;
        }
        let elements = match &leaf.item {
            FrameItem::Shape(..) => crate::graphics::convert_with_options(leaf, options),
            FrameItem::Image(image, size, _) => {
                pictures::native_image(leaf, image, *size, options.image_dpi)
            }
            _ => unreachable!(),
        }
        .map_err(|error| diagnostics::at(error, Origin::leaf(capture, page, id)))?;
        output.insert(id, links::attach(capture, page, &[id], elements));
        removed.insert(id);
        // Reserve the source gap between text before and after an inline
        // graphic with a native tab. Block backgrounds have no paragraph owner.
        if let Some(owner) = capture.nearest(leaf, Kind::Paragraph) {
            prepared.horizontal_spaces[page].push((id + 1, Some(owner)));
        }
    }
    for node in &mut prepared.nodes {
        if let Some(np) = node.pages.get_mut(&page) {
            np.leaves.retain(|id| !removed.contains(id));
        }
    }
    if prepared.nodes[idx].pages[&page].leaves.iter().any(|&id| {
        (matches!(prepared.pages[page][id].item, FrameItem::Text(_))
            || prepared
                .nearest(&prepared.pages[page][id], Kind::Label)
                .is_some()
                && prepared.pages[page][id].is_drawable())
            && belongs_to_list(
                &prepared,
                *prepared.pages[page][id].ancestors.last().unwrap(),
                idx,
            )
    }) {
        for (order, block) in list_blocks(&prepared, idx, page, options, None, true)? {
            output.entry(order).or_default().push(Element::Text(block));
        }
    }
    Ok(Element::group(output.into_values().flatten().collect()))
}
