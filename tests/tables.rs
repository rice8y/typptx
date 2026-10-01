use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn compile(source: &str) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("table.typ");
    fs::write(&path, format!("#set page(width:600pt,height:350pt,margin:30pt)\n#set text(font:\"Libertinus Serif\",size:18pt)\n{source}")).unwrap();
    lower::convert(
        &CompilerWorld::new(&path, None, &[], &[])
            .unwrap()
            .compile()
            .unwrap()
            .0,
    )
    .unwrap()
}
fn tables(p: &Presentation) -> Vec<&Table> {
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p.slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
        .filter_map(|e| {
            if let Element::Table(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .collect()
}
fn text(c: &TableCell) -> String {
    c.paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .map(|r| r.text.as_str())
        .collect()
}
fn xml(p: &Presentation) -> String {
    xml_with_pictures(p, 0)
}
fn xml_with_pictures(p: &Presentation, pictures: usize) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    assert_eq!(
        zip.file_names()
            .filter(|n| n.starts_with("ppt/media/"))
            .count(),
        pictures * 2
    );
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 0.03, "{a} != {b}");
}
fn paragraph_texts(xml: &str) -> Vec<String> {
    roxmltree::Document::parse(xml)
        .unwrap()
        .descendants()
        .filter(|n| n.has_tag_name(("http://schemas.openxmlformats.org/drawingml/2006/main", "p")))
        .map(|p| {
            p.descendants()
                .filter(|n| {
                    n.has_tag_name(("http://schemas.openxmlformats.org/drawingml/2006/main", "t"))
                })
                .filter_map(|n| n.text())
                .collect()
        })
        .collect()
}

#[test]
fn gutters_remain_native_tracks_and_spans_include_the_inner_gutter() {
    let p = compile(
        "#table(columns:(90pt,120pt,100pt),rows:(40pt,50pt),column-gutter:10pt,row-gutter:12pt,inset:6pt,fill:yellow,table.cell(colspan:2)[Wide],[C],[D],[E],[F])",
    );
    let t = tables(&p)[0];
    assert_eq!(t.column_widths, [90., 10., 120., 10., 100.]);
    assert_eq!(t.row_heights, [40., 12., 50.]);
    assert_eq!(t.cells[0].column_span, 3);
    assert_eq!(text(&t.cells[0]), "Wide");
    for c in &t.cells {
        if text(c).is_empty() {
            assert!(c.fill.is_none());
            assert_eq!(c.inset, [0.; 4]);
        }
    }
    let s = xml(&p);
    assert_eq!(s.matches("<a:tbl>").count(), 1);
    assert_eq!(s.matches("<a:gridCol ").count(), 5);
    assert!(s.contains("gridSpan=\"3\""));
    assert_eq!(
        paragraph_texts(&s).iter().filter(|t| *t == "Wide").count(),
        1
    );
}

#[test]
fn zero_gutters_do_not_add_empty_office_rows_or_columns() {
    for (args, columns, rows) in [
        ("column-gutter:8pt", 3, 2),
        ("row-gutter:8pt", 2, 3),
        ("gutter:0pt", 2, 2),
    ] {
        let p = compile(&format!(
            "#table(columns:(100pt,100pt),rows:(40pt,40pt),{args},[A],[B],[C],[D])"
        ));
        let t = tables(&p)[0];
        assert_eq!(t.column_widths.len(), columns);
        assert_eq!(t.row_heights.len(), rows);
        xml(&p);
    }
}

#[test]
fn fully_merged_cells_keep_hidden_tracks_and_all_four_borders() {
    let p = compile(
        "#table(columns:(90pt,120pt,100pt),rows:(40pt,50pt),stroke:2pt+red,table.cell(colspan:3,rowspan:2)[Merged])",
    );
    let t = tables(&p)[0];
    assert_eq!(t.column_widths, [90., 120., 100.]);
    assert_eq!(t.row_heights, [40., 50.]);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    let origin = d
        .descendants()
        .find(|n| n.tag_name().name() == "tc")
        .unwrap();
    for tag in ["lnL", "lnR", "lnT", "lnB"] {
        let edge = origin
            .descendants()
            .find(|n| n.tag_name().name() == tag)
            .unwrap();
        assert_eq!(edge.attribute("w"), Some("25400"), "{tag}");
    }
}

#[test]
fn hidden_auto_fractional_and_percentage_tracks_use_resolved_sizing() {
    let p = compile("#table(columns:2,table.cell(colspan:2)[All auto])");
    let t = tables(&p)[0];
    assert_eq!(t.column_widths, [0., t.bounds.width]);
    assert!(t.column_widths[1] > 40.);
    xml(&p);
    for (cols, expected) in [("(1fr,2fr)", [180., 360.]), ("(20%,40%)", [108., 216.])] {
        let p = compile(&format!(
            "#table(columns:{cols},table.cell(colspan:2)[Merged])"
        ));
        let t = tables(&p)[0];
        near(t.column_widths[0], expected[0]);
        near(t.column_widths[1], expected[1]);
        xml(&p);
    }
}

#[test]
fn relative_cell_insets_use_full_span_and_inherited_font_size() {
    let p = compile(
        "#table(columns:(90pt,120pt),rows:(40pt,50pt),inset:10%,table.cell(colspan:2,rowspan:2)[Merged])",
    );
    assert_eq!(tables(&p)[0].cells[0].inset, [9., 21., 9., 21.]);
    let p = compile("#table(columns:200pt,inset:1em,[#text(size:9pt)[Small]])");
    assert_eq!(tables(&p)[0].cells[0].inset, [18.; 4]);
    let p = compile("#table(columns:150pt,rows:60pt,inset:10%,align:center+horizon,[Center])");
    let c = &tables(&p)[0].cells[0];
    assert_eq!(c.vertical_alignment, "ctr");
    assert!(c.text_inset[0] < c.inset[0]);
    assert!(c.text_inset[2] < c.inset[2]);
}

#[test]
fn repeated_headers_and_footers_keep_each_body_row_once() {
    for gutter in ["", "column-gutter:8pt,row-gutter:6pt,"] {
        let p = compile(&format!(
            "#table(columns:(90pt,260pt),inset:8pt,{gutter}table.header([ID],[Description]),..range(22).map(i=>([#i],[Row #i])).flatten(),table.footer([End],[Continued]))"
        ));
        let ts = tables(&p);
        assert!(ts.len() > 1);
        assert_eq!(ts.len(), p.slides.len());
        let mut bodies = Vec::new();
        for t in &ts {
            assert_eq!(t.cells.iter().filter(|c| text(c) == "ID").count(), 1);
            assert_eq!(t.cells.iter().filter(|c| text(c) == "End").count(), 1);
            for c in &t.cells {
                if text(c).starts_with("Row ") {
                    bodies.push(text(c));
                }
            }
            near(
                t.column_widths.iter().sum(),
                if gutter.is_empty() { 350. } else { 358. },
            );
        }
        assert_eq!(
            bodies,
            (0..22).map(|i| format!("Row {i}")).collect::<Vec<_>>()
        );
        xml(&p);
    }
}

#[test]
fn page_decorations_do_not_change_continued_table_tracks() {
    let p = compile(
        r#"
        #set page(header:[Header],footer:context grid(columns:(1fr,1fr,1fr),[Footer],[#counter(page).display()],[End]))
        #table(columns:(90pt,260pt),rows:36pt,inset:8pt,
            table.header([ID],[Description]),
            ..range(16).map(i=>([#i],[Row #i])).flatten())
    "#,
    );
    let ts = tables(&p);
    assert!(ts.len() >= 3);
    let mut bodies = Vec::new();
    for t in ts {
        near(t.bounds.x, 30.);
        near(t.bounds.y, 30.);
        assert_eq!(t.column_widths, [90., 260.]);
        for &height in &t.row_heights {
            near(height, 36.);
        }
        bodies.extend(t.cells.iter().map(text).filter(|s| s.starts_with("Row ")));
    }
    assert_eq!(
        bodies,
        (0..16).map(|i| format!("Row {i}")).collect::<Vec<_>>()
    );
    for slide in &p.slides {
        let content: String = slide
            .elements
            .iter()
            .flat_map(Element::walk)
            .filter_map(|e| {
                if let Element::Text(t) = e {
                    Some(
                        t.paragraphs
                            .iter()
                            .flat_map(|p| &p.runs)
                            .map(|r| r.text.as_str())
                            .collect::<String>(),
                    )
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(content.matches("Header").count(), 1);
        assert_eq!(content.matches("Footer").count(), 1);
    }
    xml(&p);
}

#[test]
fn a_cell_split_across_pages_keeps_each_page_native() {
    let p = compile("#table(columns:300pt,inset:8pt,[#lorem(240)])");
    assert!(p.slides.len() > 1);
    let ts = tables(&p);
    assert_eq!(ts.len(), p.slides.len());
    assert!(
        ts.iter()
            .all(|t| t.cells.len() == 1 && !text(&t.cells[0]).is_empty())
    );
    let actual: String = ts.iter().flat_map(|t| &t.cells).map(text).collect();
    let reference =
        compile("#set page(height:5000pt)\n#table(columns:300pt,inset:8pt,[#lorem(240)])");
    let reference = tables(&reference)
        .iter()
        .flat_map(|t| &t.cells)
        .map(text)
        .collect::<String>();
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    assert_eq!(compact(&actual), compact(&reference));
    xml(&p);
}

#[test]
fn lists_in_split_cells_keep_their_items_bullets_and_columns() {
    for marker in ["[•]", "circle(radius:3pt,fill:blue)", "[$x^2$]"] {
        let p = compile(&format!(
            r#"
#set page(height:135pt,margin:10pt)
#set list(marker:{marker})
#table(columns:(210pt,210pt),inset:8pt,
  [#list(..range(8).map(i=>[Left #i]))],
  [#list(..range(8).map(i=>[Right #i]))],
)
"#
        ));
        assert!(p.slides.len() >= 2);
        let ts = tables(&p);
        assert_eq!(ts.len(), p.slides.len());
        for column in 0..2 {
            let expected: Vec<_> = (0..8)
                .map(|i| format!("{} {i}", if column == 0 { "Left" } else { "Right" }))
                .collect();
            let ps: Vec<_> = ts
                .iter()
                .flat_map(|t| &t.cells)
                .filter(|c| c.column == column)
                .flat_map(|c| &c.paragraphs)
                .collect();
            let actual: Vec<String> = ps
                .iter()
                .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect())
                .collect();
            assert_eq!(actual, expected, "{marker}");
            assert!(ps.iter().all(|p| p.bullet.is_some()), "{marker}");
            for p in &ps {
                assert!(
                    (p.margin_left - ps[0].margin_left).abs() < 0.03,
                    "{marker}: margin {} != {} for {:?}",
                    p.margin_left,
                    ps[0].margin_left,
                    p.runs.iter().map(|r| r.text.as_str()).collect::<String>()
                );
                near(p.indent, ps[0].indent);
            }
        }
        assert!(
            !p.slides
                .iter()
                .flat_map(|s| &s.elements)
                .flat_map(Element::walk)
                .any(|e| matches!(e, Element::Drawing { .. } | Element::Text(_)))
        );
        pptx::write(&p).unwrap();
    }
}

#[test]
fn a_numbered_item_continued_in_a_split_cell_does_not_repeat_its_marker() {
    let p = compile(
        r#"
#set page(height:135pt,margin:10pt)
#table(columns:300pt,inset:8pt,[
  + #for i in range(12) [Line #i #linebreak()]
  + Last
])
"#,
    );
    assert!(p.slides.len() >= 2);
    let ps: Vec<_> = tables(&p)
        .iter()
        .flat_map(|t| &t.cells)
        .flat_map(|c| &c.paragraphs)
        .collect();
    assert_eq!(ps.iter().filter(|p| p.bullet.is_some()).count(), 2);
    let actual = ps
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let expected: Vec<_> = (0..12)
        .map(|i| format!("Line {i}"))
        .chain(["Last".into()])
        .collect();
    assert_eq!(actual.lines().map(str::trim).collect::<Vec<_>>(), expected);
    assert!(matches!(
        ps.first().unwrap().bullet,
        Some(Bullet::Number { start: Some(1), .. })
    ));
    assert!(matches!(
        ps.last().unwrap().bullet,
        Some(Bullet::Number { start: Some(2), .. })
    ));
    xml(&p);
}

#[test]
fn split_cell_text_styles_do_not_leak_between_columns() {
    let p = compile(
        r#"
#set page(height:135pt,margin:10pt)
#table(columns:(210pt,210pt),inset:8pt,
  text(tracking:1pt,fill:red)[#list(..range(8).map(i=>[Left #i]))],
  [#list(..range(8).map(i=>[Right #i]))],
)
"#,
    );
    assert!(p.slides.len() > 1);
    for t in tables(&p) {
        for cell in &t.cells {
            let tracking = if cell.column == 0 { 1. } else { 0. };
            let color = if cell.column == 0 {
                [255, 65, 54, 255]
            } else {
                [0, 0, 0, 255]
            };
            for run in cell.paragraphs.iter().flat_map(|p| &p.runs) {
                assert_eq!(run.style.letter_spacing, tracking);
                assert_eq!(run.style.color, color);
            }
        }
    }
    pptx::write(&p).unwrap();
}

#[test]
fn nested_tables_remain_native_and_grouped_with_their_parent() {
    let p = compile("#table(columns:300pt,[Before\n\n#table(columns:2,[A],[B])\n\nAfter])");
    let ts = tables(&p);
    assert_eq!(ts.len(), 2);
    assert!(text(&ts[0].cells[0]).is_empty());
    assert_eq!(text(&ts[1].cells[0]), "A");
    assert_eq!(text(&ts[1].cells[1]), "B");
    let s = xml(&p);
    assert!(s.contains("<p:grpSp>"));
    assert_eq!(s.matches("<a:tbl>").count(), 2);
    let paragraphs = paragraph_texts(&s);
    assert_eq!(paragraphs.iter().filter(|s| *s == "Before").count(), 1);
    assert_eq!(paragraphs.iter().filter(|s| *s == "After").count(), 1);
}

#[test]
fn layout_grid_cells_do_not_become_columns_of_the_containing_table() {
    for columns in ["(1fr,1fr,1fr)", "1"] {
        let p = compile(&format!(
            r#"#table(columns:(80pt,360pt),inset:8pt,
            [Label], [#grid(columns:{columns},gutter:6pt,[Alpha],[Beta],[Gamma])])"#
        ));
        let ts = tables(&p);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].column_widths, [80., 360.]);
        assert_eq!(ts[0].cells.len(), 2);
        let native: Vec<_> = p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .filter_map(|e| {
                if let Element::Text(t) = e {
                    Some(t)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            native.len(),
            3,
            "grid paragraphs must retain separate layout cells"
        );
        for (t, label) in native.iter().zip(["Alpha", "Beta", "Gamma"]) {
            assert_eq!(
                t.paragraphs
                    .iter()
                    .flat_map(|p| &p.runs)
                    .map(|r| r.text.as_str())
                    .collect::<String>(),
                label
            );
        }
        for pair in native.windows(2) {
            if columns == "1" {
                near(pair[0].bounds.x, pair[1].bounds.x);
                assert!(pair[1].bounds.y > pair[0].bounds.y);
            } else {
                near(pair[0].bounds.y, pair[1].bounds.y);
                near(
                    pair[1].bounds.x - pair[0].bounds.x,
                    (360. - 16. - 12.) / 3. + 6.,
                );
            }
        }
        let s = xml(&p);
        for label in ["Label", "Alpha", "Beta", "Gamma"] {
            assert_eq!(
                paragraph_texts(&s).iter().filter(|t| *t == label).count(),
                1
            );
        }
    }
}

#[test]
fn math_scripts_do_not_trigger_cell_wrapping_and_paragraph_gaps_are_preserved() {
    let p = compile("#table(columns:240pt,[$y = a x^2 + b x + c$],[First\n\nSecond])");
    let t = tables(&p)[0];
    assert!(!t.cells[0].wrap);
    assert_eq!(t.cells[1].paragraphs.len(), 2);
    assert!(t.cells[1].paragraphs[0].space_after > 5.0);
    let s = xml(&p);
    assert!(s.contains("<m:sSup>"));
    assert!(s.contains("<a:spcAft>"));
}

#[test]
fn partial_rules_on_merged_sides_remain_separate_editable_lines() {
    let p = compile(
        "#table(columns:(100pt,100pt),stroke:none,table.hline(start:1,stroke:2pt+red),table.cell(colspan:2)[Merged])",
    );
    let ts = tables(&p);
    assert_eq!(ts.len(), 1);
    assert!(
        ts[0]
            .horizontal_borders
            .iter()
            .flatten()
            .all(Option::is_none)
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
    assert_eq!(shapes.len(), 1);
    near(shapes[0].bounds.x, 129.);
    near(shapes[0].bounds.width, 102.);
    assert!(xml(&p).contains("<p:grpSp>"));
}

#[test]
fn percentage_tracks_inside_a_sized_block_use_its_available_width() {
    let p = compile("#block(width:300pt)[#table(columns:(20%,40%),table.cell(colspan:2)[Merged])]");
    let t = tables(&p)[0];
    near(t.column_widths[0], 60.);
    near(t.column_widths[1], 120.);
    let p = compile("#table(columns:200pt,rows:2,table.cell(rowspan:2)[Merged])");
    let t = tables(&p)[0];
    assert_eq!(t.row_heights.len(), 1);
    assert!(t.row_heights[0] > 18.);
    xml(&p);
}

#[test]
fn gradient_fills_and_cell_graphics_remain_editable() {
    let p = compile(
        "#table(columns:180pt,fill:gradient.linear(red,blue),[A], [#rect(width:30pt,height:20pt,fill:green)])",
    );
    assert_eq!(tables(&p).len(), 1);
    assert!(matches!(
        tables(&p)[0].cells[0].fill,
        Some(Brush::Linear { .. })
    ));
    let s = xml(&p);
    assert!(s.contains("<a:gradFill"));
    assert!(s.contains("<p:grpSp>"));
}

#[test]
fn inline_cell_pictures_preserve_the_surrounding_paragraph() {
    let p = compile(
        r#"#table(columns:240pt,[Before #box[#image(bytes("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><circle cx='10' cy='10' r='8' fill='red'/></svg>"),format:"svg",width:20pt)] after])"#,
    );
    let ts = tables(&p);
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].cells[0].paragraphs.len(), 1);
    assert_eq!(text(&ts[0].cells[0]), "Before \t after");
    let s = xml_with_pictures(&p, 1);
    assert!(s.contains("<p:grpSp>"));
    assert_eq!(s.matches("<p:pic>").count(), 1);
}

#[test]
fn inline_pictures_do_not_change_unrelated_table_text_layout() {
    let picture = r#"#box[#image(bytes("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><circle cx='10' cy='10' r='8' fill='red'/></svg>"),format:"svg",width:20pt)]"#;
    let table = "#table(columns:(auto,75pt),align:(center,right),inset:8pt,[AVATAR],[0.95],[Longer heading],[This text wraps over several lines])";
    for separator in ["\n\n", "\n#pagebreak()\n"] {
        let reference = compile(&format!("#context [Before X after{separator}{table}]"));
        let with_picture = compile(&format!(
            "#context [Before {picture} after{separator}{table}]"
        ));
        let a = tables(&reference)[0];
        let b = tables(&with_picture)[0];
        assert_eq!(a.column_widths, b.column_widths);
        for (a, b) in a.cells.iter().zip(&b.cells) {
            assert_eq!(a.wrap, b.wrap, "{}", text(b));
            let mut actual = b.paragraphs.clone();
            for (expected, actual) in a.paragraphs.iter().zip(&mut actual) {
                near(expected.line_spacing, actual.line_spacing);
                actual.line_spacing = expected.line_spacing;
            }
            assert_eq!(
                serde_json::to_value(&a.paragraphs).unwrap(),
                serde_json::to_value(&actual).unwrap(),
                "an inline picture in a sibling paragraph changed {}",
                text(b)
            );
        }
    }
    let same_table = compile(&format!(
        "#context [#table(columns:(240pt,auto),align:(left,right),[Before {picture} after],[AVATAR])]"
    ));
    let t = tables(&same_table)[0];
    assert!(!t.cells[0].paragraphs[0].lines.is_empty());
    assert!(t.cells[1].paragraphs[0].lines.is_empty());
    assert_eq!(t.cells[1].paragraphs[0].alignment, "r");
    assert!(!t.cells[1].paragraphs[0].runs[0].advances.is_empty());
    xml_with_pictures(&same_table, 1);
}

#[test]
fn exact_integer_advances_do_not_get_arbitrary_tracking() {
    // At 125pt, this font's nominal advances are exact multiples of 1/8pt.
    // There is no kerning/advance-rounding difference to trigger compensation.
    let p = compile("#set text(size:125pt)\n#table(inset:8pt,[HH])");
    let t = tables(&p)[0];
    let cell = &t.cells[0];
    let run = &cell.paragraphs[0].runs[0];
    assert_eq!(run.style.size, 125.);
    assert_eq!(cell.inset, [8.; 4]);
    assert_eq!(cell.text_inset[1], 8.);
    assert_eq!(cell.text_inset[3], 8.);
    assert_eq!(run.style.letter_spacing, 0.);
    let longer =
        compile("#set page(width:2000pt)\n#set text(size:125pt)\n#table(inset:8pt,[HHHHHH])");
    let roomy = compile("#set text(size:125pt)\n#table(columns:500pt,inset:8pt,[HH])");
    assert_eq!(
        tables(&roomy)[0].cells[0].paragraphs[0].runs[0]
            .style
            .letter_spacing,
        0.
    );
    for p in [&p, &longer, &roomy] {
        let s = xml(p);
        let doc = roxmltree::Document::parse(&s).unwrap();
        assert!(
            doc.descendants()
                .filter(|n| n.tag_name().name() == "rPr")
                .all(|n| n.attribute("spc") == Some("0"))
        );
    }
}

#[test]
fn table_formatting_preserves_shaped_character_origins_and_native_cells() {
    for size in [15., 20., 20.05, 32.] {
        for align in ["left", "center", "right"] {
            let p = compile(&format!(
                "#set text(size:{size}pt)\n#table(inset:8pt,align:{align},[AVATAR],[0.95])"
            ));
            let table = tables(&p)[0];
            let s = xml(&p);
            let doc = roxmltree::Document::parse(&s).unwrap();
            let cells: Vec<_> = doc
                .descendants()
                .filter(|n| n.tag_name().name() == "tc")
                .collect();
            assert_eq!(cells.len(), 2);
            for (cell, native) in table.cells.iter().zip(cells) {
                assert_eq!(
                    native
                        .descendants()
                        .filter(|n| n.tag_name().name() == "p")
                        .count(),
                    1
                );
                assert_eq!(cell.inset, [8.; 4]);
                let metrics: Vec<_> = cell.paragraphs[0]
                    .runs
                    .iter()
                    .flat_map(|r| &r.advances)
                    .collect();
                assert_eq!(metrics.len(), text(cell).chars().count());
                let mut source: f64 = 0.;
                let mut placed: f64 = 0.;
                let mut index = 0;
                for run in native.descendants().filter(|n| n.tag_name().name() == "r") {
                    let props = run
                        .children()
                        .find(|n| n.tag_name().name() == "rPr")
                        .unwrap();
                    assert_eq!(props.attribute("kern"), Some("0"));
                    let spacing = props.attribute("spc").unwrap().parse::<f64>().unwrap() / 100.;
                    for ch in run
                        .children()
                        .find(|n| n.tag_name().name() == "t")
                        .unwrap()
                        .text()
                        .unwrap()
                        .chars()
                    {
                        let metric = metrics[index];
                        assert_eq!(ch, metric.character);
                        assert!((placed - source).abs() <= 0.00501, "{placed} != {source}");
                        source += metric.shaped * size;
                        placed += (metric.nominal * size * 8.).round() / 8. + spacing;
                        index += 1;
                    }
                }
                let available = (table.column_widths[0] * 12700.).round() / 12700. - 16.;
                assert!(placed <= (available * 8.).floor() / 8. + 1e-6);
                assert!(source - placed < 0.136);
            }
        }
    }
}

#[test]
fn complex_clusters_keep_native_shaping_instead_of_character_spacing() {
    let p = compile("#table([office],[é],[#text(dir:rtl,lang:\"ar\")[العربية]])");
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    for (source, cell) in tables(&p)[0]
        .cells
        .iter()
        .zip(doc.descendants().filter(|n| n.tag_name().name() == "tc"))
    {
        for opaque in source
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .filter(|r| r.advances.is_empty())
        {
            let intact = cell
                .descendants()
                .filter(|n| n.tag_name().name() == "r")
                .find(|n| {
                    n.children().any(|t| {
                        t.tag_name().name() == "t" && t.text() == Some(opaque.text.as_str())
                    })
                })
                .unwrap();
            let props = intact
                .children()
                .find(|n| n.tag_name().name() == "rPr")
                .unwrap();
            assert_eq!(props.attribute("spc"), Some("0"));
            assert_eq!(props.attribute("kern"), Some("1"));
        }
    }
}

#[test]
fn a_rowspan_split_across_pages_preserves_all_text_once() {
    for direction in ["ltr", "rtl"] {
        let source = "#table(columns:(150pt,100pt),table.cell(rowspan:3)[#lorem(230)], [Side A],[Side B],[Side C])";
        let source = format!("#set text(dir:{direction})\n{source}");
        let p = compile(&source);
        let actual: String = tables(&p).iter().flat_map(|t| &t.cells).map(text).collect();
        let reference = compile(&format!("#set page(height:5000pt)\n{source}"));
        let expected: String = tables(&reference)
            .iter()
            .flat_map(|t| &t.cells)
            .map(text)
            .collect();
        let normalize = |s: &str| {
            s.replace("Side A", "")
                .replace("Side B", "")
                .replace("Side C", "")
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
        };
        assert_eq!(normalize(&actual), normalize(&expected));
        for side in ["Side A", "Side B", "Side C"] {
            assert_eq!(actual.matches(side).count(), 1);
        }
        pptx::write(&p).unwrap();
    }
}
