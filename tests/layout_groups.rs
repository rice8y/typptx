//! Alignment and side-by-side content must retain their semantic text owners.
use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{
    capture::{Capture, Kind},
    ir::*,
    lower, pptx,
    world::CompilerWorld,
};
use typst::layout::FrameItem;

fn compile(body: &str) -> (Presentation, Capture) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layout.typ");
    fs::write(&path, format!("#set page(width:600pt,height:400pt,margin:30pt)\n#set text(font:\"Libertinus Serif\",size:18pt)\n{body}")).unwrap();
    let doc = CompilerWorld::new(&path, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0;
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert!(
        p.slides
            .iter()
            .flat_map(|s| &s.elements)
            .flat_map(Element::walk)
            .all(|e| !matches!(e, Element::Picture { .. } | Element::Drawing { .. }))
    );
    (p, Capture::new(&doc))
}
fn texts(p: &Presentation) -> Vec<&TextBlock> {
    p.slides
        .iter()
        .flat_map(|s| &s.elements)
        .flat_map(Element::walk)
        .filter_map(|e| {
            if let Element::Text(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .collect()
}
fn text(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.as_str()).collect()
}
fn xml(p: &Presentation) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[test]
fn centered_list_keeps_body_wrapping_width_and_native_marker_anchors() {
    for dir in ["ltr", "rtl"] {
        let (p, c) = compile(&format!(
            "#set text(dir:{dir})\n#align(center)[\n- First\n- Second\n]"
        ));
        let blocks = texts(&p);
        assert_eq!(blocks.len(), 4);
        let markers: Vec<_> = blocks.iter().filter(|t| t.role == "list_marker").collect();
        assert_eq!(markers.len(), 2);
        for marker in markers {
            assert!(marker.paragraphs[0].bullet.is_some());
            let expected_x = if dir == "rtl" { 563.682 } else { 30. };
            assert!((marker.bounds.x - expected_x).abs() < 0.01);
        }
        for block in blocks.into_iter().filter(|t| t.role == "list") {
            let paragraph = &block.paragraphs[0];
            assert_eq!(paragraph.alignment, "ctr");
            let t = text(paragraph);
            let leaf = c.pages[0]
                .iter()
                .find(|l| matches!(&l.item, FrameItem::Text(x) if x.text.as_str()==t))
                .unwrap();
            let body = c.nearest(leaf, Kind::ItemBody).unwrap();
            let expected = c.nodes[body].pages[&0].context;
            assert!((block.bounds.x + paragraph.margin_left - expected.x).abs() < 0.01);
            assert!(
                (block.bounds.right() - paragraph.margin_right - expected.right()).abs() < 0.01
            );
            assert!(paragraph.bullet.is_none());
            assert!(block.wrap);
        }
        assert!(xml(&p).contains("algn=\"ctr\""));
    }
}

#[test]
fn centered_lists_inside_native_cells_use_the_cell_body_width() {
    let (p, _) = compile("#table(columns:300pt,inset:8pt,[#align(center)[\n- First\n- Second\n]])");
    let table = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Table(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert!(table.cells[0].paragraphs.is_empty());
    let blocks: Vec<_> = texts(&p).into_iter().filter(|t| t.role == "list").collect();
    assert_eq!(blocks.len(), 2);
    for block in blocks {
        let p = &block.paragraphs[0];
        assert_eq!(p.alignment, "ctr");
        assert!((block.bounds.x + p.margin_left - 53.318).abs() < 0.01);
        assert!((block.bounds.right() - p.margin_right - 322.).abs() < 0.01);
    }
    assert!(xml(&p).contains("<a:tbl>"));
}

#[test]
fn centered_multiline_items_keep_one_body_paragraph_and_independent_number() {
    let (p, _) = compile(
        "#block(width:190pt)[#align(center)[\n+ Short \\\n  A longer centered line wraps within the original body width.\n]]",
    );
    let blocks = texts(&p);
    assert_eq!(blocks.len(), 2);
    let marker = blocks.iter().find(|b| b.role == "list_marker").unwrap();
    assert!(matches!(
        marker.paragraphs[0].bullet,
        Some(Bullet::Number { start: Some(1), .. })
    ));
    let body = blocks.iter().find(|b| b.role == "list").unwrap();
    assert_eq!(body.paragraphs.len(), 1);
    assert_eq!(body.paragraphs[0].alignment, "ctr");
    assert!(text(&body.paragraphs[0]).starts_with("Short\n"));
    assert!(
        (body.bounds.width
            - body.paragraphs[0].margin_left
            - body.paragraphs[0].margin_right
            - 168.67)
            .abs()
            < 0.01
    );
    assert!((marker.bounds.x - 30.).abs() < 0.01);
    xml(&p);
}

#[test]
fn columns_in_table_lists_do_not_collapse_into_native_cell_paragraphs() {
    let (p, _) = compile(
        "#table(columns:500pt,[\n- Before\n\n  #columns(2)[Left #colbreak() Right]\n- After\n])",
    );
    let blocks = texts(&p);
    assert_eq!(blocks.len(), 4);
    assert_eq!(
        blocks
            .iter()
            .flat_map(|b| &b.paragraphs)
            .map(text)
            .collect::<Vec<_>>(),
        ["Before", "Left", "Right", "After"]
    );
    assert!((blocks[1].bounds.y - blocks[2].bounds.y).abs() < 0.01);
    assert!(xml(&p).contains("<a:tbl>"));
}

#[test]
fn grid_and_columns_in_list_keep_source_baselines_and_bullet_ownership() {
    for layout in [
        "#grid(columns:(120pt,120pt),[Left],[Right])",
        "#columns(2)[Left #colbreak() Right]",
    ] {
        let (p, c) = compile(&format!("+ Before\n\n  {layout}\n+ After"));
        let blocks = texts(&p);
        assert_eq!(blocks.len(), 4, "{layout}");
        assert_eq!(
            blocks
                .iter()
                .flat_map(|t| &t.paragraphs)
                .map(text)
                .collect::<Vec<_>>(),
            ["Before", "Left", "Right", "After"]
        );
        assert_eq!(
            blocks
                .iter()
                .filter(|t| t.paragraphs[0].bullet.is_some())
                .count(),
            2
        );
        let left = blocks
            .iter()
            .find(|t| text(&t.paragraphs[0]) == "Left")
            .unwrap();
        let right = blocks
            .iter()
            .find(|t| text(&t.paragraphs[0]) == "Right")
            .unwrap();
        assert!((left.bounds.y - right.bounds.y).abs() < 0.01, "{layout}");
        for block in blocks {
            let t = text(&block.paragraphs[0]);
            let leaf = c.pages[0]
                .iter()
                .find(|l| matches!(&l.item, FrameItem::Text(x) if x.text.trim()==t))
                .unwrap();
            assert!(
                (block.bounds.x + block.paragraphs[0].margin_left - leaf.position.0).abs() < 0.01,
                "{t}"
            );
        }
        let x = xml(&p);
        assert!(x.contains("startAt=\"2\""));
    }
}

#[test]
fn inline_shapes_and_highlights_keep_one_complete_paragraph() {
    for body in [
        "Before #box[#rect(width:12pt,height:12pt,fill:red)] after.",
        "Before #highlight(fill:yellow)[highlighted words] after.",
        "#block(width:150pt)[Before #highlight(fill:yellow)[highlighted words that wrap over lines] after.]",
    ] {
        let (p, _) = compile(body);
        let blocks = texts(&p);
        assert_eq!(blocks.len(), 1, "{body}");
        assert_eq!(blocks[0].role, "paragraph");
        assert_eq!(blocks[0].paragraphs.len(), 1);
        let par = &blocks[0].paragraphs[0];
        if body.contains("rect") {
            assert_eq!(text(par), "Before \t after.");
            assert_eq!(par.tab_stops.len(), 1);
        } else {
            assert!(text(par).starts_with("Before highlighted words"));
            assert!(text(par).ends_with(" after."));
            assert!(par.tab_stops.is_empty());
        }
        let x = xml(&p);
        assert_eq!(x.matches("<p:txBody>").count(), 1);
        assert!(x.contains("<p:grpSp>"));
    }
}

#[test]
fn transformed_paragraph_with_inline_shape_stays_native_and_complete() {
    let (p, _) =
        compile("#rotate(-10deg)[Before #box[#rect(width:12pt,height:12pt,fill:red)] after.]");
    assert_eq!(texts(&p).len(), 1);
    assert_eq!(text(&texts(&p)[0].paragraphs[0]), "Before \t after.");
    assert!(xml(&p).contains("rot="));
}

#[test]
fn leading_inline_shape_reserves_its_advance_before_a_wrapped_paragraph() {
    let (p, _) = compile("#block(width:170pt)[#box[#rect(width:12pt,height:12pt,fill:red)]]");
    assert!(texts(&p).is_empty());
    let (p, _) =
        compile("#block(width:170pt)[#box[#rect(width:12pt,height:12pt,fill:red)]] After.");
    assert_eq!(texts(&p).len(), 1);
    let (p, _) = compile(
        "#block(width:170pt)[#box[#rect(width:12pt,height:12pt,fill:red)] Leading graphic before a long sentence that wraps over several lines.]",
    );
    let blocks = texts(&p);
    assert_eq!(blocks.len(), 1);
    let body = blocks[0];
    assert_eq!(body.paragraphs.len(), 1);
    assert!(text(&body.paragraphs[0]).starts_with("\t Leading graphic"));
    assert_eq!(body.paragraphs[0].tab_stops, [12.]);
    assert!((body.bounds.x - 30.).abs() < 0.01);
    assert!((body.bounds.width - 170.).abs() < 0.01, "{:?}", body.bounds);
    assert!(body.wrap);
    xml(&p);
}
