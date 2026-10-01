//! Preserve source clusters while mapping SVG shifts to paragraph run properties.
use super::*;
use std::ops::Range;
use typst::text::FontInstance;
use unicode_script::{Script, UnicodeScript};

pub(super) struct Cluster {
    pub source: Range<usize>,
    pub glyph: usize,
    pub spacing: f64,
    pub rtl: bool,
}

pub(super) fn resolve(
    text: &usvg::Text,
    chunk: &usvg::TextChunk,
    glyphs: &[&usvg::layout::PositionedGlyph],
    fonts: &[FontInstance],
    char_start: usize,
) -> Result<Vec<Cluster>> {
    ensure!(
        text.writing_mode() == usvg::WritingMode::LeftToRight,
        "SVG character offsets in vertical text need independent text layout"
    );
    // usvg resolves bidi with an LTR paragraph base, independently of text-anchor.
    let bidi = unicode_bidi::BidiInfo::new(chunk.text(), Some(unicode_bidi::Level::ltr()));
    let paragraph = bidi
        .paragraphs
        .first()
        .ok_or_else(|| anyhow!("empty SVG text chunk"))?;
    let (levels, visual_runs) = bidi.visual_runs(paragraph, paragraph.range.clone());
    let mixed = levels.iter().any(|l| l.is_rtl()) && levels.iter().any(|l| l.is_ltr());
    ensure!(
        !mixed || glyphs.iter().all(|g| !g.text.is_empty()),
        "mixed-direction SVG offsets with multi-glyph clusters need an unambiguous source mapping"
    );
    let mut clusters = Vec::new();
    let mut cursor = 0;
    for span in chunk.spans() {
        for visual in &visual_runs {
            let range = span.start().max(visual.start)..span.end().min(visual.end);
            if range.is_empty() {
                continue;
            }
            let rtl = levels[range.start].is_rtl();
            let mut at = if rtl { range.end } else { range.start };
            while (if rtl {
                at > range.start
            } else {
                at < range.end
            }) && cursor < glyphs.len()
            {
                let first = cursor;
                let content;
                if rtl {
                    content = glyphs[cursor].text.as_str();
                    ensure!(!content.is_empty(), "unmatched SVG text cluster");
                    cursor += 1;
                    while cursor < glyphs.len() && glyphs[cursor].text.is_empty() {
                        cursor += 1;
                    }
                } else {
                    while cursor < glyphs.len() && glyphs[cursor].text.is_empty() {
                        cursor += 1;
                    }
                    content = glyphs
                        .get(cursor)
                        .ok_or_else(|| anyhow!("unmatched SVG text cluster"))?
                        .text
                        .as_str();
                    cursor += 1;
                }
                let source = if rtl {
                    let end = at;
                    at = at
                        .checked_sub(content.len())
                        .ok_or_else(|| anyhow!("invalid SVG cluster range"))?;
                    at..end
                } else {
                    let start = at;
                    at += content.len();
                    start..at
                };
                ensure!(
                    source.start >= range.start
                        && source.end <= range.end
                        && chunk.text().get(source.clone()) == Some(content),
                    "SVG text clusters do not match logical source order"
                );
                // A combining glyph can be shifted relative to its base. Use the
                // advancing base glyph to measure the source cluster's baseline.
                let glyph = (first..cursor)
                    .max_by_key(|&i| fonts[i].ttf().glyph_hor_advance(glyphs[i].id).unwrap_or(0))
                    .unwrap();
                ensure!(
                    (first..cursor).all(|i| glyphs[i].font == glyphs[glyph].font),
                    "SVG offsets across a cluster with multiple fonts need independent text layout"
                );
                let start = char_start + chunk.text()[..source.start].chars().count();
                for i in start + 1..start + content.chars().count() {
                    ensure!(
                        text.dx().get(i).is_none_or(|v| v.abs() < 1e-6)
                            && text.dy().get(i).is_none_or(|v| v.abs() < 1e-6),
                        "SVG offsets inside a shaped cluster cannot remain editable as one cluster"
                    );
                }
                clusters.push(Cluster {
                    source,
                    glyph,
                    spacing: 0.,
                    rtl,
                });
            }
            ensure!(
                at == if rtl { range.start } else { range.end },
                "incomplete SVG text cluster mapping"
            );
        }
    }
    ensure!(
        cursor == glyphs.len(),
        "unmatched SVG glyphs after cluster mapping"
    );
    // Reconstruct visual cluster order from the bidi runs, independent of SVG
    // formatting spans. Never infer order by sorting glyph coordinates: large
    // negative dx values can reverse the actual positions and must be rejected.
    let mut visual = Vec::new();
    for range in &visual_runs {
        let mut indices: Vec<_> = (0..clusters.len())
            .filter(|&i| {
                clusters[i].source.start >= range.start && clusters[i].source.end <= range.end
            })
            .collect();
        indices.sort_by_key(|&i| clusters[i].source.start);
        if levels[range.start].is_rtl() {
            indices.reverse();
        }
        visual.extend(indices);
    }
    for pair in visual.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let a = glyphs[clusters[left].glyph].transform().tx;
        let b = glyphs[clusters[right].glyph].transform().tx;
        ensure!(
            b + 1e-6 >= a,
            "SVG character offsets that reverse character order need independent text layout"
        );
        let cp = char_start + chunk.text()[..clusters[right].source.start].chars().count();
        let spacing = f64::from(text.dx().get(cp).copied().unwrap_or(0.));
        if spacing.abs() < 1e-6 {
            continue;
        }
        // Map gaps inside a directional run. A gap at a bidi boundary does
        // not in general belong to the logical character's trailing edge.
        ensure!(
            levels[clusters[left].source.start] == levels[clusters[right].source.start],
            "SVG horizontal offsets at a text-direction boundary need independent layout"
        );
        let owner = if clusters[left].rtl { right } else { left };
        let content = &chunk.text()[clusters[owner].source.clone()];
        ensure!(
            chunk.text().chars().all(|ch| matches!(
                ch.script(),
                Script::Latin
                    | Script::Greek
                    | Script::Cyrillic
                    | Script::Hebrew
                    | Script::Han
                    | Script::Hiragana
                    | Script::Katakana
                    | Script::Hangul
                    | Script::Common
                    | Script::Inherited
            )),
            "SVG horizontal offsets in joining scripts cannot preserve native shaping"
        );
        ensure!(
            content.chars().count() == 1,
            "horizontal spacing inside a multi-character cluster cannot preserve its shaping"
        );
        clusters[owner].spacing = spacing;
    }
    clusters.sort_by_key(|c| c.source.start);
    Ok(clusters)
}
