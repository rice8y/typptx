#[test]
fn harfbuzz_subset_flags_round_trip_through_the_native_api() {
    use hb_subset::{Flags, SubsetInput, sys};

    let mut input = SubsetInput::new().unwrap();
    {
        let mut flags = input.flags();
        *flags = Flags::default();
        flags.retain_glyph_indices().remove_hinting();
        flags.0 |= sys::hb_subset_flags_t::DOWNGRADE_CFF2;
    }
    let expected = sys::hb_subset_flags_t::RETAIN_GIDS
        | sys::hb_subset_flags_t::NO_HINTING
        | sys::hb_subset_flags_t::DOWNGRADE_CFF2;
    assert_eq!(input.flags().0, expected);

    input.flags().retain_hinting();
    assert_eq!(
        input.flags().0,
        sys::hb_subset_flags_t::RETAIN_GIDS | sys::hb_subset_flags_t::DOWNGRADE_CFF2
    );
    *input.flags() = Flags::default();
    assert_eq!(*input.flags(), Flags::default());
}

#[test]
fn harfbuzz_predefined_name_ids_preserve_values_and_the_invalid_sentinel() {
    use hb_subset::sys::{hb_ot_name_id_predefined_t as Predefined, hb_ot_name_id_t as NameId};

    for (predefined, expected) in [
        (Predefined::COPYRIGHT, 0),
        (Predefined::FONT_FAMILY, 1),
        (Predefined::FULL_NAME, 4),
        (Predefined::VARIATIONS_PS_PREFIX, 25),
        (Predefined::INVALID, 0xffff),
    ] {
        assert_eq!(NameId::from(predefined).0, expected);
    }
}

#[test]
fn variable_instances_are_distinct_native_typefaces_in_the_presentation() {
    use std::{collections::BTreeSet, fs, path::Path};
    use typptx::{ir::Element, lower, pptx, world::CompilerWorld};
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("fonts.typ");
    fs::write(&input,"#set page(width:600pt,height:400pt)\n#set text(font:\"Roboto\",size:24pt)\n#text(weight:300)[Light ABC]\n\n#text(weight:700)[Bold ABC]").unwrap();
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fonts");
    let (doc, warnings) = CompilerWorld::new(&input, None, &[fonts], &[])
        .unwrap()
        .compile()
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let used: BTreeSet<_> = p
        .slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
        .filter_map(|e| {
            if let Element::Text(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .flat_map(|t| &t.paragraphs)
        .flat_map(|p| &p.runs)
        .map(|r| &r.style.font)
        .collect();
    assert_eq!(used.len(), 2);
    assert_eq!(p.fonts.len(), 2);
    assert!(p.fonts.iter().all(|f| used.contains(&f.family)));
    pptx::write(&p).unwrap();
}

#[test]
fn cff2_fonts_are_embedded_without_replacing_the_native_paragraphs() {
    use std::{
        fs,
        io::{Cursor, Read},
        path::Path,
    };
    use typptx::{lower, pptx, world::CompilerWorld};
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("cff2.typ");
    fs::write(&input,"#set page(width:600pt,height:400pt)\n#set text(font:\"Source Serif 4\",size:24pt)\n#text(weight:300)[Hello World]\n\n#text(weight:700)[Hello World]").unwrap();
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fonts");
    let (doc, warnings) = CompilerWorld::new(&input, None, &[fonts], &[])
        .unwrap()
        .compile()
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert_eq!(p.fonts.len(), 2);
    assert_ne!(p.fonts[0].family, p.fonts[1].family);
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    assert!(!zip.file_names().any(|n| n.starts_with("ppt/media/")));
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("Hello World").count(), 2);
    for font in &p.fonts {
        assert!(xml.contains(&font.family));
    }
}
