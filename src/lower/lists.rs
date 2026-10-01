//! Preserve list hierarchy, labels, numbering, and paragraph spacing.
mod rich;
use crate::compiler::capture::{Capture, Kind};
use crate::ir::*;
use crate::lower::structure::{boolean, integer, string};
use crate::lower::text::{paragraph, single_line_spacing, text_clip, validate_text_leaves};
use anyhow::{Result, anyhow, ensure};
pub(super) use rich::needed as needs_group;
use std::collections::{HashMap, HashSet};
use typst::foundations::{StyleChain, StyledElem, Value};
use typst::layout::FrameItem;
use typst::text::TextElem;

pub(super) fn list(
    capture: &Capture,
    idx: usize,
    page: usize,
    options: &crate::lower::Options,
) -> Result<Element> {
    if rich::needed(capture, idx, page) {
        rich::lower(capture, idx, page, options)
    } else {
        list_in_region(capture, idx, page, options, None).map(Element::Text)
    }
}

pub(super) fn list_in_region(
    capture: &Capture,
    idx: usize,
    page: usize,
    options: &crate::lower::Options,
    region: Option<Rect>,
) -> Result<TextBlock> {
    Ok(list_blocks(capture, idx, page, options, region, false)?
        .pop()
        .unwrap()
        .1)
}

fn list_blocks(
    capture: &Capture,
    idx: usize,
    page: usize,
    options: &crate::lower::Options,
    region: Option<Rect>,
    separate: bool,
) -> Result<Vec<(usize, TextBlock)>> {
    let (prepared, markers) = picture_markers(capture, idx, page, options.image_dpi)?;
    let capture = prepared.as_ref();
    let node = &capture.nodes[idx];
    let np = &node.pages[&page];
    validate_text_leaves(capture, page, &np.leaves)?;
    // Each list body and each actual paragraph within it are identified by
    // compiler tags. Nested bodies naturally interrupt the outer body.
    let mut groups: Vec<(usize, Option<usize>, Vec<usize>)> = Vec::new();
    for &id in &np.leaves {
        let leaf = &capture.pages[page][id];
        if !matches!(leaf.item, FrameItem::Text(_)) || capture.nearest(leaf, Kind::Label).is_some()
        {
            continue;
        }
        let body = capture
            .nearest(leaf, Kind::ItemBody)
            .ok_or_else(|| anyhow!("list has text outside an item body"))?;
        let par = capture.nearest(leaf, Kind::Paragraph).filter(|&p| p > body);
        if let Some((b, p, ids)) = groups
            .last_mut()
            .filter(|(b, p, _)| *b == body && *p == par)
        {
            let _ = (b, p);
            ids.push(id);
        } else {
            groups.push((body, par, vec![id]));
        }
    }
    // Empty bodies retain a semantic item tag and a realized marker.
    for body in capture
        .descendants(idx)
        .into_iter()
        .filter(|&i| capture.nodes[i].kind == Kind::ItemBody && belongs_to_list(capture, i, idx))
    {
        if capture.nodes[body].pages.contains_key(&page)
            && !groups.iter().any(|(b, _, _)| *b == body)
        {
            // A continued outer item may contain only its nested list on this
            // page. It has no repeated marker and needs no empty paragraph.
            let owner = owner_list(capture, body);
            let has_marker = (0..body)
                .rev()
                .find(|&i| capture.nodes[i].kind == Kind::Label && owner_list(capture, i) == owner)
                .and_then(|i| capture.nodes[i].pages.get(&page))
                .is_some_and(|np| {
                    np.leaves
                        .iter()
                        .any(|&id| matches!(capture.pages[page][id].item, FrameItem::Text(_)))
                });
            if has_marker {
                groups.push((body, None, vec![]));
            }
        }
    }
    groups.sort_by_key(|(body, _, ids)| {
        ids.first().copied().unwrap_or_else(|| {
            (0..*body)
                .rev()
                .find_map(|i| {
                    if capture.nodes[i].kind != Kind::Label {
                        return None;
                    }
                    capture.nodes[i].pages.get(&page)?.leaves.first().copied()
                })
                .unwrap_or(usize::MAX)
        })
    });
    ensure!(!groups.is_empty(), "list has no items");
    let mut measured = Vec::new();
    let mut centered_markers = Vec::new();
    let mut placements = Vec::new();
    let mut seen_bodies = HashSet::new();
    let mut last_numbers: HashMap<usize, (u32, u32)> = HashMap::new();
    let region = region.unwrap_or_else(|| {
        let mut bounds = np.layout.map(|(_, b)| b).unwrap_or(np.context);
        if separate {
            return bounds;
        }
        // Office rounds advances to 1/8pt. Intrinsically sized lists can
        // otherwise wrap their longest source line at the final glyph.
        // Native table cells supply their own fixed content region.
        bounds.width += 0.125;
        let rtl = groups
            .iter()
            .flat_map(|(_, _, ids)| ids)
            .find_map(|&id| {
                capture.pages[page][id]
                    .ancestors
                    .iter()
                    .rev()
                    .find_map(|&i| {
                        capture.nodes[i]
                            .content
                            .location()
                            .and_then(|loc| capture.paragraph_rtl.get(&loc).copied())
                    })
            })
            .unwrap_or(false);
        if rtl {
            bounds.x -= 0.125;
        }
        bounds
    });
    for (body_idx, par_idx, ids) in groups {
        let order = ids.first().copied().unwrap_or_else(|| {
            (0..body_idx)
                .rev()
                .filter(|&i| capture.nodes[i].kind == Kind::Label)
                .find_map(|i| capture.nodes[i].pages.get(&page)?.leaves.first().copied())
                .unwrap_or(usize::MAX)
        });
        let mut context = capture.nodes[par_idx.unwrap_or(body_idx)].pages[&page].context;
        if let Some((left, right)) = par_idx.and_then(|p| capture.grid_cell_extent(p, page)) {
            context.x = left;
            context.width = right - left;
        }
        let paragraph_region = Some(context);
        placements.push((order, paragraph_region, text_clip(capture, page, &ids)?));
        let list_idx =
            owner_list(capture, body_idx).ok_or_else(|| anyhow!("missing enclosing list"))?;
        let owner = &capture.nodes[list_idx];
        let mut level = 0;
        let mut ancestor = owner_list(capture, list_idx);
        while let Some(parent) = ancestor {
            level += 1;
            ancestor = owner_list(capture, parent);
        }
        ensure!(level <= 8, "PowerPoint supports at most nine list levels");
        let mut p = if ids.is_empty() {
            let label = (0..body_idx)
                .rev()
                .find(|&i| {
                    capture.nodes[i].kind == Kind::Label && owner_list(capture, i) == Some(list_idx)
                })
                .ok_or_else(|| anyhow!("missing empty-item marker"))?;
            let marker = capture.nodes[label]
                .pages
                .get(&page)
                .ok_or_else(|| anyhow!("empty list item has no marker on this page"))?;
            let mut p = paragraph(capture, page, &marker.leaves)?;
            p.paragraph.runs.truncate(1);
            p.paragraph.runs[0].text = "\u{200b}".into();
            p.paragraph.lines.clear();
            p.paragraph.alignment = if p.paragraph.rtl { "r" } else { "l" }.into();
            if p.paragraph.rtl {
                // An empty RTL body has no frame width: its tag sits at the
                // far left of the item. Use the resolved label/body gap.
                let resolved = owner
                    .content
                    .location()
                    .and_then(|loc| capture.resolved.get(&loc))
                    .and_then(|c| c.to_packed::<StyledElem>());
                let styles =
                    resolved.map_or_else(StyleChain::default, |s| StyleChain::new(&s.styles));
                let gap = if let Some(list) = owner.content.to_packed::<typst::model::ListElem>() {
                    list.body_indent.get(styles)
                } else if let Some(list) = owner.content.to_packed::<typst::model::EnumElem>() {
                    list.body_indent.get(styles)
                } else {
                    unreachable!()
                };
                let scale = capture.nodes[label].pages[&page]
                    .leaves
                    .first()
                    .map_or(1., |&id| capture.pages[page][id].scale());
                p.x -= gap.at(styles.resolve(TextElem::size)).to_pt() * scale;
            } else {
                p.x = capture.nodes[body_idx].pages[&page].origin.0;
            }
            p.right = p.x;
            p
        } else {
            paragraph(capture, page, &ids)?
        };
        if (p.last_baseline - p.first_baseline).abs() < 0.1
            && let Some(spacing) = single_line_spacing(capture, page, &ids)
        {
            p.paragraph.line_spacing = spacing;
        }
        p.paragraph.level = level;
        if p.paragraph.alignment == "ctr" {
            p.paragraph.margin_left = (context.x - region.x).max(0.);
            p.paragraph.margin_right = (region.right() - context.right()).max(0.);
        } else if p.paragraph.rtl {
            p.paragraph.margin_right = (region.right() - p.right).max(0.0);
        } else {
            p.paragraph.margin_left = (p.x - region.x).max(0.0);
        }
        if seen_bodies.insert(body_idx) {
            let label_idx = (0..body_idx).rev().find(|&i| {
                capture.nodes[i].kind == Kind::Label && owner_list(capture, i) == Some(list_idx)
            });
            let label_idx = label_idx.ok_or_else(|| anyhow!("missing list marker tag"))?;
            let label_node = &capture.nodes[label_idx];
            // A split item can continue on a later page without repeating its
            // marker. Such continuation paragraphs must not gain a new bullet.
            if let Some(label_page) = label_node.pages.get(&page)
                && label_page
                    .leaves
                    .iter()
                    .any(|&id| matches!(capture.pages[page][id].item, FrameItem::Text(_)))
            {
                let label = paragraph(capture, page, &label_page.leaves)?;
                let marker = label
                    .paragraph
                    .runs
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<String>();
                p.paragraph.indent = if p.paragraph.rtl {
                    p.right - label.right
                } else {
                    label.x - p.x
                };
                let mut depth = 0;
                let mut parent = owner_list(capture, list_idx);
                while let Some(idx) = parent {
                    depth += usize::from(capture.nodes[idx].kind == Kind::Enum);
                    parent = owner_list(capture, idx);
                }
                let scheme = string(owner, "numbering")
                    .and_then(|pattern| numbering_scheme(&pattern, depth));
                let automatic = owner.kind == Kind::Enum
                    && !boolean(owner, "reversed")
                    && !boolean(owner, "full");
                if let Some(marker) = markers.get(&label_idx) {
                    p.paragraph.bullet = Some(marker.clone());
                } else if let Some(scheme) = scheme.filter(|_| automatic) {
                    let ordinal = (0..body_idx)
                        .filter(|&i| {
                            capture.nodes[i].kind == Kind::ItemBody
                                && owner_list(capture, i) == Some(list_idx)
                        })
                        .count();
                    let children = owner.content.field_by_name("children").ok();
                    let mut number = integer(owner, "start").unwrap_or(1);
                    for n in 0..=ordinal {
                        if n > 0 {
                            number = number
                                .checked_add(1)
                                .ok_or_else(|| anyhow!("numbering overflow"))?;
                        }
                        if let Some(Value::Array(children)) = &children
                            && let Some(Value::Content(child)) = children.iter().nth(n)
                            && let Ok(Value::Int(explicit)) = child.field_by_name("number")
                        {
                            number = explicit;
                        }
                    }
                    if !(1..=32767).contains(&number) {
                        let style = &label.paragraph.runs[0].style;
                        p.paragraph.bullet = Some(Bullet::Character {
                            character: marker,
                            font: style.font.clone(),
                            color: style.color,
                            size: style.size,
                        });
                    } else {
                        let number = number as u32;
                        // PowerPoint treats a changed startAt as a new sequence. Keep the
                        // sequence start on every paragraph so 3,4,5 does not become 3,1,2.
                        let start = match last_numbers.get(&list_idx) {
                            Some(&(previous, start)) if previous + 1 == number => start,
                            _ => number,
                        };
                        last_numbers.insert(list_idx, (number, start));
                        // Separate text boxes have independent numbering sequences.
                        let start = Some(if separate { number } else { start });
                        p.paragraph.bullet = Some(Bullet::Number {
                            scheme: scheme.into(),
                            start,
                        });
                    }
                } else {
                    let style = &label.paragraph.runs[0].style;
                    p.paragraph.bullet = Some(Bullet::Character {
                        character: marker,
                        font: style.font.clone(),
                        color: style.color,
                        size: style.size,
                    });
                }
                if separate && p.paragraph.alignment == "ctr" {
                    // Typst centers only the body; Office centers the bullet
                    // and body together. Keep a native marker paragraph at
                    // its source anchor, leaving the complete body paragraph
                    // centered within its original wrapping width.
                    let mut marker = label.paragraph;
                    marker.runs.truncate(1);
                    marker.runs[0].text = "\u{200b}".into();
                    marker.runs[0].advances.clear();
                    marker.runs[0].source_width = None;
                    marker.lines.clear();
                    marker.rtl = p.paragraph.rtl;
                    marker.alignment = if marker.rtl { "r" } else { "l" }.into();
                    marker.level = p.paragraph.level;
                    marker.bullet = p.paragraph.bullet.take();
                    marker.indent = 0.;
                    marker.margin_left = 0.;
                    marker.margin_right = 0.;
                    marker.tab_stops.clear();
                    centered_markers.push((
                        label_page.leaves[0],
                        TextBlock {
                            vertical: None,
                            source_id: label_node.id(),
                            role: "list_marker".into(),
                            clip: text_clip(capture, page, &label_page.leaves)?,
                            font_scale: None,
                            bounds: Rect {
                                x: label.x,
                                y: label.first_baseline - label.ascent,
                                width: label.right - label.x,
                                height: label.last_baseline - label.first_baseline
                                    + label.ascent
                                    + label.descent
                                    + 2.,
                            },
                            paragraphs: vec![marker],
                            wrap: false,
                        },
                    ));
                    p.paragraph.indent = 0.;
                }
            }
        }
        measured.push(p);
    }
    for i in 0..measured.len() - 1 {
        let gap = measured[i + 1].first_baseline - measured[i].last_baseline;
        // DrawingML uses the following paragraph's line spacing to place its
        // first line. Extra item spacing is independent of that line height.
        measured[i].paragraph.space_after = (gap - measured[i + 1].paragraph.line_spacing).max(0.0);
    }
    if separate {
        let blocks = measured
            .into_iter()
            .zip(placements)
            .map(|(mut p, (order, context, clip))| {
                p.paragraph.space_after = 0.;
                let mut bounds = Rect {
                    x: region.x,
                    y: p.first_baseline - p.ascent,
                    width: region.width,
                    height: p.last_baseline - p.first_baseline + p.ascent + p.descent + 2.,
                };
                if let Some(context) = context
                    && !matches!(p.paragraph.alignment.as_str(), "ctr" | "r")
                {
                    // Typst can flatten the first column into its parent
                    // frame. Its longest realized line still bounds the
                    // available width, including inset callout paragraphs.
                    bounds.width = context.right().min(p.right + 1.) - region.x;
                    if p.paragraph.rtl {
                        p.paragraph.margin_right = (bounds.right() - p.right).max(0.);
                    }
                }
                if p.paragraph.alignment == "r" {
                    bounds.x = p.x;
                    bounds.width = p.right - p.x + 0.1;
                    p.paragraph.margin_left = 0.;
                    p.paragraph.margin_right = 0.;
                }
                (
                    order,
                    TextBlock {
                        vertical: None,
                        source_id: node.id(),
                        role: "list".into(),
                        clip,
                        font_scale: None,
                        bounds,
                        paragraphs: vec![p.paragraph],
                        wrap: !capture.has_fixed_math_layout(page, &np.leaves),
                    },
                )
            });
        centered_markers.extend(blocks);
        return Ok(centered_markers);
    }
    let first = measured.first().unwrap();
    let last = measured.last().unwrap();
    let bounds = Rect {
        x: region.x,
        y: first.first_baseline - first.ascent,
        width: region.width,
        height: last.last_baseline + last.descent - (first.first_baseline - first.ascent) + 2.0,
    };
    Ok(vec![(
        np.leaves[0],
        TextBlock {
            vertical: None,
            source_id: node.id(),
            role: "list".into(),
            clip: text_clip(capture, page, &np.leaves)?,
            font_scale: None,
            bounds,
            paragraphs: measured.into_iter().map(|p| p.paragraph).collect(),
            wrap: !capture.has_fixed_math_layout(page, &np.leaves),
        },
    )])
}

/// Whether a label needs more styling or layout than a character bullet can
/// carry. Keep simple labels native; render the complete realized marker when
/// its math, transforms, or per-run formatting would otherwise be discarded.
fn needs_picture_marker(capture: &Capture, label_idx: usize, page: usize, ids: &[usize]) -> bool {
    if ids.iter().any(|&id| {
        let leaf = &capture.pages[page][id];
        !matches!(leaf.item, FrameItem::Text(_))
            || !leaf.plain_transform()
            || capture.nearest(leaf, Kind::Equation).is_some()
    }) {
        return true;
    }
    let Ok(label) = paragraph(capture, page, ids) else {
        return true;
    };
    if (label.first_baseline - label.last_baseline).abs() > 0.1 {
        return true;
    }
    let Some(first) = label.paragraph.runs.first() else {
        return false;
    };
    // Automatic numbering inherits its paragraph's character properties.
    // Ordinary bold/italic numbered lists must remain automatically numbered.
    if let Some(owner) = owner_list(capture, label_idx)
        && capture.nodes[owner].kind == Kind::Enum
        && !boolean(&capture.nodes[owner], "reversed")
        && !boolean(&capture.nodes[owner], "full")
        && string(&capture.nodes[owner], "numbering")
            .and_then(|pattern| numbering_scheme(&pattern, 0))
            .is_some()
        && let Some(body) = (label_idx + 1..capture.nodes.len()).find(|&i| {
            capture.nodes[i].kind == Kind::ItemBody && owner_list(capture, i) == Some(owner)
        })
        && let Some(np) = capture.nodes[body].pages.get(&page)
        && let Some(&id) = np
            .leaves
            .iter()
            .find(|&&id| matches!(capture.pages[page][id].item, FrameItem::Text(_)))
        && let Ok(body) = paragraph(capture, page, &[id])
        && let Some(body_run) = body.paragraph.runs.first()
        && label
            .paragraph
            .runs
            .iter()
            .all(|r| r.math.is_none() && r.style == body_run.style)
    {
        return false;
    }
    label.paragraph.runs.iter().any(|r| {
        let s = &r.style;
        r.math.is_some()
            || s.font != first.style.font
            || s.color != first.style.color
            || s.size != first.style.size
            || s.bold
            || s.italic
            || s.underline
            || s.strike
            || s.baseline != 0.
            || s.letter_spacing != 0.
            || s.outline.is_some()
            || s.fill
                .as_ref()
                .is_some_and(|fill| !matches!(fill, Brush::Solid { .. }))
    })
}

/// Replace rich labels with layout placeholders while the list's
/// paragraphs are measured. The actual marker is stored as a picture bullet.
fn picture_markers(
    capture: &Capture,
    idx: usize,
    page: usize,
    dpi: Option<u32>,
) -> Result<(std::borrow::Cow<'_, Capture>, HashMap<usize, Bullet>)> {
    use std::borrow::Cow;
    use typst::{
        layout::{Em, Transform},
        syntax::Span,
        text::Glyph,
    };
    let mut prepared = Cow::Borrowed(capture);
    let mut markers = HashMap::new();
    for label in capture.descendants(idx) {
        let node = &capture.nodes[label];
        if node.kind != Kind::Label || !belongs_to_list(capture, label, idx) {
            continue;
        }
        let Some(np) = node.pages.get(&page) else {
            continue;
        };
        let ids: Vec<_> = np
            .leaves
            .iter()
            .copied()
            .filter(|&id| capture.pages[page][id].is_drawable())
            .collect();
        if ids.is_empty() || !needs_picture_marker(capture, label, page, &ids) {
            continue;
        }
        let id = ids[0];
        let leaves: Vec<_> = ids.iter().map(|&id| &capture.pages[page][id]).collect();
        let Element::Picture {
            bounds,
            extension,
            bytes,
            svg,
            clip: None,
            ..
        } = crate::lower::pictures::picture_marker(&leaves, dpi)?
        else {
            unreachable!("picture markers are normalized to one picture");
        };
        markers.insert(
            label,
            Bullet::Picture {
                size: bounds.height,
                extension,
                bytes,
                svg,
            },
        );
        let mut text = capture.pages[page]
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                if let FrameItem::Text(t) = &l.item {
                    Some((i, t))
                } else {
                    None
                }
            })
            .min_by_key(|(i, _)| i.abs_diff(id))
            .map(|(_, t)| t.clone())
            .unwrap_or_else(crate::math::svg::empty_text);
        text.text = "\u{200b}".into();
        text.stroke = None;
        text.fill = typst::visualize::Paint::Solid(typst::visualize::Color::BLACK);
        text.glyphs = vec![Glyph {
            id: 0,
            x_advance: Em::new(bounds.width / text.size.to_pt()),
            x_offset: Em::zero(),
            y_advance: Em::zero(),
            y_offset: Em::zero(),
            range: 0..3,
            span: (Span::detached(), 0),
        }];
        let leaf = &mut prepared.to_mut().pages[page][id];
        leaf.item = FrameItem::Text(text);
        leaf.position = (bounds.x, np.baseline.unwrap_or(bounds.bottom()));
        leaf.transform = Transform::identity();
        // The crop is already inside the marker picture; the invisible layout
        // placeholder must not impose that small clip on the list body.
        leaf.clipped = false;
        leaf.clips.clear();
        // The marker's internal equations/styles are now part of its picture.
        // Keep only the label's ancestry for native paragraph measurement.
        if let Some(at) = leaf.ancestors.iter().position(|&i| i == label) {
            leaf.ancestors.truncate(at + 1);
        }
        let removed: HashSet<_> = ids.into_iter().skip(1).collect();
        for node in &mut prepared.to_mut().nodes {
            if let Some(np) = node.pages.get_mut(&page) {
                np.leaves.retain(|id| !removed.contains(id));
            }
        }
    }
    Ok((prepared, markers))
}

pub(super) fn owner_list(capture: &Capture, idx: usize) -> Option<usize> {
    let mut parent = capture.nodes[idx].parent;
    while let Some(i) = parent {
        if matches!(capture.nodes[i].kind, Kind::List | Kind::Enum) {
            return Some(i);
        }
        if matches!(capture.nodes[i].kind, Kind::Table | Kind::Cell) {
            return None;
        }
        parent = capture.nodes[i].parent;
    }
    None
}

fn belongs_to_list(capture: &Capture, idx: usize, list: usize) -> bool {
    let mut parent = Some(idx);
    while let Some(i) = parent {
        if i == list {
            return true;
        }
        if matches!(capture.nodes[i].kind, Kind::Table | Kind::Cell) {
            return false;
        }
        parent = capture.nodes[i].parent;
    }
    false
}

fn numbering_scheme(pattern: &str, depth: usize) -> Option<&'static str> {
    // Follow Typst's apply_kth: repeat the last counting symbol at deeper levels,
    // with the first prefix and the pattern's suffix.
    let parsed: typst::model::NumberingPattern = pattern.parse().ok()?;
    let system = parsed.pieces.get(depth).or_else(|| parsed.pieces.last())?.1;
    let single = format!(
        "{}{}{}",
        parsed.pieces.first()?.0,
        system.shorthand()?,
        parsed.suffix
    );
    Some(match single.as_str() {
        "1." => "arabicPeriod",
        "1)" => "arabicParenR",
        "(1)" => "arabicParenBoth",
        "1" => "arabicPlain",
        "a." => "alphaLcPeriod",
        "A." => "alphaUcPeriod",
        "a)" => "alphaLcParenR",
        "A)" => "alphaUcParenR",
        "(a)" => "alphaLcParenBoth",
        "(A)" => "alphaUcParenBoth",
        "i." => "romanLcPeriod",
        "I." => "romanUcPeriod",
        "i)" => "romanLcParenR",
        "I)" => "romanUcParenR",
        _ => return None,
    })
}
