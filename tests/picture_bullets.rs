//! Picture markers remain native list formatting, including nested/table lists.
use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn compile(body: &str) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("icon.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#).unwrap();
    image::RgbImage::from_pixel(20, 10, image::Rgb([0, 60, 180]))
        .save(dir.path().join("icon.png"))
        .unwrap();
    let input = dir.path().join("main.typ");
    fs::write(&input, format!("#set page(width:400pt,height:240pt,margin:20pt)\n#set text(font:\"Libertinus Serif\",size:20pt)\n{body}")).unwrap();
    let (doc, warnings) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}
fn paragraphs(p: &Presentation) -> Vec<&Paragraph> {
    p.slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
        .flat_map(|e| match e {
            Element::Text(t) => t.paragraphs.iter().collect::<Vec<_>>(),
            Element::Table(t) => t.cells.iter().flat_map(|c| &c.paragraphs).collect(),
            _ => Vec::new(),
        })
        .collect()
}
fn text(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.as_str()).collect()
}
fn package(p: &Presentation, expected: usize) {
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    let mut count = 0;
    for page in 1..=p.slides.len() {
        let mut xml = String::new();
        z.by_name(&format!("ppt/slides/slide{page}.xml"))
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        let d = roxmltree::Document::parse(&xml).unwrap();
        assert!(!d.descendants().any(|n| n.tag_name().name() == "pic"));
        count += d
            .descendants()
            .filter(|n| n.tag_name().name() == "pPr")
            .filter(|p| p.descendants().any(|n| n.tag_name().name() == "buBlip"))
            .count();
    }
    assert_eq!(count, expected);
}

#[test]
fn image_bullets_keep_one_text_box_and_reflowable_body_paragraphs() {
    for format in ["svg", "png"] {
        let p = compile(&format!(
            "#set list(marker:image(\"icon.{format}\",width:16pt))\n- First\n- Second"
        ));
        let ps = paragraphs(&p);
        assert_eq!(
            ps.iter().map(|p| text(p)).collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert!(ps.iter().all(
            |p| matches!(p.bullet, Some(Bullet::Picture { size, .. }) if (size - 8.).abs()<1e-6)
        ));
        let elements: Vec<_> = p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .collect();
        assert_eq!(
            elements
                .iter()
                .filter(|e| matches!(e, Element::Text(t) if t.role == "list" && t.wrap))
                .count(),
            1
        );
        package(&p, 2);
    }
}

#[test]
fn empty_nested_and_mixed_marker_items_keep_their_levels() {
    let p = compile(
        r#"
#set list(marker: (image("icon.svg",width:16pt), "–"))
- Parent
  - Nested
- 
- Last
"#,
    );
    let ps = paragraphs(&p);
    assert_eq!(ps.len(), 4);
    assert!(matches!(ps[0].bullet, Some(Bullet::Picture { .. })));
    assert_eq!(ps[0].level, 0);
    assert!(matches!(ps[1].bullet, Some(Bullet::Character { .. })));
    assert_eq!(ps[1].level, 1);
    assert!(matches!(ps[2].bullet, Some(Bullet::Picture { .. })));
    assert!(
        text(ps[2])
            .chars()
            .all(|c| c == '\u{200b}' || c.is_whitespace())
    );
    package(&p, 3);
}

#[test]
fn picture_bullets_inside_tables_preserve_native_table_text() {
    let p = compile(
        r#"
#set list(marker:image("icon.svg",width:16pt))
#table(columns:1,inset:10pt,[
- InCellOne
- InCellTwo
])
"#,
    );
    assert!(
        p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .any(|e| matches!(e, Element::Table(_)))
    );
    let ps = paragraphs(&p);
    assert_eq!(
        ps.iter()
            .filter(|p| matches!(p.bullet, Some(Bullet::Picture { .. })))
            .count(),
        2
    );
    assert!(ps.iter().any(|p| text(p) == "InCellOne"));
    package(&p, 2);
}

#[test]
fn continued_items_do_not_repeat_the_picture_marker() {
    let p = compile(
        r#"
#set page(height:110pt)
#set list(marker:image("icon.svg",width:16pt))
- #for i in range(12) [line-#i #linebreak()]
- Last
"#,
    );
    assert!(p.slides.len() > 1);
    let ps = paragraphs(&p);
    assert_eq!(
        ps.iter()
            .filter(|p| matches!(p.bullet, Some(Bullet::Picture { .. })))
            .count(),
        2
    );
    package(&p, 2);
}

#[test]
fn rotated_picture_lists_keep_editable_text_and_native_bullets() {
    let p = compile(
        r#"
#set list(marker:image("icon.svg",width:16pt))
#rotate(20deg)[
- Rotated one
- Rotated two
]
"#,
    );
    assert!(matches!(p.slides[0].elements[0], Element::Group(_)));
    assert_eq!(
        paragraphs(&p)
            .iter()
            .filter(|p| matches!(p.bullet, Some(Bullet::Picture { .. })))
            .count(),
        2
    );
    package(&p, 2);
}

#[test]
fn rotated_and_cropped_markers_are_baked_into_one_vector_bullet() {
    for marker in [
        "rotate(25deg,reflow:true)[#image(\"icon.svg\",width:16pt)]",
        "box(width:10pt,height:6pt,clip:true)[#image(\"icon.svg\",width:16pt)]",
        "rotate(25deg,reflow:true)[#box(width:10pt,height:6pt,clip:true)[#image(\"icon.png\",width:16pt)]]",
        "scale(x:-100%)[#image(\"icon.svg\",width:16pt)]",
    ] {
        let p = compile(&format!("#set list(marker:{marker})\n- First\n- Second"));
        assert_eq!(
            paragraphs(&p).iter().map(|p| text(p)).collect::<Vec<_>>(),
            ["First", "Second"]
        );
        for paragraph in paragraphs(&p) {
            let Some(Bullet::Picture {
                bytes,
                svg: Some(svg),
                size,
                ..
            }) = &paragraph.bullet
            else {
                panic!("expected a vector marker")
            };
            assert!(*size > 0.);
            let preview = image::load_from_memory(bytes).unwrap().to_rgba8();
            assert!(preview.pixels().any(|p| p.0[3] > 0));
            assert!(svg.contains("<svg"));
        }
        package(&p, 2);
    }
}

#[test]
fn graphical_and_compound_markers_keep_one_native_list_with_only_body_text() {
    for marker in [
        "rect(width:8pt,height:8pt,fill:red,stroke:none)",
        "[#image(\"icon.svg\",width:12pt)#h(2pt)#text(fill:blue)[+]]",
        "[#image(\"icon.svg\",width:12pt)#image(\"icon.png\",width:12pt)]",
        "circle(radius:8pt,fill:green)[#align(center+horizon)[+]]",
        "rotate(25deg,reflow:true)[#rect(width:8pt,height:8pt,fill:red)#h(2pt)+]",
        "box(width:12pt,height:8pt,clip:true)[#image(\"icon.svg\",width:20pt)+]",
        "[#rect(width:8pt,height:8pt,fill:red)#h(2pt)$x^2$]",
    ] {
        let p = compile(&format!("#set list(marker:{marker})\n- First\n- Second"));
        assert_eq!(
            paragraphs(&p).iter().map(|p| text(p)).collect::<Vec<_>>(),
            ["First", "Second"]
        );
        for paragraph in paragraphs(&p) {
            let Some(Bullet::Picture {
                bytes,
                svg: Some(svg),
                ..
            }) = &paragraph.bullet
            else {
                panic!("expected picture marker for {marker}");
            };
            assert!(
                image::load_from_memory(bytes)
                    .unwrap()
                    .to_rgba8()
                    .pixels()
                    .any(|p| p.0[3] > 0)
            );
            assert!(
                !svg.contains("<text"),
                "marker fonts must travel inside the picture"
            );
        }
        assert_eq!(
            p.slides[0]
                .elements
                .iter()
                .flat_map(Element::walk)
                .filter(|e| matches!(e, Element::Text(t) if t.role == "list" && t.wrap))
                .count(),
            1
        );
        package(&p, 2);
    }
}

#[test]
fn compound_markers_keep_empty_nested_and_table_items_native() {
    let p = compile(
        r#"
#set list(marker:([#image("icon.svg",width:12pt)+],circle(radius:3pt,fill:blue)))
- Parent
  - Nested
-

#pagebreak()
#table(columns:1,[
- Cell one
- Cell two
])
"#,
    );
    let ps = paragraphs(&p);
    assert_eq!(
        ps.iter()
            .filter(|p| matches!(p.bullet, Some(Bullet::Picture { .. })))
            .count(),
        5
    );
    assert_eq!(ps[1].level, 1);
    assert!(
        text(ps[2])
            .chars()
            .all(|c| c == '\u{200b}' || c.is_whitespace())
    );
    assert!(
        p.slides
            .iter()
            .flat_map(|s| &s.elements)
            .flat_map(Element::walk)
            .any(|e| matches!(e, Element::Table(_)))
    );
    package(&p, 5);
}

#[test]
fn transformed_compound_markers_stay_inside_native_table_cells() {
    let p = compile(
        r#"
#set list(marker:rotate(20deg,reflow:true,box(width:24pt)[#image("icon.svg",width:12pt)+]))
#table(columns:1,inset:10pt,[
- Cell one
- Cell two
])
"#,
    );
    let tables: Vec<_> = p
        .slides
        .iter()
        .flat_map(|s| &s.elements)
        .flat_map(Element::walk)
        .filter_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 1);
    let ps = &tables[0].cells[0].paragraphs;
    assert_eq!(
        ps.iter().map(text).collect::<Vec<_>>(),
        ["Cell one", "Cell two"]
    );
    assert!(
        ps.iter()
            .all(|p| matches!(p.bullet, Some(Bullet::Picture { .. })))
    );
    assert!(
        !p.slides
            .iter()
            .flat_map(|s| &s.elements)
            .flat_map(Element::walk)
            .any(|e| matches!(e, Element::Text(_)))
    );
    package(&p, 2);
}

#[test]
fn rich_text_and_math_markers_keep_their_realized_appearance() {
    for marker in [
        "[#text(fill:red)[A]#text(fill:blue)[B]]",
        "[$x^2$]",
        "[$frac(1,2)$]",
        "rotate(25deg,reflow:true)[X]",
        "[*Bold*]",
        "[_Italic_]",
        "[A#super[2]]",
        "text(tracking:2pt)[AB]",
        "text(fill:gradient.linear(red,blue))[AB]",
        "box(width:8pt,clip:true)[AB]",
    ] {
        let p = compile(&format!("#set list(marker:{marker})\n- First\n- Second"));
        assert_eq!(
            paragraphs(&p).iter().map(|p| text(p)).collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert!(
            paragraphs(&p)
                .iter()
                .all(|p| matches!(p.bullet, Some(Bullet::Picture { .. }))),
            "{marker}"
        );
        if marker.contains("fill:blue") {
            let Some(Bullet::Picture { svg: Some(svg), .. }) = &paragraphs(&p)[0].bullet else {
                unreachable!()
            };
            assert!(
                svg.contains("#ff4136") && svg.contains("#0074d9"),
                "both source colors must be retained: {svg}"
            );
        }
        package(&p, 2);
    }
}

#[test]
fn ordinary_styled_numbering_keeps_automatic_numbers() {
    let p = compile("#set text(weight:\"bold\",style:\"italic\")\n+ First\n+ Second");
    assert!(
        paragraphs(&p)
            .iter()
            .all(|p| matches!(p.bullet, Some(Bullet::Number { .. })))
    );
    let p = compile("#set list(marker:[–])\n- First\n- Second");
    assert!(
        paragraphs(&p)
            .iter()
            .all(|p| matches!(p.bullet, Some(Bullet::Character { .. })))
    );
}
