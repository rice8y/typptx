//! Mixed list bodies retain editable paragraphs, shapes, and native tables.
use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn compile(body: &str) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lists.typ");
    fs::write(&path, format!("#set page(width:600pt,height:400pt,margin:30pt)\n#set text(font:\"Libertinus Serif\",size:18pt)\n{body}")).unwrap();
    let doc = CompilerWorld::new(&path, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0;
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert!(
        elements(&p)
            .iter()
            .all(|e| !matches!(e, Element::Drawing { .. } | Element::Picture { .. }))
    );
    p
}

fn elements(p: &Presentation) -> Vec<&Element> {
    p.slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
        .collect()
}

fn texts(p: &Presentation) -> Vec<&TextBlock> {
    elements(p)
        .into_iter()
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn text(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.as_str()).collect()
}

fn slide_xml(p: &Presentation) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(!xml.contains("<p:pic>"));
    xml
}

#[test]
fn empty_and_hidden_markers_keep_body_text_indent_and_blank_item_spacing() {
    for (marker, indent) in [("[]", 9.), ("hide[•]", 15.318)] {
        for direction in ["ltr", "rtl"] {
            let p = compile(&format!(
                "#set text(dir:{direction})\n#set list(marker:{marker})\n- First\n-\n- Second"
            ));
            let blocks = texts(&p);
            assert_eq!(blocks.len(), 1);
            let ps = &blocks[0].paragraphs;
            assert_eq!(ps.iter().map(text).collect::<Vec<_>>(), ["First", "Second"]);
            assert!(ps.iter().all(|p| p.bullet.is_none()));
            for p in ps {
                let margin = if direction == "rtl" {
                    p.margin_right
                } else {
                    p.margin_left
                };
                assert!(
                    (margin - indent).abs() < 0.03,
                    "{marker} {direction}: {margin}"
                );
            }
            let gap = ps[0].space_after + ps[1].line_spacing;
            let expected = if marker == "[]" { 35.244 } else { 47.088 };
            assert!((gap - expected).abs() < 0.03, "{gap}");
            let xml = slide_xml(&p);
            assert!(!xml.contains("<a:buChar") && !xml.contains("<a:buBlip"));
        }
    }
}

#[test]
fn inline_graphics_keep_native_bullets_and_the_gap_between_text_runs() {
    let p = compile("- Before #box[#rect(width:12pt,height:12pt,fill:red)] after\n- Second");
    let blocks = texts(&p);
    assert_eq!(blocks.len(), 2);
    assert_eq!(text(&blocks[0].paragraphs[0]), "Before \t after");
    assert_eq!(text(&blocks[1].paragraphs[0]), "Second");
    assert!(blocks.iter().all(|t| t.paragraphs[0].bullet.is_some()));
    let shape = elements(&p)
        .into_iter()
        .find_map(|e| match e {
            Element::Shape(s) => Some(s),
            _ => None,
        })
        .unwrap();
    let par = &blocks[0].paragraphs[0];
    assert_eq!(par.tab_stops.len(), 1);
    let stop = blocks[0].bounds.x + par.margin_left + par.tab_stops[0];
    assert!((stop - shape.bounds.right()).abs() < 0.01);
    assert!(slide_xml(&p).contains("<p:grpSp>"));
}

#[test]
fn backgrounds_and_nested_tables_keep_numbering_and_native_cell_lists() {
    let p = compile(
        "+ First\n\n  #block(inset:8pt,fill:red)[Callout]\n+ Second\n\n  #table(columns:2,[A],[\n- Inner one\n- Inner two\n])\n+ Last",
    );
    let blocks = texts(&p);
    assert_eq!(
        blocks
            .iter()
            .flat_map(|t| &t.paragraphs)
            .map(text)
            .collect::<Vec<_>>(),
        ["First", "Callout", "Second", "Last"]
    );
    let numbers: Vec<_> = blocks
        .iter()
        .flat_map(|t| &t.paragraphs)
        .filter_map(|p| match p.bullet {
            Some(Bullet::Number { start, .. }) => start,
            _ => None,
        })
        .collect();
    assert_eq!(numbers, [1, 2, 3]);
    let tables: Vec<_> = elements(&p)
        .into_iter()
        .filter_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 1);
    assert_eq!(
        tables[0].cells[1]
            .paragraphs
            .iter()
            .map(text)
            .collect::<Vec<_>>(),
        ["Inner one", "Inner two"]
    );
    assert!(
        tables[0].cells[1]
            .paragraphs
            .iter()
            .all(|p| p.level == 0 && p.bullet.is_some())
    );
    let xml = slide_xml(&p);
    assert_eq!(xml.matches("<a:tbl>").count(), 1);
}

#[test]
fn items_with_only_native_objects_keep_their_markers() {
    for body in [
        "#rect(width:12pt,height:12pt,fill:red)",
        "#table(columns:2,[A],[B])",
    ] {
        let p = compile(&format!("- {body}\n- Second"));
        let blocks = texts(&p);
        assert_eq!(blocks.len(), 2, "{body}");
        assert!(
            text(&blocks[0].paragraphs[0])
                .chars()
                .all(|c| c == '\u{200b}')
        );
        assert!(blocks.iter().all(|t| t.paragraphs[0].bullet.is_some()));
        assert_eq!(text(&blocks[1].paragraphs[0]), "Second");
        slide_xml(&p);
    }
}

#[test]
fn rich_lists_in_split_cells_keep_each_body_once() {
    let p = compile(
        "#set page(height:180pt,margin:20pt)\n#table(columns:400pt,[#list(..range(8).map(i=>[Item-#i #box[#rect(width:12pt,height:12pt,fill:red)]]))])",
    );
    assert!(p.slides.len() > 1);
    let actual: Vec<_> = texts(&p)
        .iter()
        .flat_map(|t| &t.paragraphs)
        .map(|p| text(p).trim().to_string())
        .collect();
    assert_eq!(
        actual,
        (0..8).map(|i| format!("Item-{i}")).collect::<Vec<_>>()
    );
    assert_eq!(
        elements(&p)
            .iter()
            .filter(|e| matches!(e, Element::Shape(_)))
            .count(),
        8
    );
    assert_eq!(
        texts(&p)
            .iter()
            .flat_map(|t| &t.paragraphs)
            .filter(|p| p.bullet.is_some())
            .count(),
        8
    );
    pptx::write(&p).unwrap();
}

#[test]
fn invisible_markers_in_split_cells_keep_native_paragraphs() {
    for marker in ["[]", "hide[•]"] {
        let p = compile(&format!(
            "#set page(height:135pt,margin:10pt)\n#set list(marker:{marker})\n#table(columns:300pt,[#list(..range(8).map(i=>[Item-#i]))])"
        ));
        assert!(p.slides.len() > 1);
        let ps: Vec<_> = elements(&p)
            .into_iter()
            .filter_map(|e| match e {
                Element::Table(t) => Some(t),
                _ => None,
            })
            .flat_map(|t| &t.cells)
            .flat_map(|c| &c.paragraphs)
            .collect();
        assert_eq!(
            ps.iter().map(|p| text(p)).collect::<Vec<_>>(),
            (0..8).map(|i| format!("Item-{i}")).collect::<Vec<_>>()
        );
        assert!(ps.iter().all(|p| p.bullet.is_none()));
        assert!(
            ps.iter()
                .all(|p| (p.margin_left - ps[0].margin_left).abs() < 0.03)
        );
        pptx::write(&p).unwrap();
    }
}

#[test]
fn a_picture_marker_and_a_table_only_body_both_survive() {
    let p = compile("#set list(marker:circle(radius:3pt,fill:blue))\n- #table(columns:2,[A],[B])");
    let blocks = texts(&p);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(
        blocks[0].paragraphs[0].bullet,
        Some(Bullet::Picture { .. })
    ));
    assert_eq!(
        elements(&p)
            .iter()
            .filter(|e| matches!(e, Element::Table(_)))
            .count(),
        1
    );
    slide_xml(&p);
}

#[test]
fn a_table_marker_remains_a_picture_bullet_in_a_graphical_list() {
    let p = compile(
        "#set list(marker:table(columns:1,inset:1pt,[x]))\n- Before #box[#rect(width:12pt,height:12pt,fill:red)] after",
    );
    assert!(matches!(
        texts(&p)[0].paragraphs[0].bullet,
        Some(Bullet::Picture { .. })
    ));
    assert!(!elements(&p).iter().any(|e| matches!(e, Element::Table(_))));
    slide_xml(&p);
}

#[test]
fn rotated_graphical_lists_keep_native_shapes_and_text() {
    let p = compile(
        "#rotate(-10deg)[\n- Before #box[#rect(width:12pt,height:12pt,fill:red)] after\n- Second\n]",
    );
    assert_eq!(texts(&p).len(), 2);
    assert!(elements(&p).iter().any(|e| matches!(e, Element::Shape(_))));
    assert!(slide_xml(&p).contains("rot="));
}
