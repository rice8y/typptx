//! Pair bibliography labels and entries on a shared native baseline.
use crate::compiler::capture::{Capture, Kind};
use crate::ir::*;
use crate::lower::text::{paragraph, push_run, text_clip, validate_text_leaves};
use anyhow::{Result, anyhow, ensure};
use typst::layout::FrameItem;

/// Typst lays out numbered bibliographies as a two-column grid. Recover the
/// label/body pairs from its ordered semantic tags, so Office
/// lays out each reference on one shared baseline with a hanging indent.
pub(super) fn bibliography(capture: &Capture, idx: usize, page: usize) -> Result<TextBlock> {
    let node = &capture.nodes[idx];
    let np = &node.pages[&page];
    validate_text_leaves(capture, page, &np.leaves)?;
    let descendants = capture.descendants(idx);
    // A grid cell's tag can end before its paginated text frame begins.
    // Pair the Lbl/BibEntry tags in source order instead of assuming that
    // every entry stays inside its cell's tag in the display list.
    let mut pairs = Vec::new();
    let mut label = None;
    for &n in &descendants {
        match capture.nodes[n].kind {
            Kind::Label => {
                ensure!(label.is_none(), "bibliography has consecutive labels");
                label = Some(n);
            }
            Kind::BibEntry => pairs.push((
                label
                    .take()
                    .ok_or_else(|| anyhow!("bibliography entry has no label"))?,
                n,
            )),
            _ => {}
        }
    }
    ensure!(label.is_none(), "bibliography label has no entry");
    let region = np.layout.map(|(_, bounds)| bounds).unwrap_or(np.context);
    let left = region.x;
    let mut measured = Vec::new();
    for (label, entry) in pairs {
        let Some(body) = capture.nodes[entry].pages.get(&page) else {
            continue;
        };
        if !body
            .leaves
            .iter()
            .any(|&id| matches!(capture.pages[page][id].item, FrameItem::Text(_)))
        {
            continue;
        }
        let mut p = paragraph(capture, page, &body.leaves)?;
        p.paragraph.alignment = "l".into();
        p.paragraph.margin_left = p.x - left;
        if let Some(prefix) = capture.nodes[label].pages.get(&page)
            && prefix
                .leaves
                .iter()
                .any(|&id| matches!(capture.pages[page][id].item, FrameItem::Text(_)))
        {
            let prefix = paragraph(capture, page, &prefix.leaves)?;
            ensure!(
                (prefix.first_baseline - p.first_baseline).abs() < 0.1,
                "bibliography label is not aligned with its entry"
            );
            p.paragraph.indent = prefix.x - p.x;
            let mut runs = prefix.paragraph.runs;
            while runs.last().is_some_and(|r| r.text.trim_end().is_empty()) {
                runs.pop();
            }
            if let Some(last) = runs.last_mut() {
                last.text = last.text.trim_end().to_owned();
                let mut tab = last.clone();
                tab.text = "\t".into();
                push_run(&mut runs, tab);
                p.paragraph.tab_stops.insert(0, 0.0);
                if let Some(line) = p.paragraph.lines.first_mut() {
                    line.tab_stops.insert(0, 0.0);
                }
            }
            for run in p.paragraph.runs {
                push_run(&mut runs, run);
            }
            p.paragraph.runs = runs;
            p.ascent = p.ascent.max(prefix.ascent);
            p.descent = p.descent.max(prefix.descent);
        }
        // A continuation on the next page has no repeated label; it begins
        // at the body column and retains the same hanging indentation.
        measured.push(p);
    }
    ensure!(!measured.is_empty(), "bibliography has no text entries");
    for i in 0..measured.len() - 1 {
        let gap = measured[i + 1].first_baseline - measured[i].last_baseline;
        measured[i].paragraph.space_after = (gap - measured[i].paragraph.line_spacing).max(0.0);
    }
    let first = measured.first().unwrap();
    let last = measured.last().unwrap();
    Ok(TextBlock {
        mirror_x: false,
        vertical: None,
        source_id: node.id(),
        role: "bibliography".into(),
        clip: text_clip(capture, page, &np.leaves)?,
        font_scale: None,
        bounds: Rect {
            x: left,
            y: first.first_baseline - first.ascent,
            width: region.width,
            height: last.last_baseline + last.descent - (first.first_baseline - first.ascent) + 2.0,
        },
        paragraphs: measured.into_iter().map(|p| p.paragraph).collect(),
        wrap: !capture.has_fixed_math_layout(page, &np.leaves),
    })
}
