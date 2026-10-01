use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn compile(source: &str, dpi: Option<u32>) -> Presentation {
    let p = compile_unchecked(source, dpi);
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}

fn compile_unchecked(source: &str, dpi: Option<u32>) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    image::RgbImage::from_pixel(600, 400, image::Rgb([120, 80, 220]))
        .save(dir.path().join("image.png"))
        .unwrap();
    let input = dir.path().join("clip.typ");
    fs::write(
        &input,
        format!("#set page(width:600pt,height:400pt,margin:30pt)\n{source}"),
    )
    .unwrap();
    let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    lower::convert_with_options(
        &doc,
        &lower::Options {
            image_dpi: dpi,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn clipped_pictures_keep_the_original_image_and_an_editable_crop() {
    let p = compile(
        "#box(width:80pt,height:50pt,clip:true)[#place(dx:-20pt,dy:-10pt)[#image(\"image.png\",width:150pt)]]",
        None,
    );
    let pictures: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| {
            if let Element::Picture { clip, bytes, .. } = e {
                Some((clip, bytes))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(pictures.len(), 1);
    assert!(pictures[0].0.is_some());
    assert_eq!(image::load_from_memory(pictures[0].1).unwrap().width(), 600);
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let d = roxmltree::Document::parse(&xml).unwrap();
    let pic = d
        .descendants()
        .find(|n| n.tag_name().name() == "pic")
        .unwrap();
    assert!(pic.descendants().any(|n| n.tag_name().name() == "custGeom"));
    let crop = pic
        .descendants()
        .find(|n| n.tag_name().name() == "srcRect")
        .unwrap();
    assert_eq!(crop.attribute("l"), Some("13333"));
    assert_eq!(crop.attribute("t"), Some("35000"));
}

#[test]
fn a_clip_that_contains_all_text_does_not_prevent_native_editing() {
    let p = compile(
        "#box(width:220pt,height:70pt,clip:true)[#pad(10pt)[A native paragraph]]",
        None,
    );
    assert!(p.slides[0].elements.iter().flat_map(Element::walk).any(|e|matches!(e,Element::Text(t) if t.paragraphs.iter().flat_map(|p|&p.runs).any(|r|r.text.contains("native")))));
    pptx::write(&p).unwrap();
}

#[test]
fn an_image_only_box_does_not_add_an_empty_text_box() {
    let p = compile("#box[#image(\"image.png\",width:90pt)]", None);
    assert!(
        !p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .any(|e| matches!(e, Element::Text(_)))
    );
    assert!(
        p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .any(|e| matches!(e, Element::Picture { .. }))
    );
}

#[test]
fn vertical_text_clipping_keeps_full_paragraphs_and_native_overflow() {
    let p = compile(
        "#set text(size:24pt)\n#box(width:400pt,height:20pt,clip:true)[#place(dy:-5pt)[Editable complete paragraph]]",
        None,
    );
    let text = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Text(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        text.paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .map(|r| r.text.as_str())
            .collect::<String>(),
        "Editable complete paragraph"
    );
    assert!(text.clip.is_some());
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let bp = doc
        .descendants()
        .find(|n| n.tag_name().name() == "bodyPr")
        .unwrap();
    assert_eq!(bp.attribute("horzOverflow"), Some("clip"));
    assert_eq!(bp.attribute("vertOverflow"), Some("clip"));
    assert!(bp.attribute("tIns").unwrap().parse::<i64>().unwrap() < 0);
}

#[test]
fn a_clip_cutting_through_a_text_line_reports_the_office_limit() {
    for source in [
        "#set text(size:24pt)\n#box(width:130pt,height:40pt,clip:true)[#place(dx:-20pt)[Editable complete paragraph]]",
        "#rect(width:100pt,height:100pt,stroke:none,fill:tiling(size:(30pt,30pt))[*Text*])",
    ] {
        let p = compile_unchecked(source, None);
        assert!(
            p.diagnostics.iter().any(|d| d
                .message
                .contains("partial clipping along an editable text line")),
            "{:?}",
            p.diagnostics
        );
    }
}

#[test]
fn clipped_vector_strokes_are_native_paths_within_the_clip() {
    let p = compile(
        "#box(width:80pt,height:50pt,clip:true)[#place(dx:-20pt,dy:-10pt)[#circle(radius:60pt,fill:blue,stroke:4pt+red)]]",
        None,
    );
    let shapes: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| {
            if let Element::Shape(s) = e {
                Some(s)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(shapes.len(), 2);
    for s in shapes {
        assert!(s.bounds.x >= 29.99 && s.bounds.y >= 29.99);
        assert!(s.bounds.right() <= 110.01 && s.bounds.bottom() <= 80.01);
    }
    pptx::write(&p).unwrap();
}

#[test]
fn rotated_scaled_pictures_use_their_placed_size_for_dpi() {
    let p = compile(
        "#scale(200%)[#rotate(25deg)[#image(\"image.png\",width:72pt)]]",
        Some(100),
    );
    let bytes = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Picture { bytes, .. } = e {
                Some(bytes)
            } else {
                None
            }
        })
        .unwrap();
    let decoded = image::load_from_memory(bytes).unwrap();
    assert_eq!(decoded.width(), 200);
    assert_eq!(decoded.height(), 133);
}
