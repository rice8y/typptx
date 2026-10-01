//! Importing an image is an explicit boundary for PowerPoint object editing.
use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn convert(svg: &str, body: &str) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("asset.svg"), svg).unwrap();
    let source = dir.path().join("main.typ");
    fs::write(&source, format!("#set page(width:400pt,height:300pt,margin:0pt)\n#set text(font:\"Libertinus Serif\")\n{body}")).unwrap();
    let (doc, warnings) = CompilerWorld::new(&source, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><g opacity=".5"><rect width="80" height="50" fill="red"/><circle cx="50" cy="40" r="20" fill="blue"/></g><text x="90" y="40" font-family="Libertinus Serif" font-size="16">Static</text></svg>"##;

fn picture(p: &Presentation) -> (&str, &[u8]) {
    let objects: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter(|e| !matches!(e, Element::Group(_)))
        .collect();
    assert_eq!(objects.len(), 1);
    assert!(p.fonts.is_empty());
    let Element::Picture {
        svg: Some(svg),
        bytes,
        ..
    } = objects[0]
    else {
        panic!("expected one picture")
    };
    (svg, bytes)
}

#[test]
fn imported_svg_text_and_shapes_share_one_picture_and_no_office_fonts() {
    let p = convert(SVG, "#image(\"asset.svg\",width:200pt)");
    let (svg, bytes) = picture(&p);
    assert!(!svg.contains("<text"));
    assert!(svg.contains("<path"));
    let png = image::load_from_memory(bytes).unwrap().to_rgba8();
    assert_eq!(png.dimensions(), (400, 200));
    assert_eq!(png.get_pixel(20, 20).0, [255, 0, 0, 128]);
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        doc.descendants()
            .filter(|n| n.tag_name().name() == "pic")
            .count(),
        1
    );
    assert!(
        !doc.descendants()
            .any(|n| matches!(n.tag_name().name(), "txBody" | "sp"))
    );
    assert!(doc.descendants().any(|n| n.tag_name().name() == "svgBlip"));
}

#[test]
fn svg_effects_and_partially_clipped_text_do_not_require_native_editing() {
    for content in [
        r##"<defs><filter id="f"><feColorMatrix type="saturate" values="0.3"/></filter></defs><rect width="100" height="50" fill="red" filter="url(#f)"/>"##,
        r#"<text x="-10" y="25" font-family="Libertinus Serif" font-size="20">Clipped text</text>"#,
        r#"<text x="30" y="-10" font-family="Libertinus Serif" font-size="20" writing-mode="tb">Vertical text</text>"#,
    ] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">{content}</svg>"#
        );
        let p = convert(&svg, "#image(\"asset.svg\",width:100pt)");
        picture(&p);
        pptx::write(&p).unwrap();
    }
}

#[test]
fn svg_rotation_and_clipping_apply_to_the_complete_picture() {
    let p = convert(
        SVG,
        "#rotate(25deg,reflow:true)[#box(width:80pt,height:60pt,clip:true)[#image(\"asset.svg\",width:200pt)]]",
    );
    picture(&p);
    assert!(
        p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .any(|e| matches!(e, Element::Picture { clip: Some(_), .. }))
    );
    pptx::write(&p).unwrap();
}

#[test]
fn images_inside_typst_tiling_remain_complete_pictures() {
    let p = convert(
        SVG,
        "#rect(width:80pt,height:40pt,stroke:none,fill:tiling(size:(40pt,20pt))[#image(\"asset.svg\",width:40pt,height:20pt)])",
    );
    let objects: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter(|e| !matches!(e, Element::Group(_)))
        .collect();
    assert_eq!(objects.len(), 4);
    assert!(objects.iter().all(|e| matches!(e, Element::Picture { .. })));
    pptx::write(&p).unwrap();
}

#[test]
fn cetz_and_typst_primitives_remain_editable_beside_an_imported_image() {
    let p = convert(
        SVG,
        r##"
#import "@preview/cetz:0.5.2": canvas, draw
#rect(width:30pt,height:20pt,fill:red)
#canvas({
    import draw: *
    circle((0,0), radius:0.5, fill:blue)
    line((0,0), (1,1), stroke:2pt)
})
#image("asset.svg",width:200pt)
"##,
    );
    let objects: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .collect();
    assert_eq!(
        objects
            .iter()
            .filter(|e| matches!(e, Element::Picture { .. }))
            .count(),
        1
    );
    assert!(
        objects
            .iter()
            .filter(|e| matches!(e, Element::Shape(_)))
            .count()
            >= 3
    );
    assert!(!objects.iter().any(|e| matches!(e, Element::Drawing { .. })));
    pptx::write(&p).unwrap();
}
