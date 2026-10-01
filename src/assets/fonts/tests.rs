use super::*;
#[test]
fn variable_fonts_keep_all_glyphs_and_freeze_the_requested_axes() {
    use typst::{
        foundations::Bytes,
        layout::Abs,
        text::{Font, FontWeight},
    };
    let source = Font::new(
        Bytes::new(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fonts/RobotoCondensed.ttf"
            ))
            .as_slice(),
        ),
        0,
    )
    .unwrap();
    let mut families = Vec::new();
    let mut widths = Vec::new();
    for weight in [300, 700] {
        let mut variant = source.info().variant;
        variant.weight = FontWeight::from_number(weight);
        let original = source
            .clone()
            .instantiate(variant, Abs::pt(24.), &Default::default());
        let eot = eot(&original).expect("static variable-font instance");
        let length = u32::from_le_bytes(eot[4..8].try_into().unwrap()) as usize;
        let data = &eot[eot.len() - length..];
        let font = Font::new(Bytes::new(data.to_vec()), 0).unwrap();
        assert!(font.info().axes.is_empty());
        assert_eq!(font.info().family, family(&original));
        let fixed =
            font.clone()
                .instantiate(font.info().variant, Abs::pt(24.), &Default::default());
        assert_eq!(
            fixed.ttf().number_of_glyphs(),
            original.ttf().number_of_glyphs()
        );
        for ch in ['A', 'W', 'm', 'é', 'Ω', 'Ж'] {
            let before = original.ttf().glyph_index(ch).unwrap();
            let after = fixed.ttf().glyph_index(ch).unwrap();
            let a = original.ttf().glyph_hor_advance(before).unwrap();
            let b = fixed.ttf().glyph_hor_advance(after).unwrap();
            assert!(a.abs_diff(b) <= 1, "{ch} advance: {a} != {b}");
        }
        widths.push(
            fixed
                .ttf()
                .glyph_hor_advance(fixed.ttf().glyph_index('m').unwrap())
                .unwrap(),
        );
        families.push(font.info().family.clone());
    }
    assert_ne!(families[0], families[1]);
    assert_ne!(widths[0], widths[1]);
}
#[test]
fn cff2_instances_keep_glyphs_metrics_and_outlines_in_static_cff() {
    use typst::{
        foundations::Bytes,
        layout::Abs,
        text::{Font, FontWeight},
    };
    use write_fonts::read::{FontRef, TableProvider};
    let source = Font::new(
        Bytes::new(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fonts/SourceSerif4Variable-HelloWorld.otf"
            ))
            .as_slice(),
        ),
        0,
    )
    .unwrap();
    let mut outlines = Vec::new();
    let references: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/fonts/SourceSerif4-reference.json"
    )))
    .unwrap();
    for reference in references.as_array().unwrap() {
        let weight = reference["weight"].as_u64().unwrap() as u16;
        let size = reference["size"].as_f64().unwrap();
        let mut variant = source.info().variant;
        variant.weight = FontWeight::from_number(weight);
        let original = source
            .clone()
            .instantiate(variant, Abs::pt(size), &Default::default());
        let bytes = eot(&original).expect("CFF2 becomes an editable static font");
        let length = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let data = &bytes[bytes.len() - length..];
        let tables = FontRef::new(data).unwrap();
        assert!(tables.cff().is_ok());
        assert!(tables.cff2().is_err());
        assert!(tables.fvar().is_err());
        let fixed = Font::new(Bytes::new(data.to_vec()), 0).unwrap();
        assert_eq!(fixed.info().family, family(&original));
        let fixed =
            fixed
                .clone()
                .instantiate(fixed.info().variant, Abs::pt(size), &Default::default());
        assert_eq!(
            fixed.ttf().number_of_glyphs(),
            original.ttf().number_of_glyphs()
        );
        let mut boxes = Vec::new();
        for ch in "HelloWorld".chars() {
            let before = original.ttf().glyph_index(ch).unwrap();
            let after = fixed.ttf().glyph_index(ch).unwrap();
            let a = original.ttf().glyph_hor_advance(before).unwrap();
            let b = fixed.ttf().glyph_hor_advance(after).unwrap();
            assert!(a.abs_diff(b) <= 1, "{ch}: {a} != {b}");
            let expected = &reference["glyphs"][ch.to_string()];
            assert!(
                (expected["advance"].as_f64().unwrap() - f64::from(b)).abs() <= 1.,
                "{weight}/{size} {ch} advance differs from the independent reference"
            );
            let b = fixed.ttf().glyph_bounding_box(after).unwrap();
            for (a, b) in expected["bounds"]
                .as_array()
                .unwrap()
                .iter()
                .zip([b.x_min, b.y_min, b.x_max, b.y_max])
            {
                let a = a.as_f64().unwrap();
                assert!(
                    (a - f64::from(b)).abs() <= 2.,
                    "{weight}/{size} {ch} outline: {a} != {b}"
                );
            }
            boxes.push(b);
        }
        outlines.push(boxes);
    }
    assert_ne!(outlines[0], outlines[1]);
    assert_ne!(outlines[0], outlines[2]);
}

#[test]
fn embedding_never_makes_the_presentation_read_only() {
    for flags in [0, 8, 0x100, 0x108] {
        assert!(editable(flags));
    }
    for flags in [2, 4, 0x104, 0x200, 0x208] {
        assert!(!editable(flags));
    }
}

#[test]
fn a_collection_face_is_extracted_as_a_complete_editable_font() {
    use typst::{foundations::Bytes, layout::Abs, text::Font};
    let (font, _) = typst_kit::fonts::embedded()
        .find(|(font, _)| font.data().as_slice().starts_with(&[0, 1, 0, 0]))
        .unwrap();
    let original = font.data().as_slice();
    let mut collection = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10".to_vec();
    collection.extend_from_slice(original);
    for i in 0..be16(original, 4).unwrap() as usize {
        let at = 16 + 12 + i * 16 + 8;
        let offset = u32::from_be_bytes(collection[at..at + 4].try_into().unwrap()) + 16;
        collection[at..at + 4].copy_from_slice(&offset.to_be_bytes());
    }
    let face = Font::new(Bytes::new(collection), 0).unwrap();
    let instance = face
        .clone()
        .instantiate(face.info().variant, Abs::pt(12.), &Default::default());
    let embedded = eot(&instance).unwrap();
    let len = u32::from_le_bytes(embedded[4..8].try_into().unwrap()) as usize;
    let data = &embedded[embedded.len() - len..];
    assert!(!data.starts_with(b"ttcf"));
    let restored = Font::new(Bytes::new(data.to_vec()), 0).unwrap();
    let restored_instance =
        restored
            .clone()
            .instantiate(restored.info().variant, Abs::pt(12.), &Default::default());
    let original_instance =
        font.clone()
            .instantiate(font.info().variant, Abs::pt(12.), &Default::default());
    assert_eq!(
        restored_instance.ttf().number_of_glyphs(),
        original_instance.ttf().number_of_glyphs()
    );
    assert_eq!(
        restored_instance.ttf().glyph_index('A'),
        original_instance.ttf().glyph_index('A')
    );
    assert_eq!(restored.info().family, font.info().family);
}
