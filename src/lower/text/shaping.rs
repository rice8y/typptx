//! Preserve source shaping boundaries without breaking a grapheme or ligature.
//!
//! A source text item can contain both ordinary characters and a complex
//! cluster. Keeping only an all-or-nothing advance vector loses the useful
//! metrics on either side. Formatting runs are the portable boundary: each
//! cluster remains intact and the containing native paragraph stays editable.
use crate::ir::{Run, TextAdvance};
use typst::text::TextItem;
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn segments(run: Run, text: &TextItem, scale: f64) -> Vec<Run> {
    // Contextual and reordered scripts need the complete source run for native
    // shaping. Do not introduce character-level formatting into their joins.
    if run.style.rtl
        || text.text.chars().any(|c| !simple_script(c))
        || text
            .glyphs
            .windows(2)
            .any(|g| g[0].range().start > g[1].range().start)
        || text.glyphs.iter().any(|g| {
            let r = g.range();
            r.start > r.end
                || r.end > text.text.len()
                || !text.text.is_char_boundary(r.start)
                || !text.text.is_char_boundary(r.end)
        })
    {
        return vec![run];
    }
    let graphemes: Vec<_> = text
        .text
        .grapheme_indices(true)
        .map(|(start, s)| start..start + s.len())
        .collect();
    let mut result = Vec::new();
    let mut gi = 0;
    let mut ci = 0;
    let mut template = run.clone();
    template.text.clear();
    template.advances.clear();
    while ci < graphemes.len() {
        let start = graphemes[ci].start;
        let mut end = graphemes[ci].end;
        let first_glyph = gi;
        // Union shaping clusters and graphemes. A ligature may cover several
        // graphemes; a decomposed accent may contain several shaping clusters.
        loop {
            while gi < text.glyphs.len() && text.glyphs[gi].range().start < end {
                end = end.max(text.glyphs[gi].range().end);
                gi += 1;
            }
            while ci < graphemes.len() && graphemes[ci].start < end {
                end = end.max(graphemes[ci].end);
                ci += 1;
            }
            if gi == text.glyphs.len() || text.glyphs[gi].range().start >= end {
                break;
            }
        }
        let glyphs = &text.glyphs[first_glyph..gi];
        let mut part = template.clone();
        part.text = text.text[start..end].to_string();
        let width = glyphs
            .iter()
            .map(|g| g.x_advance.at(text.size).to_pt())
            .sum::<f64>()
            * scale;
        part.source_width = Some(width / part.style.size);
        if let [glyph] = glyphs
            && let Some(character) = part.text.chars().next()
            && part.text.chars().count() == 1
            && glyph.range() == (start..end)
            && glyph.x_offset.abs().at(text.size).to_pt() < 1e-6
            && glyph.y_offset.abs().at(text.size).to_pt() < 1e-6
            && text.font.ttf().glyph_index(character).map(|g| g.0) == Some(glyph.id)
            && !character.is_control()
            && character != '\u{ad}'
            && let Some(nominal) = text.font.x_advance(glyph.id)
        {
            part.advances.push(TextAdvance {
                character,
                nominal: nominal.at(text.size).to_pt() / text.size.to_pt(),
                shaped: width / part.style.size,
            });
        }
        result.push(part);
    }
    if gi == text.glyphs.len() {
        result
    } else {
        vec![run]
    }
}

fn simple_script(c: char) -> bool {
    matches!(
        c.script(),
        Script::Latin
            | Script::Greek
            | Script::Cyrillic
            | Script::Han
            | Script::Hiragana
            | Script::Katakana
            | Script::Hangul
            | Script::Bopomofo
            | Script::Common
            | Script::Inherited
    )
}
