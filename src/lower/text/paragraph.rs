//! Assemble styled runs and source line metrics into native paragraphs.
use crate::compiler::capture::{Capture, Kind};
use crate::graphics::rgba;
use crate::ir::*;
use crate::lower::structure::string;
use crate::lower::text::metrics::{is_raw_block, script_metrics};
use crate::lower::text::validate_text_leaves;
use anyhow::{Result, ensure};
use std::collections::HashSet;
use typst::layout::{Abs, FrameItem};
use typst::text::{FontFlags, FontStyle};
use typst::visualize::Paint;

pub(in crate::lower) struct MeasuredParagraph {
    pub(in crate::lower) paragraph: Paragraph,
    pub(in crate::lower) first_baseline: f64,
    pub(in crate::lower) last_baseline: f64,
    pub(in crate::lower) x: f64,
    pub(in crate::lower) ascent: f64,
    pub(in crate::lower) descent: f64,
    pub(in crate::lower) right: f64,
}

pub(in crate::lower) fn paragraph(
    capture: &Capture,
    page: usize,
    ids: &[usize],
) -> Result<MeasuredParagraph> {
    validate_text_leaves(capture, page, ids)?;
    let rtl = ids
        .iter()
        .find_map(|&id| {
            capture.pages[page][id]
                .ancestors
                .iter()
                .rev()
                .find_map(|&idx| {
                    capture.nodes[idx]
                        .content
                        .location()
                        .and_then(|loc| capture.paragraph_rtl.get(&loc).copied())
                })
        })
        .unwrap_or(false);
    let raw_block = is_raw_block(capture, page, ids);
    let svg_math = capture.has_fixed_math_layout(page, ids);
    let mut runs: Vec<Run> = Vec::new();
    let mut baselines: Vec<f64> = Vec::new();
    let mut ascent: f64 = 0.0;
    let mut descent: f64 = 0.0;
    let mut x = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut seen_math = HashSet::new();
    let mut previous_leaf = None;
    let mut tab_stops = Vec::new();
    let mut lines = vec![ParagraphLine {
        x: f64::INFINITY,
        right: f64::NEG_INFINITY,
        ..Default::default()
    }];
    for &id in ids {
        let leaf = &capture.pages[page][id];
        let FrameItem::Text(text) = &leaf.item else {
            continue;
        };
        let equation = capture.nearest(leaf, Kind::Equation);
        if let Some(eq) = equation
            && !seen_math.insert(eq)
        {
            continue;
        }
        let scale = leaf.scale();
        let (font_size, shift, baseline) = script_metrics(capture, page, leaf, text);
        let new_line = baselines.last().is_some_and(|v| (v - baseline).abs() > 0.1);
        if baselines.is_empty() || new_line {
            baselines.push(baseline);
        }
        let info = text.font.info();
        let style = TextStyle {
            rtl: unicode_bidi::BidiInfo::new(&text.text, None)
                .paragraphs
                .first()
                .is_some_and(|p| p.level.is_rtl()),
            font: crate::assets::fonts::family(&text.font),
            pitch_family: if info.flags.contains(FontFlags::MONOSPACE) {
                0x31
            } else if info.flags.contains(FontFlags::SERIF) {
                0x12
            } else {
                0x22
            },
            size: font_size * scale,
            baseline: shift * scale,
            letter_spacing: leaf.tracking * scale,
            kerning: leaf.kerning,
            bold: !crate::assets::fonts::baked(&text.font)
                && info.variant.weight.to_number() >= 600,
            italic: !crate::assets::fonts::baked(&text.font)
                && info.variant.style != FontStyle::Normal,
            underline: leaf
                .ancestors
                .iter()
                .any(|&i| capture.nodes[i].content.elem().name() == "underline"),
            strike: leaf
                .ancestors
                .iter()
                .any(|&i| capture.nodes[i].content.elem().name() == "strike"),
            color: match &text.fill {
                Paint::Solid(c) => rgba(c.clone()),
                _ => [0, 0, 0, 255],
            },
            fill: (!matches!(text.fill, Paint::Solid(_)))
                .then(|| crate::graphics::brush(&text.fill))
                .transpose()?,
            outline: text
                .stroke
                .as_ref()
                .map(|s| crate::graphics::stroke(s, scale))
                .transpose()?,
            language: text.lang.as_str().to_string(),
        };
        let hyperlink = leaf.ancestors.iter().rev().find_map(|&i| {
            if capture.nodes[i].content.elem().name() == "link" {
                string(&capture.nodes[i], "dest")
            } else {
                None
            }
        });
        let mut run = Run {
            // Typst inserts discretionary hyphens during line breaking. They
            // must not pin the old wrapping into the editable PowerPoint text.
            text: text.text.chars().filter(|&c| c != '\u{ad}').collect(),
            style,
            hyperlink,
            math: None,
            math_inline: false,
            advances: Vec::new(),
            source_line: baselines.len() - 1,
            source_width: Some(text.width().to_pt() * scale / (font_size * scale)),
        };
        let explicit_breaks = capture.line_breaks[page]
            .iter()
            .filter(|&&(before, owner)| {
                before <= id
                    && previous_leaf.is_none_or(|previous| before > previous)
                    && owner.is_some_and(|owner| leaf.ancestors.contains(&owner))
            })
            .count();
        // Code needs the source's visual line boundaries, including soft
        // wraps. Keep a single editable paragraph with native line breaks;
        // inline raw text in prose must still reflow with its paragraph.
        let breaks = explicit_breaks.max(usize::from((raw_block || svg_math) && new_line));
        if breaks > 0 {
            let mut br = run.clone();
            br.text = "\n".repeat(breaks);
            br.source_width = None;
            push_run(&mut runs, br);
            let gap = baselines
                .iter()
                .rev()
                .nth(1)
                .map(|previous| (baseline - previous) / breaks as f64);
            for _ in 0..breaks {
                lines.push(ParagraphLine {
                    x: f64::INFINITY,
                    right: f64::NEG_INFINITY,
                    spacing: gap,
                    ..Default::default()
                });
            }
        }
        let line = lines.last_mut().unwrap();
        line.x = line.x.min(leaf.position.0);
        line.right = line
            .right
            .max(leaf.position.0 + text.width().to_pt() * scale);
        let has_gap = previous_leaf.is_some_and(|previous| {
            let prev = &capture.pages[page][previous];
            let FrameItem::Text(prev_text) = &prev.item else {
                return false;
            };
            let right = if breaks > 0 {
                x
            } else {
                prev.position.0 + prev_text.width().to_pt() * prev.scale()
            };
            if rtl {
                prev.position.0 - (leaf.position.0 + text.width().to_pt() * scale) > 0.02
            } else {
                leaf.position.0 - right > 0.02
            }
        });
        if has_gap
            && capture.horizontal_spaces[page]
                .iter()
                .any(|&(before, owner)| {
                    before <= id
                        && previous_leaf.is_some_and(|previous| before > previous)
                        && owner.is_some_and(|owner| leaf.ancestors.contains(&owner))
                })
        {
            let mut tab = run.clone();
            tab.text = "\t".into();
            tab.source_width = None;
            push_run(&mut runs, tab);
            let tab = leaf.position.0
                + if rtl {
                    text.width().to_pt() * scale
                } else {
                    0.
                };
            tab_stops.push(tab);
            lines.last_mut().unwrap().tab_stops.push(tab);
        }
        previous_leaf = Some(id);
        if capture.inline_objects.contains(&(page, id))
            || equation.is_some_and(|eq| capture.svg_math.contains(&eq))
        {
            // The tab reserves the equation's realized advance. The picture
            // retains its original coordinates, independently of Office fonts.
            let target = leaf.position.0
                + if rtl {
                    0.
                } else {
                    text.width().to_pt() * scale
                };
            tab_stops.push(target);
            lines.last_mut().unwrap().tab_stops.push(target);
        } else if let Some(eq) = equation {
            let content = &capture.nodes[eq].content;
            let resolved = content
                .location()
                .and_then(|loc| capture.equations.get(&loc))
                .unwrap_or(content);
            let math = crate::math::convert(resolved)?;
            run.text = crate::math::plain(&math);
            run.math = Some(math);
            run.math_inline = crate::math::inline(resolved);
            // Match the font used by the Office Math writer in paragraph
            // defaults too; otherwise Office uses the source math font for
            // operator metrics and asks to embed it again when saving.
            run.style.font = "Cambria Math".into();
            run.style.pitch_family = 0x12;
            run.style.bold = false;
            run.style.italic = false;
            let eq_text: Vec<_> = capture.nodes[eq].pages[&page]
                .leaves
                .iter()
                .filter_map(|&j| {
                    let l = &capture.pages[page][j];
                    if let FrameItem::Text(t) = &l.item {
                        Some((l, t))
                    } else {
                        None
                    }
                })
                .collect();
            run.style.size = eq_text
                .iter()
                .map(|(l, t)| t.size.to_pt() * l.scale())
                .fold(run.style.size, f64::max);
            let equation_page = &capture.nodes[eq].pages[&page];
            run.source_width = equation_page.layout.map(|(_, b)| b.width / run.style.size);
            for (l, t) in eq_text {
                right = right.max(l.position.0 + t.width().to_pt() * l.scale());
            }
        }
        if run.math.is_some() || run.text != text.text.as_str() {
            push_run(&mut runs, run);
        } else {
            for part in crate::lower::text::shaping::segments(run, text, scale) {
                push_run(&mut runs, part);
            }
        }
        // Office positions the first baseline using font ascent, whereas Typst
        // commonly uses cap height as the top edge.
        ascent = ascent
            .max((text.font.metrics().ascender.at(Abs::pt(font_size)).to_pt() + shift) * scale);
        descent = descent
            .max((-text.font.metrics().descender.at(Abs::pt(font_size)).to_pt() - shift) * scale);
        x = x.min(leaf.position.0);
        right = right.max(leaf.position.0 + text.width().to_pt() * scale);
    }
    ensure!(!runs.is_empty(), "empty text block");
    let first = baselines[0];
    let last = *baselines.last().unwrap();
    let size = runs.iter().map(|r| r.style.size).fold(0.0, f64::max);
    let spacing = baselines
        .windows(2)
        .map(|b| b[1] - b[0])
        .filter(|d| *d > size * 0.5)
        .reduce(f64::min)
        .unwrap_or(size * 1.2);
    Ok(MeasuredParagraph {
        paragraph: Paragraph {
            rtl,
            margin_right: 0.,
            runs,
            lines: if svg_math {
                for line in &mut lines {
                    if !line.x.is_finite() {
                        line.x = x;
                    }
                    if !line.right.is_finite() {
                        line.right = right;
                    }
                    for tab in &mut line.tab_stops {
                        *tab = if rtl {
                            line.right - *tab
                        } else {
                            *tab - line.x
                        };
                    }
                    line.tab_stops.sort_by(f64::total_cmp);
                    line.tab_stops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
                }
                lines
            } else {
                Vec::new()
            },
            level: 0,
            bullet: None,
            margin_left: 0.0,
            indent: 0.0,
            alignment: ids
                .first()
                .and_then(|&id| capture.nearest(&capture.pages[page][id], Kind::Paragraph))
                .and_then(|idx| capture.nodes[idx].content.location())
                .and_then(|loc| capture.paragraph_alignment.get(&loc))
                .cloned()
                .unwrap_or_else(|| "l".into()),
            line_spacing: spacing,
            space_before: 0.0,
            space_after: 0.0,
            tab_stops: {
                let mut tabs: Vec<_> = tab_stops
                    .into_iter()
                    .map(|pos| if rtl { right - pos } else { pos - x })
                    .collect();
                tabs.sort_by(f64::total_cmp);
                tabs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
                tabs
            },
            break_latin: false,
        },
        first_baseline: first,
        last_baseline: last,
        x,
        ascent,
        descent,
        right,
    })
}

pub(in crate::lower) fn push_run(runs: &mut Vec<Run>, mut run: Run) {
    if !run.advances.is_empty()
        && !run
            .text
            .chars()
            .eq(run.advances.iter().map(|a| a.character))
    {
        run.advances.clear();
        run.source_width = None;
    }
    if let Some(last) = runs.last_mut().filter(|last| {
        last.style == run.style
            && last.hyperlink == run.hyperlink
            && last.math.is_none()
            && run.math.is_none()
            && last.source_line == run.source_line
            && last.advances.is_empty() == run.advances.is_empty()
    }) {
        last.text.push_str(&run.text);
        last.source_width = last.source_width.zip(run.source_width).map(|(a, b)| a + b);
        if last.advances.is_empty() || run.advances.is_empty() {
            last.advances.clear();
        } else {
            last.advances.extend(run.advances);
        }
    } else {
        runs.push(run);
    }
}
