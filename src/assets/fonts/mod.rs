//! Whole-font EOT embedding, as required by PowerPoint's font parts.
//! https://www.w3.org/submissions/EOT/#Version02
use crate::{compiler::capture::Capture, ir::*};
use std::collections::{BTreeMap, HashSet};
use typst::{
    layout::FrameItem,
    text::{FontInstance, FontStyle},
};
mod instance;

pub(crate) fn family(font: &FontInstance) -> String {
    if !font.info().axes.is_empty() {
        instance::family(font)
    } else {
        font.info().family.clone()
    }
}

pub(crate) fn baked(font: &FontInstance) -> bool {
    !font.info().axes.is_empty()
}

pub fn collect(
    capture: &Capture,
    document: &typst_layout::PagedDocument,
    output: &mut Presentation,
) {
    let mut used = HashSet::new();
    for slide in &output.slides {
        for element in slide.elements.iter().flat_map(Element::walk) {
            let paragraphs: Vec<_> = match element {
                Element::Text(t) => t.paragraphs.iter().collect(),
                Element::Table(t) => t.cells.iter().flat_map(|c| &c.paragraphs).collect(),
                _ => Vec::new(),
            };
            for paragraph in paragraphs {
                if let Some(Bullet::Character { font, .. }) = &paragraph.bullet {
                    used.insert((font.clone(), false, false));
                }
                for run in &paragraph.runs {
                    if run.math.is_none() && !run.text.trim().is_empty() {
                        used.insert((run.style.font.clone(), run.style.bold, run.style.italic));
                    }
                }
            }
        }
    }
    let mut fonts = BTreeMap::new();
    for (page, leaves) in capture.pages.iter().enumerate() {
        for leaf in leaves {
            collect_item_fonts(&leaf.item, page + 1, &used, &mut fonts);
        }
    }
    for (page, data) in document.pages().iter().enumerate() {
        if let Some(typst::visualize::Paint::Tiling(t)) = data.fill_or_white() {
            for (_, item) in t.frame().items() {
                collect_item_fonts(item, page + 1, &used, &mut fonts);
            }
        }
    }
    for ((family, bold, italic), (page, font)) in fonts {
        match eot(&font) {
            Some(data) => output.fonts.push(EmbeddedFont {
                family,
                bold,
                italic,
                data,
            }),
            None => {
                restore_family(output, &family, &font);
                let origin = capture.pages[page - 1]
                    .iter()
                    .enumerate()
                    .find_map(|(id, leaf)| {
                        if matches!(&leaf.item, FrameItem::Text(t) if t.font == font) {
                            crate::compiler::diagnostics::Origin::leaf(capture, page - 1, id)
                        } else {
                            None
                        }
                    });
                output.diagnostics.push(crate::compiler::diagnostics::from_error(
                    page, font.info().family.clone(), "font_not_embedded", "font",
                    anyhow::anyhow!("{}: font format or embedding permissions require this font to be installed in PowerPoint", font.info().family), origin,
                ));
            }
        }
    }
}

fn collect_item_fonts(
    item: &FrameItem,
    page: usize,
    used: &HashSet<(String, bool, bool)>,
    fonts: &mut BTreeMap<(String, bool, bool), (usize, FontInstance)>,
) {
    use typst::visualize::{ImageKind, Paint};
    match item {
        FrameItem::Text(text) => {
            let info = text.font.info();
            let key = (
                family(&text.font),
                !baked(&text.font) && info.variant.weight.to_number() >= 600,
                !baked(&text.font) && info.variant.style != FontStyle::Normal,
            );
            if used.contains(&key) {
                fonts.entry(key).or_insert((page, text.font.clone()));
            }
        }
        FrameItem::Image(image, _, _) => {
            if let ImageKind::Svg(svg) = image.kind() {
                collect_svg_fonts(svg.tree(), page, used, fonts);
            }
        }
        FrameItem::Group(g) => {
            for (_, item) in g.frame.items() {
                collect_item_fonts(item, page, used, fonts);
            }
        }
        FrameItem::Shape(s, _) => {
            for paint in s.fill.iter().chain(s.stroke.iter().map(|s| &s.paint)) {
                if let Paint::Tiling(t) = paint {
                    for (_, item) in t.frame().items() {
                        collect_item_fonts(item, page, used, fonts);
                    }
                }
            }
        }
        _ => {}
    }
}

fn editable(flags: u16) -> bool {
    flags & 0x0200 == 0 && (flags & 0x000e == 0 || flags & 0x0008 != 0)
}

fn eot(font: &FontInstance) -> Option<Vec<u8>> {
    if !font.info().axes.is_empty() {
        // Check permissions before changing the font or its family name.
        use write_fonts::read::TableProvider;
        let face =
            write_fonts::read::FontRef::from_index(font.data().as_slice(), font.index()).ok()?;
        if !editable(face.os2().ok()?.fs_type()) {
            return None;
        }
        let data = instance::materialize(font)?;
        let face = typst::text::Font::new(typst::foundations::Bytes::new(data), 0)?;
        let fixed = face.clone().instantiate(
            face.info().variant,
            typst::layout::Abs::pt(12.),
            &Default::default(),
        );
        return eot(&fixed);
    }
    let original = font.data().as_slice();
    let extracted;
    let data = if original.starts_with(b"ttcf") {
        let face = write_fonts::read::FontRef::from_index(original, font.index()).ok()?;
        let mut builder = write_fonts::FontBuilder::new();
        builder.copy_missing_tables(face);
        extracted = builder.build();
        extracted.as_slice()
    } else {
        if !matches!(original.get(..4)?, [0, 1, 0, 0] | b"OTTO") {
            return None;
        }
        original
    };
    let table = |tag: &[u8; 4]| -> Option<&[u8]> {
        let count = be16(data, 4)? as usize;
        for i in 0..count {
            let entry = data.get(12 + i * 16..28 + i * 16)?;
            if &entry[..4] == tag {
                let start = be32(entry, 8)? as usize;
                return data.get(start..start.checked_add(be32(entry, 12)? as usize)?);
            }
        }
        None
    };
    if table(b"fvar").is_some() {
        return None;
    }
    let os2 = table(b"OS/2")?;
    let flags = be16(os2, 8)?;
    if !editable(flags) {
        return None;
    }
    let mut out = vec![0u8; 80];
    out[4..8].copy_from_slice(&(u32::try_from(data.len()).ok()?).to_le_bytes());
    // Mac PowerPoint needs the version-2 root-string fields even when empty.
    out[8..12].copy_from_slice(&0x00020001u32.to_le_bytes());
    out[16..26].copy_from_slice(os2.get(32..42)?);
    out[26] = 1; // DEFAULT_CHARSET
    out[27] = u8::from(be16(os2, 62)? & 1 != 0);
    out[28..32].copy_from_slice(&(be16(os2, 4)? as u32).to_le_bytes());
    out[32..34].copy_from_slice(&flags.to_le_bytes());
    out[34..36].copy_from_slice(&0x504cu16.to_le_bytes());
    for (dest, src) in [(36, 42), (40, 46), (44, 50), (48, 54), (52, 78), (56, 82)] {
        out[dest..dest + 4].copy_from_slice(&be32(os2, src).unwrap_or(0).to_le_bytes());
    }
    out[60..64].copy_from_slice(&be32(table(b"head")?, 8)?.to_le_bytes());
    for id in [1, 2, 5, 4] {
        let name = font
            .ttf()
            .names()
            .into_iter()
            .filter(|n| n.name_id == id)
            .find_map(|n| n.to_string())
            .unwrap_or_default();
        let bytes: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&u16::try_from(bytes.len()).ok()?.to_le_bytes());
        out.extend_from_slice(&bytes);
    }
    out.extend_from_slice(&[0; 4]); // Padding5, empty RootString
    out.extend_from_slice(data);
    let len = u32::try_from(out.len()).ok()?;
    out[..4].copy_from_slice(&len.to_le_bytes());
    Some(out)
}

fn restore_family(output: &mut Presentation, alias: &str, font: &FontInstance) {
    let original = &font.info().family;
    let axis = |tag: &[u8; 4]| {
        font.variations()
            .0
            .iter()
            .find(|(t, _)| &t.to_bytes() == tag)
            .map(|(_, v)| v.0)
    };
    let bold = axis(b"wght").map_or(font.info().variant.weight.to_number() >= 600, |v| v >= 600.);
    let italic = font.info().variant.style != FontStyle::Normal
        || axis(b"ital").is_some_and(|v| v != 0.)
        || axis(b"slnt").is_some_and(|v| v != 0.);
    fn visit(e: &mut Element, alias: &str, original: &str, bold: bool, italic: bool) {
        let paragraphs: Vec<_> = match e {
            Element::Text(t) => t.paragraphs.iter_mut().collect(),
            Element::Table(t) => t.cells.iter_mut().flat_map(|c| &mut c.paragraphs).collect(),
            Element::Group(g) => {
                for e in &mut g.elements {
                    visit(e, alias, original, bold, italic);
                }
                return;
            }
            _ => return,
        };
        for p in paragraphs {
            if let Some(Bullet::Character { font, .. }) = &mut p.bullet
                && font == alias
            {
                *font = original.into();
            }
            for r in &mut p.runs {
                if r.style.font == alias {
                    r.style.font = original.into();
                    if alias != original {
                        r.style.bold = bold;
                        r.style.italic = italic;
                    }
                }
            }
        }
    }
    for s in &mut output.slides {
        for e in &mut s.elements {
            visit(e, alias, original, bold, italic);
        }
    }
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}
fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn collect_svg_fonts(
    tree: &usvg::Tree,
    page: usize,
    used: &HashSet<(String, bool, bool)>,
    fonts: &mut BTreeMap<(String, bool, bool), (usize, FontInstance)>,
) {
    fn sizes(group: &usvg::Group, out: &mut Vec<(usvg::fontdb::ID, f64)>) {
        for node in group.children() {
            match node {
                usvg::Node::Text(t) => {
                    for span in t.layouted() {
                        for glyph in &span.positioned_glyphs {
                            out.push((glyph.font, f64::from(glyph.font_size())));
                        }
                    }
                }
                usvg::Node::Group(g) => sizes(g, out),
                _ => (),
            }
            node.subroots(|root| sizes(root, out));
        }
    }
    let mut instances = Vec::new();
    sizes(tree.root(), &mut instances);
    for (id, size) in instances {
        let font = tree
            .fontdb()
            .with_face_data(id, |data, index| {
                typst::text::Font::new(typst::foundations::Bytes::new(data.to_vec()), index)
            })
            .flatten();
        if let Some(font) = font {
            let font = font.clone().instantiate(
                font.info().variant,
                typst::layout::Abs::pt(size),
                &Default::default(),
            );
            let info = font.info();
            let key = (
                family(&font),
                !baked(&font) && info.variant.weight.to_number() >= 600,
                !baked(&font) && info.variant.style != FontStyle::Normal,
            );
            if used.contains(&key) {
                fonts.entry(key).or_insert((page, font));
            }
        }
    }
}

#[cfg(test)]
mod tests;
