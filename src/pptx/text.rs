//! Native paragraphs and text boxes mapped to DrawingML types.
use super::drawing::*;
use super::*;
use ooxmlsdk::{schemas::a14, units::TextPointValue};

macro_rules! character_properties {
    ($run:expr,$link:expr,$ty:ident,$field:ident,$choice:ident) => {{
        let s = &$run.style;
        // Office scales script runs to about two thirds of their nominal size.
        let size = if s.baseline.abs() > 0.001 {
            s.size * 1.5
        } else {
            s.size
        };
        a::$ty {
            language: Some(s.language.clone()),
            font_size: Some(centipt(size) as i32),
            baseline: Some(percentage(s.baseline / size)),
            spacing: Some(TextPointValue::Points100(centipt(s.letter_spacing) as i32)),
            // An omitted kern attribute disables kerning in Office table cells.
            // Use a positive threshold to enable the source's kerning setting.
            kerning: Some(if s.kerning { 1 } else { 0 }),
            bold: Some(s.bold.into()),
            italic: Some(s.italic.into()),
            underline: Some(enumeration(if s.underline { "sng" } else { "none" })),
            strike: Some(enumeration(if s.strike { "sngStrike" } else { "noStrike" })),
            dirty: Some(false.into()),
            right_to_left: Some(a::RightToLeft {
                val: Some(s.rtl.into()),
            }),
            $field: Some(match &s.fill {
                None => a::$choice::SolidFill(Box::new(solid(s.color))),
                Some(Brush::Solid { color }) => a::$choice::SolidFill(Box::new(solid(*color))),
                Some(b) => a::$choice::GradientFill(Box::new(gradient(b))),
            }),
            outline: s.outline.as_ref().map(|s| Box::new(outline(Some(s)))),
            latin_font: Some(a::LatinFont {
                typeface: Some(s.font.clone()),
                pitch_family: Some(s.pitch_family as i8),
                ..Default::default()
            }),
            east_asian_font: Some(a::EastAsianFont {
                typeface: Some(s.font.clone()),
                ..Default::default()
            }),
            complex_script_font: Some(a::ComplexScriptFont {
                typeface: Some(s.font.clone()),
                ..Default::default()
            }),
            hyperlink_on_click: $link.map(|link: &a::HyperlinkOnClick| Box::new(link.clone())),
            ..Default::default()
        }
    }};
}
pub(super) fn run_properties(run: &Run, link: Option<&a::HyperlinkOnClick>) -> a::RunProperties {
    let link = link.cloned().map(|mut link| {
        use ooxmlsdk::{common::XmlNamespace, namespaces::XmlKnownNamespace, schemas::ahyp};
        // A run's solid/gradient fill alone does not override the theme's
        // hyperlink color. Explicitly retain the source text fill in Office.
        link.hyperlink_extension_list = Some(a::HyperlinkExtensionList {
            hyperlink_extension: vec![a::HyperlinkExtension {
                uri: "{A12FA001-AC4F-418D-AE19-62706E023703}".into(),
                hyperlink_extension_choice: Some(a::HyperlinkExtensionChoice::HyperlinkColor(
                    ahyp::HyperlinkColor {
                        xmlns: vec![XmlNamespace::known(XmlKnownNamespace::Ahyp)],
                        val: ahyp::HyperlinkColorEnum::Tx,
                    },
                )),
            }],
            ..Default::default()
        });
        link
    });
    character_properties!(
        run,
        link.as_ref(),
        RunProperties,
        run_properties_choice1,
        RunPropertiesChoice
    )
}
fn default_properties(run: &Run) -> a::DefaultRunProperties {
    character_properties!(
        run,
        None::<&a::HyperlinkOnClick>,
        DefaultRunProperties,
        default_run_properties_choice1,
        DefaultRunPropertiesChoice
    )
}
fn end_properties(run: &Run) -> a::EndParagraphRunProperties {
    character_properties!(
        run,
        None::<&a::HyperlinkOnClick>,
        EndParagraphRunProperties,
        end_paragraph_run_properties_choice1,
        EndParagraphRunPropertiesChoice
    )
}
pub(super) fn properties(p: &Paragraph, rels: &Relationships) -> a::ParagraphProperties {
    let mut out = a::ParagraphProperties {
        level: Some(i32::from(p.level)),
        // PowerPoint mirrors paragraph margins when rtl is set: marL is
        // still the leading margin, while our IR stores physical sides.
        left_margin: Some(emu(if p.rtl { p.margin_right } else { p.margin_left }) as i32),
        right_margin: Some(emu(if p.rtl { p.margin_left } else { p.margin_right }) as i32),
        right_to_left: Some(p.rtl.into()),
        indent: Some(emu(p.indent) as i32),
        alignment: Some(enumeration(&p.alignment)),
        latin_line_break: Some(p.break_latin.into()),
        font_alignment: Some(enumeration("base")),
        line_spacing: Some(Box::new(a::LineSpacing {
            line_spacing_choice: Some(a::LineSpacingChoice::SpacingPoints(a::SpacingPoints {
                val: centipt(p.line_spacing) as i32,
            })),
        })),
        space_before: Some(Box::new(a::SpaceBefore {
            space_before_choice: Some(a::SpaceBeforeChoice::SpacingPoints(a::SpacingPoints {
                val: centipt(p.space_before) as i32,
            })),
        })),
        space_after: Some(Box::new(a::SpaceAfter {
            space_after_choice: Some(a::SpaceAfterChoice::SpacingPoints(a::SpacingPoints {
                val: centipt(p.space_after) as i32,
            })),
        })),
        ..Default::default()
    };
    out.paragraph_properties_choice4 = Some(match &p.bullet {
        Some(bullet @ Bullet::Picture { size, .. }) => {
            out.paragraph_properties_choice2 = Some(
                a::ParagraphPropertiesChoice2::BulletSizePoints(a::BulletSizePoints {
                    // PowerPoint draws picture bullets at 70% of this nominal
                    // point size. Keep the realized image height from Typst.
                    val: centipt(*size / 0.7) as i32,
                }),
            );
            a::ParagraphPropertiesChoice4::PictureBullet(Box::new(a::PictureBullet {
                blip: Box::new(rels.bullet(bullet)),
            }))
        }
        Some(Bullet::Character {
            character,
            font,
            color: c,
            size,
        }) => {
            out.paragraph_properties_choice1 = Some(a::ParagraphPropertiesChoice::BulletColor(
                Box::new(a::BulletColor {
                    bullet_color_choice: Some(a::BulletColorChoice::RgbColorModelHex(Box::new(
                        color(*c),
                    ))),
                }),
            ));
            out.paragraph_properties_choice2 = Some(
                a::ParagraphPropertiesChoice2::BulletSizePoints(a::BulletSizePoints {
                    val: centipt(*size) as i32,
                }),
            );
            out.paragraph_properties_choice3 =
                Some(a::ParagraphPropertiesChoice3::BulletFont(a::BulletFont {
                    typeface: Some(font.clone()),
                    ..Default::default()
                }));
            a::ParagraphPropertiesChoice4::CharacterBullet(a::CharacterBullet {
                char: character.clone(),
            })
        }
        Some(Bullet::Number { scheme, start }) => {
            a::ParagraphPropertiesChoice4::AutoNumberedBullet(a::AutoNumberedBullet {
                r#type: enumeration(scheme),
                start_at: start.map(|s| s as i32),
            })
        }
        None => a::ParagraphPropertiesChoice4::NoBullet,
    });
    if !p.tab_stops.is_empty() {
        out.tab_stop_list = Some(a::TabStopList {
            tab_stop: p
                .tab_stops
                .iter()
                .map(|pos| a::TabStop {
                    position: Some(coordinate32(
                        pos + if p.rtl { p.margin_right } else { p.margin_left },
                    )),
                    alignment: Some(enumeration(if p.rtl { "r" } else { "l" })),
                })
                .collect(),
        });
    }
    if let Some(run) = p
        .runs
        .iter()
        .find(|r| r.style.baseline == 0.)
        .or(p.runs.first())
    {
        let mut run = run.clone();
        run.style.baseline = 0.;
        out.default_run_properties = Some(Box::new(default_properties(&run)));
    }
    out
}

// The schema names the same paragraph attributes separately for each list level.
// Keep their mapping in one macro so fixes affect the paragraph and all levels.
macro_rules! list_level {
    ($p:expr,$rels:expr,$ty:ident,$f1:ident,$e1:ident,$f2:ident,$e2:ident,$f3:ident,$e3:ident,$f4:ident,$e4:ident) => {{
        let p = properties($p, $rels);
        Box::new(a::$ty {
            left_margin: p.left_margin,
            right_margin: p.right_margin,
            level: p.level,
            indent: p.indent,
            alignment: p.alignment,
            default_tab_size: p.default_tab_size,
            right_to_left: p.right_to_left,
            east_asian_line_break: p.east_asian_line_break,
            font_alignment: p.font_alignment,
            latin_line_break: p.latin_line_break,
            height: p.height,
            line_spacing: p.line_spacing,
            space_before: p.space_before,
            space_after: p.space_after,
            tab_stop_list: p.tab_stop_list,
            default_run_properties: p.default_run_properties,
            extension_list: p.extension_list,
            $f1: p.paragraph_properties_choice1.map(|v| match v {
                a::ParagraphPropertiesChoice::BulletColorText => a::$e1::BulletColorText,
                a::ParagraphPropertiesChoice::BulletColor(v) => a::$e1::BulletColor(v),
            }),
            $f2: p.paragraph_properties_choice2.map(|v| match v {
                a::ParagraphPropertiesChoice2::BulletSizeText => a::$e2::BulletSizeText,
                a::ParagraphPropertiesChoice2::BulletSizePercentage(v) => {
                    a::$e2::BulletSizePercentage(v)
                }
                a::ParagraphPropertiesChoice2::BulletSizePoints(v) => a::$e2::BulletSizePoints(v),
            }),
            $f3: p.paragraph_properties_choice3.map(|v| match v {
                a::ParagraphPropertiesChoice3::BulletFontText => a::$e3::BulletFontText,
                a::ParagraphPropertiesChoice3::BulletFont(v) => a::$e3::BulletFont(v),
            }),
            $f4: p.paragraph_properties_choice4.map(|v| match v {
                a::ParagraphPropertiesChoice4::NoBullet => a::$e4::NoBullet,
                a::ParagraphPropertiesChoice4::AutoNumberedBullet(v) => {
                    a::$e4::AutoNumberedBullet(v)
                }
                a::ParagraphPropertiesChoice4::CharacterBullet(v) => a::$e4::CharacterBullet(v),
                a::ParagraphPropertiesChoice4::PictureBullet(v) => a::$e4::PictureBullet(v),
            }),
        })
    }};
}
fn list_style(block: &TextBlock, rels: &Relationships) -> a::ListStyle {
    let mut style = a::ListStyle::default();
    if block.role != "list" || block.paragraphs.is_empty() {
        return style;
    }
    for level in 0..9 {
        let source = block
            .paragraphs
            .iter()
            .find(|p| p.level == level && p.bullet.is_some())
            .unwrap_or(&block.paragraphs[0]);
        let mut p = source.clone();
        p.level = level;
        p.margin_left += 18. * (f64::from(level) - f64::from(source.level));
        if let Some(Bullet::Number { start, .. }) = &mut p.bullet {
            *start = None;
        }
        match level {
            0 => {
                style.level1_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level1ParagraphProperties,
                    level1_paragraph_properties_choice1,
                    Level1ParagraphPropertiesChoice,
                    level1_paragraph_properties_choice2,
                    Level1ParagraphPropertiesChoice2,
                    level1_paragraph_properties_choice3,
                    Level1ParagraphPropertiesChoice3,
                    level1_paragraph_properties_choice4,
                    Level1ParagraphPropertiesChoice4
                ))
            }
            1 => {
                style.level2_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level2ParagraphProperties,
                    level2_paragraph_properties_choice1,
                    Level2ParagraphPropertiesChoice,
                    level2_paragraph_properties_choice2,
                    Level2ParagraphPropertiesChoice2,
                    level2_paragraph_properties_choice3,
                    Level2ParagraphPropertiesChoice3,
                    level2_paragraph_properties_choice4,
                    Level2ParagraphPropertiesChoice4
                ))
            }
            2 => {
                style.level3_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level3ParagraphProperties,
                    level3_paragraph_properties_choice1,
                    Level3ParagraphPropertiesChoice,
                    level3_paragraph_properties_choice2,
                    Level3ParagraphPropertiesChoice2,
                    level3_paragraph_properties_choice3,
                    Level3ParagraphPropertiesChoice3,
                    level3_paragraph_properties_choice4,
                    Level3ParagraphPropertiesChoice4
                ))
            }
            3 => {
                style.level4_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level4ParagraphProperties,
                    level4_paragraph_properties_choice1,
                    Level4ParagraphPropertiesChoice,
                    level4_paragraph_properties_choice2,
                    Level4ParagraphPropertiesChoice2,
                    level4_paragraph_properties_choice3,
                    Level4ParagraphPropertiesChoice3,
                    level4_paragraph_properties_choice4,
                    Level4ParagraphPropertiesChoice4
                ))
            }
            4 => {
                style.level5_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level5ParagraphProperties,
                    level5_paragraph_properties_choice1,
                    Level5ParagraphPropertiesChoice,
                    level5_paragraph_properties_choice2,
                    Level5ParagraphPropertiesChoice2,
                    level5_paragraph_properties_choice3,
                    Level5ParagraphPropertiesChoice3,
                    level5_paragraph_properties_choice4,
                    Level5ParagraphPropertiesChoice4
                ))
            }
            5 => {
                style.level6_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level6ParagraphProperties,
                    level6_paragraph_properties_choice1,
                    Level6ParagraphPropertiesChoice,
                    level6_paragraph_properties_choice2,
                    Level6ParagraphPropertiesChoice2,
                    level6_paragraph_properties_choice3,
                    Level6ParagraphPropertiesChoice3,
                    level6_paragraph_properties_choice4,
                    Level6ParagraphPropertiesChoice4
                ))
            }
            6 => {
                style.level7_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level7ParagraphProperties,
                    level7_paragraph_properties_choice1,
                    Level7ParagraphPropertiesChoice,
                    level7_paragraph_properties_choice2,
                    Level7ParagraphPropertiesChoice2,
                    level7_paragraph_properties_choice3,
                    Level7ParagraphPropertiesChoice3,
                    level7_paragraph_properties_choice4,
                    Level7ParagraphPropertiesChoice4
                ))
            }
            7 => {
                style.level8_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level8ParagraphProperties,
                    level8_paragraph_properties_choice1,
                    Level8ParagraphPropertiesChoice,
                    level8_paragraph_properties_choice2,
                    Level8ParagraphPropertiesChoice2,
                    level8_paragraph_properties_choice3,
                    Level8ParagraphPropertiesChoice3,
                    level8_paragraph_properties_choice4,
                    Level8ParagraphPropertiesChoice4
                ))
            }
            8 => {
                style.level9_paragraph_properties = Some(list_level!(
                    &p,
                    rels,
                    Level9ParagraphProperties,
                    level9_paragraph_properties_choice1,
                    Level9ParagraphPropertiesChoice,
                    level9_paragraph_properties_choice2,
                    Level9ParagraphPropertiesChoice2,
                    level9_paragraph_properties_choice3,
                    Level9ParagraphPropertiesChoice3,
                    level9_paragraph_properties_choice4,
                    Level9ParagraphPropertiesChoice4
                ))
            }
            _ => unreachable!(),
        }
    }
    style
}
/// Adapt source advances without splitting complex clusters or replacing the
/// containing editable paragraph. Office nominal advances round to 1/8pt;
/// source shaping and DrawingML's centipoint spacing use finer precision.
pub(super) fn table_paragraph(p: &Paragraph, available: f64) -> Paragraph {
    adapt_advances(p, Some(available))
}

fn safe_advances(run: &Run) -> bool {
    run.math.is_none()
        && run.style.baseline == 0.
        && !run.style.rtl
        && !run.advances.is_empty()
        && run
            .text
            .chars()
            .eq(run.advances.iter().map(|a| a.character))
}

fn adapt_advances(p: &Paragraph, available: Option<f64>) -> Paragraph {
    let mut out = p.clone();
    if p.rtl {
        return out;
    }
    out.runs.clear();
    let mut first = 0;
    while first < p.runs.len() {
        let line = p.runs[first].source_line;
        let end = first
            + p.runs[first..]
                .iter()
                .take_while(|r| r.source_line == line)
                .count();
        let mut target = 0.;
        let mut placed = 0.;
        let mut complete = true;
        let mut last_safe = None;
        let mut trailing_tracking = 0.;
        let mut remaining = p.runs[first..end]
            .iter()
            .map(|r| r.text.chars().count())
            .sum::<usize>();
        let last_visible = p.runs[first..end]
            .iter()
            .flat_map(|r| r.text.chars())
            .enumerate()
            .filter(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .last();
        let total = remaining;
        for run in &p.runs[first..end] {
            let adapt =
                safe_advances(run) && (available.is_some() || run.style.letter_spacing != 0.);
            if !adapt {
                remaining -= run.text.chars().count();
                out.runs.push(run.clone());
                // Hard breaks have no advance; an opaque cluster, equation or
                // script run has native metrics that this adapter cannot model.
                if run.math.is_some() || run.text.chars().any(|c| c != '\n') {
                    complete = false;
                    target = 0.;
                    placed = 0.;
                }
                continue;
            }
            let mut template = run.clone();
            template.text.clear();
            template.advances.clear();
            template.source_width = None;
            template.style.kerning = false; // Included in shaped advances.
            for advance in &run.advances {
                let index = total - remaining;
                remaining -= 1;
                // Typst zeroes trimmed trailing whitespace in the frame.
                // Keep its native width for future edits, and exclude it from
                // the measured line budget instead of cancelling its advance.
                if advance.character.is_whitespace()
                    && advance.shaped.abs() < 1e-9
                    && last_visible.is_none_or(|last| index > last)
                {
                    let mut part = template.clone();
                    part.text = advance.character.to_string();
                    part.style = run.style.clone();
                    out.runs.push(part);
                    continue;
                }
                let nominal =
                    (advance.nominal * centipt(run.style.size) as f64 / 100. * 8.).round() / 8.;
                // The source shaper already included explicit tracking.
                target += advance.shaped * run.style.size;
                let spacing = ((target - placed - nominal) * 100.).round() / 100.;
                placed += nominal + spacing;
                let mut part = template.clone();
                part.text = advance.character.to_string();
                part.style.letter_spacing = spacing;
                out.runs.push(part);
                last_safe = Some(out.runs.len() - 1);
                trailing_tracking = run.style.letter_spacing.max(0.);
            }
        }
        // Each source visual line has its own width budget. Soft wraps stay
        // soft: only the final advance changes, never a glyph origin or break.
        // Unknown native metrics make line fitting unsafe, but do not disable
        // advance compensation on the known spans around them.
        if complete
            && p.lines.is_empty()
            && let (Some(available), Some(last)) = (available, last_safe)
        {
            let indent = if line == 0 { p.indent.max(0.) } else { 0. };
            let width = available
                - (emu(p.margin_left) + emu(p.margin_right) + emu(indent)) as f64 / 12700.;
            // A reshaped soft line can retain the tracking after its last
            // visible glyph, even though Typst excluded it when fitting.
            if target <= width + trailing_tracking + 2. / 12700. {
                let budget = ((width + 1e-9) * 8.).floor() / 8.;
                if placed > budget {
                    out.runs[last].style.letter_spacing -=
                        ((placed - budget) * 100. - 1e-9).ceil() / 100.;
                }
            }
        }
        first = end;
    }
    let mut merged: Vec<Run> = Vec::new();
    for run in out.runs {
        if let Some(last) = merged.last_mut().filter(|last| {
            last.style == run.style
                && last.hyperlink == run.hyperlink
                && last.math.is_none()
                && run.math.is_none()
                && last.source_line == run.source_line
                && last.advances.is_empty()
                && run.advances.is_empty()
        }) {
            last.text.push_str(&run.text);
            last.source_width = last.source_width.zip(run.source_width).map(|(a, b)| a + b);
        } else {
            merged.push(run);
        }
    }
    out.runs = merged;
    out
}

pub(super) fn paragraphs(
    p: &Paragraph,
    rels: &mut Relationships,
    origin_x: f64,
    origin_right: f64,
) -> Vec<a::Paragraph> {
    let adapted;
    let p = if p
        .runs
        .iter()
        .any(|r| r.style.letter_spacing != 0. && safe_advances(r))
    {
        adapted = adapt_advances(p, None);
        &adapted
    } else {
        p
    };
    if !p.lines.is_empty() {
        let mut lines = vec![Vec::new()];
        for run in &p.runs {
            for (i, text) in run.text.split('\n').enumerate() {
                if i > 0 {
                    lines.push(Vec::new());
                }
                if !text.is_empty() {
                    let mut run = run.clone();
                    run.text = text.into();
                    lines.last_mut().unwrap().push(run);
                }
            }
        }
        let count = lines.len();
        return lines
            .into_iter()
            .enumerate()
            .flat_map(|(i, runs)| {
                let mut line = p.clone();
                line.runs = runs;
                line.lines.clear();
                if let Some(layout) = p.lines.get(i) {
                    line.tab_stops.clone_from(&layout.tab_stops);
                    if line.rtl {
                        line.margin_right = origin_right - layout.right;
                        line.alignment = "r".into();
                    } else {
                        line.margin_left = layout.x - origin_x;
                        line.alignment = "l".into();
                    }
                    if let Some(s) = layout.spacing {
                        line.line_spacing = s;
                    }
                }
                if i > 0 {
                    line.bullet = None;
                    line.indent = 0.;
                    line.space_before = 0.;
                }
                if i + 1 < count {
                    line.space_after = 0.;
                }
                paragraphs(&line, rels, origin_x, origin_right)
            })
            .collect();
    }
    let mut out = a::Paragraph {
        paragraph_properties: Some(Box::new(properties(p, rels))),
        ..Default::default()
    };
    for run in &p.runs {
        if let Some(expr) = &run.math {
            let equation = super::math::equation(expr, run);
            let math = if p.runs.len() == 1 && !run.math_inline {
                // PowerPoint centers a lone equation unless its OMML
                // paragraph explicitly carries the source alignment.
                use ooxmlsdk::schemas::m;
                a14::TextMathChoice::Paragraph(Box::new(m::Paragraph {
                    xmlns: equation.xmlns.clone(),
                    paragraph_properties: Some(Box::new(m::ParagraphProperties {
                        justification: Some(m::Justification {
                            val: enumeration(match p.alignment.as_str() {
                                "ctr" => "center",
                                "r" => "right",
                                _ => "left",
                            }),
                        }),
                    })),
                    paragraph_choice: vec![m::ParagraphChoice::OfficeMath(equation)],
                }))
            } else {
                if p.runs.len() == 1 {
                    // A lone oMath is promoted to display math by PowerPoint,
                    // enlarging fractions and centering them. A zero-width
                    // text run preserves the source's inline math layout.
                    out.paragraph_choice
                        .push(a::ParagraphChoice::Run(Box::new(a::Run {
                            run_properties: Some(Box::new(run_properties(run, None))),
                            text: "\u{200b}".into(),
                        })));
                }
                a14::TextMathChoice::OfficeMath(equation)
            };
            out.paragraph_choice
                .push(a::ParagraphChoice::TextMath(a14::TextMath {
                    xmlns: vec![ooxmlsdk::common::XmlNamespace::known(
                        ooxmlsdk::namespaces::XmlKnownNamespace::A14,
                    )],
                    text_math_choice: vec![math],
                }));
            continue;
        }
        let link = run.hyperlink.as_ref().map(|target| rels.hyperlink(target));
        for (i, t) in run.text.split('\n').enumerate() {
            if i > 0 {
                out.paragraph_choice
                    .push(a::ParagraphChoice::Break(Box::new(a::Break {
                        run_properties: Some(Box::new(run_properties(run, None))),
                    })));
            }
            if !t.is_empty() {
                out.paragraph_choice
                    .push(a::ParagraphChoice::Run(Box::new(a::Run {
                        run_properties: Some(Box::new(run_properties(run, link.as_ref()))),
                        text: t.into(),
                    })));
            }
        }
    }
    if let Some(last) = p.runs.last() {
        let mut end = last.clone();
        if end.style.baseline != 0. {
            if let Some(base) = p.runs.iter().rev().find(|r| r.style.baseline == 0.) {
                end.style = base.style.clone();
            } else {
                end.style.baseline = 0.;
            }
        }
        out.end_paragraph_run_properties = Some(Box::new(end_properties(&end)));
    }
    vec![out]
}
pub(super) fn textbox(block: &TextBlock, id: usize, rels: &mut Relationships) -> p::Shape {
    let bounds = block.clip.unwrap_or(block.bounds);
    p::Shape {
        non_visual_shape_properties: Box::new(shape_properties(
            id,
            format!("{} {id}", block.role),
            Some(block.source_id.clone()),
            true,
        )),
        shape_properties: Box::new(p::ShapeProperties {
            transform2_d: Some(Box::new(transform(bounds))),
            shape_properties_choice1: Some(rectangle_geometry()),
            shape_properties_choice2: Some(paint(None)),
            outline: Some(Box::new(outline(None))),
            ..Default::default()
        }),
        text_body: Some(Box::new(p::TextBody {
            body_properties: Box::new(a::BodyProperties {
                vertical: block.vertical.as_deref().map(enumeration),
                wrap: Some(enumeration(if block.wrap { "square" } else { "none" })),
                left_inset: Some(coordinate32(block.bounds.x - bounds.x)),
                top_inset: Some(coordinate32(block.bounds.y - bounds.y)),
                right_inset: Some(coordinate32(bounds.right() - block.bounds.right())),
                bottom_inset: Some(coordinate32(bounds.bottom() - block.bounds.bottom())),
                horizontal_overflow: block.clip.map(|_| enumeration("clip")),
                vertical_overflow: block.clip.map(|_| enumeration("clip")),
                anchor: Some(enumeration(if block.role == "equation" {
                    "ctr"
                } else {
                    "t"
                })),
                body_properties_choice1: Some(block.font_scale.map_or(
                    a::BodyPropertiesChoice::NoAutoFit,
                    |scale| {
                        a::BodyPropertiesChoice::NormalAutoFit(a::NormalAutoFit {
                            font_scale: Some(percentage(scale)),
                            line_space_reduction: Some(percentage(0.)),
                        })
                    },
                )),
                ..Default::default()
            }),
            list_style: Some(Box::new(list_style(block, rels))),
            paragraph: block
                .paragraphs
                .iter()
                .flat_map(|p| {
                    // Tight list boxes need the same advance rounding as cells
                    // so Office does not introduce an extra soft line break.
                    let fitted;
                    let p = if block.wrap && block.role == "list" {
                        fitted = adapt_advances(p, Some(block.bounds.width));
                        &fitted
                    } else {
                        p
                    };
                    paragraphs(p, rels, block.bounds.x, block.bounds.right())
                })
                .map(|mut p| {
                    if block.role == "equation" {
                        // Fixed text-line heights make a tall Office equation
                        // overflow upward even inside a centered text box.
                        p.paragraph_properties.as_mut().unwrap().line_spacing =
                            Some(Box::new(a::LineSpacing {
                                line_spacing_choice: Some(a::LineSpacingChoice::SpacingPercent(
                                    a::SpacingPercent {
                                        val: percentage(1.),
                                    },
                                )),
                            }));
                    }
                    p
                })
                .collect(),
        })),
        ..Default::default()
    }
}
