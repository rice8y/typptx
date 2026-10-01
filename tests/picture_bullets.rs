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
