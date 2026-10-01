//! Freeze all variable-font axes while retaining every glyph for later editing.
use typst::text::FontInstance;

pub(super) fn family(font: &FontInstance) -> String {
    // A distinct family prevents Office's four style slots from conflating
    // different weights, optical sizes, or custom-axis values.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in font
        .data()
        .as_slice()
        .iter()
        .copied()
        .chain(font.index().to_be_bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    for axis in &font.info().axes {
        let value = font
            .variations()
            .0
            .iter()
            .find(|(tag, _)| *tag == axis.tag)
            .map_or(axis.default.0, |(_, v)| v.0);
        for byte in value.to_bits().to_be_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    let prefix: String = font.info().family.chars().take(12).collect();
    format!("{prefix} T{hash:016x}")
}

pub(super) fn materialize(font: &FontInstance) -> Option<Vec<u8>> {
    use write_fonts::{
        FontBuilder,
        from_obj::ToOwnedTable,
        read::{FontRef, TableProvider},
        types::NameId,
    };
    let mut axes = Vec::new();
    for axis in &font.info().axes {
        let value = font
            .variations()
            .0
            .iter()
            .find(|(tag, _)| *tag == axis.tag)
            .map_or(axis.default.0, |(_, v)| v.0);
        axes.push((u32::from_be_bytes(axis.tag.to_bytes()), value));
    }
    let blob = super::harfbuzz::instantiate(font.data().as_slice(), font.index(), &axes)?;
    let parsed = FontRef::new(&blob).ok()?;
    if parsed.fvar().is_ok() || parsed.cff2().is_ok() {
        return None;
    }
    let alias = family(font);
    let mut names: write_fonts::tables::name::Name = parsed.name().ok()?.to_owned_table();
    for n in &mut names.name_record {
        let value = match n.name_id.to_u16() {
            1 | 3 | 4 | 16 | 21 => alias.clone(),
            2 | 17 | 22 => "Regular".into(),
            6 => alias.chars().filter(char::is_ascii_alphanumeric).collect(),
            _ => continue,
        };
        n.string = value.into();
    }
    for id in [1, 2, 4, 6, 16, 17] {
        if !names
            .name_record
            .iter()
            .any(|n| n.platform_id == 3 && n.name_id == NameId::new(id))
        {
            let value = match id {
                2 | 17 => "Regular".into(),
                6 => alias.chars().filter(char::is_ascii_alphanumeric).collect(),
                _ => alias.clone(),
            };
            names
                .name_record
                .push(write_fonts::tables::name::NameRecord::new(
                    3,
                    1,
                    0x409,
                    NameId::new(id),
                    value.into(),
                ));
        }
    }
    names.name_record.sort();
    let mut head: write_fonts::tables::head::Head = parsed.head().ok()?.to_owned_table();
    head.mac_style = Default::default();
    let mut os2: write_fonts::tables::os2::Os2 = parsed.os2().ok()?.to_owned_table();
    os2.us_weight_class = 400;
    os2.us_width_class = 5;
    use write_fonts::tables::os2::SelectionFlags;
    os2.fs_selection
        .remove(SelectionFlags::ITALIC | SelectionFlags::BOLD | SelectionFlags::OBLIQUE);
    os2.fs_selection.insert(SelectionFlags::REGULAR);
    let mut builder = FontBuilder::new();
    builder
        .add_table(&names)
        .ok()?
        .add_table(&head)
        .ok()?
        .add_table(&os2)
        .ok()?;
    builder.copy_missing_tables(parsed);
    Some(builder.build())
}
