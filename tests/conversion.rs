use ooxmlsdk::common::resolve_relationship_target_path;
use std::{
    fs,
    io::{Cursor, Read},
    process::Command,
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn compile(source: &str) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.typ");
    fs::write(&path, format!("#set page(width: 480pt, height: 360pt, margin: 30pt)\n#set text(font: \"Libertinus Serif\", size: 18pt)\n{source}")).unwrap();
    let world = CompilerWorld::new(&path, None, &[], &[]).unwrap();
    lower::convert(&world.compile().unwrap().0).unwrap()
}

fn blocks(p: &Presentation) -> Vec<&TextBlock> {
    p.slides
        .iter()
        .flat_map(|s| &s.elements)
        .filter_map(|e| match e {
            Element::Text(b) => Some(b),
            _ => None,
        })
        .collect()
}
fn text(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.as_str()).collect()
}
fn part(p: &Presentation, name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    let mut xml = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[test]
fn nested_lists_are_one_textbox_with_real_paragraphs() {
    let p = compile("- First *bold* and _italic_\n  - Nested\n- Last");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    assert_eq!(b.len(), 1);
    assert_eq!(
        b[0].paragraphs.iter().map(|p| p.level).collect::<Vec<_>>(),
        [0, 1, 0]
    );
    assert_eq!(text(&b[0].paragraphs[0]), "First bold and italic");
    assert!(b[0].paragraphs[0].runs.iter().any(|r| r.style.bold));
    assert!(b[0].paragraphs[0].runs.iter().any(|r| r.style.italic));
    let xml = part(&p, "ppt/slides/slide1.xml");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        doc.descendants()
            .filter(|n| n.has_tag_name((
                "http://schemas.openxmlformats.org/presentationml/2006/main",
                "sp"
            )))
            .count(),
        1
    );
    assert_eq!(
        doc.descendants()
            .filter(|n| n.tag_name().name() == "p")
            .count(),
        3
    );
    assert!(
        !doc.descendants()
            .filter(|n| n.tag_name().name() == "t")
            .any(|n| n.text().unwrap_or("").contains('•'))
    );
}

#[test]
fn soft_wrapping_does_not_split_a_paragraph() {
    let sentence = "This long item wraps across several lines but remains a single paragraph when converted into an editable PowerPoint list. ".repeat(3);
    let p = compile(&format!("- {sentence}\n- End"));
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    assert_eq!(b[0].paragraphs.len(), 2);
    assert_eq!(text(&b[0].paragraphs[0]).trim(), sentence.trim());
}

#[test]
fn numbering_keeps_sequence_start_for_powerpoint() {
    let p = compile("#enum(start: 3)[Third][Fourth][Fifth]");
    let b = blocks(&p);
    for par in &b[0].paragraphs {
        assert!(matches!(
            par.bullet,
            Some(Bullet::Number { start: Some(3), .. })
        ));
    }
    let xml = part(&p, "ppt/slides/slide1.xml");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let starts: Vec<_> = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "pPr")
        .flat_map(|p| p.children())
        .filter(|n| n.tag_name().name() == "buAutoNum")
        .map(|n| n.attribute("startAt"))
        .collect();
    assert_eq!(starts, [Some("3"); 3]);
}

#[test]
fn evaluated_loops_and_imports_keep_list_semantics() {
    let p = compile("#for i in range(3) { list([Item #i]) }");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let text: Vec<_> = blocks(&p)
        .iter()
        .flat_map(|b| b.paragraphs.iter().map(text))
        .collect();
    assert_eq!(text, ["Item 0", "Item 1", "Item 2"]);
}

#[test]
fn explicit_breaks_and_multiple_paragraphs_are_preserved() {
    let p = compile("- First line\\\n  second line\n\n  second paragraph\n- Final item");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    assert_eq!(b[0].paragraphs.len(), 3);
    assert_eq!(text(&b[0].paragraphs[0]), "First line\nsecond line");
    assert!(b[0].paragraphs[1].bullet.is_none());
    assert!(b[0].paragraphs[2].bullet.is_some());
    assert!(part(&p, "ppt/slides/slide1.xml").contains("<a:br>"));
}

#[test]
fn simple_tables_are_native_cells_with_correct_graphic_type() {
    let p = compile("#table(columns: (1fr, 1fr), inset: 8pt, [A], [B], [C], [D])");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let table = p.slides[0]
        .elements
        .iter()
        .find_map(|e| {
            if let Element::Table(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(table.cells.len(), 4);
    assert_eq!(table.cells[0].inset, [8.0; 4]);
    let xml = part(&p, "ppt/slides/slide1.xml");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        doc.descendants()
            .find(|n| n.tag_name().name() == "graphicData")
            .unwrap()
            .attribute("uri"),
        Some("http://schemas.openxmlformats.org/drawingml/2006/table")
    );
    assert_eq!(
        doc.descendants()
            .filter(|n| n.tag_name().name() == "tc")
            .count(),
        4
    );
}

#[test]
fn lists_with_equations_keep_native_list_and_office_math() {
    let p = compile("- Plain\n- Equation $x^2 + y^2 = z^2$");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].paragraphs.len(), 2);
    assert!(b[0].paragraphs[1].bullet.is_some());
    assert!(b[0].paragraphs[1].runs.iter().any(|r| r.math.is_some()));
    let xml = part(&p, "ppt/slides/slide1.xml");
    assert!(xml.contains("<m:sSup>"));
    assert!(!xml.contains("<p:pic>"));
}

#[test]
fn empty_items_never_disappear_silently() {
    let p = compile("#list([First], [], [Last])");
    let count = blocks(&p)
        .iter()
        .flat_map(|b| &b.paragraphs)
        .filter(|p| p.bullet.is_some())
        .count();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert_eq!(count, 3);
    assert!(part(&p, "ppt/slides/slide1.xml").contains("buChar"));
}

#[test]
fn native_table_does_not_silently_erase_graphics_in_empty_text_cells() {
    let p = compile("#table(columns: 2, [A], [#rect(width: 20pt,height:20pt,fill:red)])");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let elements: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .collect();
    assert!(elements.iter().any(|e| matches!(e, Element::Table(_))));
    assert!(
        elements
            .iter()
            .any(|e| matches!(e, Element::Shape(s) if s.fill.is_some()))
    );
}

#[test]
fn package_xml_and_relationship_targets_are_valid() {
    let p =
        compile("= Heading\n- #link(\"https://example.com/?a=1&b=2\")[A & B]\n#pagebreak()\n$x^2$");
    let mut archive = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let names: std::collections::HashSet<_> = archive.file_names().map(str::to_owned).collect();
    for name in &names {
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        let mut xml = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{name}: {e}"));
        if name.ends_with(".rels") {
            // Package paths use '/' on every OS, including Windows.
            let (base, _) = name.rsplit_once("_rels/").unwrap();
            for rel in doc
                .root_element()
                .children()
                .filter(|n| n.is_element() && n.attribute("TargetMode") != Some("External"))
            {
                let resolved =
                    resolve_relationship_target_path(base, rel.attribute("Target").unwrap());
                assert!(names.contains(&resolved), "{name}: missing {resolved}");
            }
        }
    }
}

#[test]
fn strict_cli_writes_report_but_preserves_existing_pptx() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("math.typ");
    let output = dir.path().join("math.pptx");
    let report = dir.path().join("report.json");
    fs::write(&input, "#skew(ax:15deg)[Unsupported shear]").unwrap();
    fs::write(&output, "existing").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_typptx"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--strict")
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(output).unwrap(), "existing");
    assert!(!fs::read_to_string(report).unwrap().is_empty());
}

#[test]
fn different_page_sizes_are_rejected() {
    let p = compile("First\n#pagebreak()\n#set page(width: 600pt)\nSecond");
    assert!(
        pptx::write(&p)
            .unwrap_err()
            .to_string()
            .contains("single slide size")
    );
}

#[test]
fn table_paragraphs_and_alignment_are_kept() {
    let p = compile("#table(columns: 1, align: center, [First\n\nSecond])");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let Element::Table(t) = &p.slides[0].elements[0] else {
        panic!("expected native table")
    };
    assert_eq!(
        t.cells[0].paragraphs.iter().map(text).collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert!(t.cells[0].paragraphs.iter().all(|p| p.alignment == "ctr"));
}

#[test]
fn table_gutters_and_hidden_track_boundaries_are_native() {
    for source in [
        "#table(columns: 2, table.cell(colspan: 2)[Merged])",
        "#table(columns: 2, column-gutter: 8pt, [A], [B])",
    ] {
        let p = compile(source);
        assert!(p.diagnostics.is_empty(), "{source}: {:?}", p.diagnostics);
        assert!(
            p.slides[0]
                .elements
                .iter()
                .any(|e| matches!(e, Element::Table(_))),
            "{source}"
        );
    }
}

#[test]
fn numbering_continues_across_pages_and_respects_resets() {
    let p = compile("#set page(height: 170pt)\n#enum(start: 7, ..range(10).map(i=>[Item #i]))");
    assert!(p.slides.len() > 1);
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    let mut count = 0;
    for block in b {
        for par in &block.paragraphs {
            assert_eq!(text(par), format!("Item {count}"));
            count += 1;
        }
        let first = &block.paragraphs[0];
        let expected: u32 = text(first)
            .strip_prefix("Item ")
            .unwrap()
            .parse::<u32>()
            .unwrap()
            + 7;
        assert!(matches!(first.bullet,Some(Bullet::Number{start:Some(n),..}) if n==expected));
    }
    assert_eq!(count, 10);
    let p = compile("3. Third\n4. Fourth\n9. Ninth\n10. Tenth");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    let starts: Vec<_> = b[0]
        .paragraphs
        .iter()
        .map(|p| match p.bullet {
            Some(Bullet::Number { start, .. }) => start,
            _ => None,
        })
        .collect();
    assert_eq!(starts, [Some(3), Some(3), Some(9), Some(9)]);
}

#[test]
fn unsupported_text_shear_is_reported() {
    let source = "#skew(ax:15deg)[- Skewed\n- List]";
    let p = compile(source);
    assert!(!p.diagnostics.is_empty(), "{source}");
    assert!(blocks(&p).is_empty());
}

#[test]
fn cli_protects_source_and_rejects_output_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.typ");
    fs::write(&input, "Original source").unwrap();
    for option in ["-o", "--report", "--dump-ir"] {
        let result = Command::new(env!("CARGO_BIN_EXE_typptx"))
            .arg(&input)
            .arg(option)
            .arg(&input)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(fs::read_to_string(&input).unwrap(), "Original source");
    }
    let output = dir.path().join("same.file");
    let result = Command::new(env!("CARGO_BIN_EXE_typptx"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--report")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
}

#[test]
fn compiler_errors_are_actionable_and_leave_no_pptx() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("broken.typ");
    fs::write(&input, "#no-such-function()").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_typptx"))
        .arg(&input)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("unknown variable"), "{error}");
    assert!(!input.with_extension("pptx").exists());
}

#[test]
fn typographic_ligatures_and_hyphenation_keep_original_characters() {
    let source = "affinity efficiency internationalization characterization";
    let p = compile(&format!(
        "#set page(width: 160pt)\n#set text(hyphenate: true)\n- {source}"
    ));
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert_eq!(text(&blocks(&p)[0].paragraphs[0]), source);
}

#[test]
fn numbering_outside_office_range_preserves_the_realized_label() {
    for start in [32768_u64, 4294967297] {
        let p = compile(&format!("#enum(start: {start})[Item]"));
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        assert!(
            matches!(&blocks(&p)[0].paragraphs[0].bullet,Some(Bullet::Character{character,..}) if character==&format!("{start}."))
        );
        pptx::write(&p).unwrap();
    }
}

#[test]
fn custom_and_reversed_labels_remain_native_list_paragraphs() {
    for (source, labels) in [
        ("#list(marker:[=>],[A],[B])", vec!["=>", "=>"]),
        ("#enum(reversed:true)[A][B][C]", vec!["3.", "2.", "1."]),
    ] {
        let p = compile(source);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        let actual: Vec<_> = blocks(&p)[0]
            .paragraphs
            .iter()
            .map(|p| match &p.bullet {
                Some(Bullet::Character { character, .. }) => character.as_str(),
                _ => panic!("missing custom bullet"),
            })
            .collect();
        assert_eq!(actual, labels);
        part(&p, "ppt/slides/slide1.xml");
    }
}

#[test]
fn rtl_paragraphs_keep_logical_order_styles_and_native_list_margins() {
    let p = compile(
        r#"#set text(font: "Arial", lang: "he", dir: rtl)
שלום *עולם* 123 ABC שלום.

- שלום עולם
  - פריט שני
- סוף

+ ראשון
+ שני
"#,
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    let first = &b[0].paragraphs[0];
    assert_eq!(text(first), "שלום עולם 123 ABC שלום.");
    assert!(first.rtl);
    assert_eq!(first.alignment, "r");
    assert!(
        first
            .runs
            .iter()
            .any(|r| r.text == "עולם" && r.style.bold && r.style.rtl)
    );
    assert!(
        first
            .runs
            .iter()
            .any(|r| r.text.contains("ABC") && !r.style.rtl)
    );
    for block in b.iter().filter(|b| b.role == "list") {
        for par in &block.paragraphs {
            assert!(par.rtl);
            assert_eq!(par.margin_left, 0.);
            assert!(par.margin_right > 0.);
            assert!(par.indent < 0.);
            assert!(par.bullet.is_some());
        }
    }
    let xml = part(&p, "ppt/slides/slide1.xml");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert!(
        doc.descendants()
            .filter(|n| n.tag_name().name() == "pPr")
            .all(|n| n.attribute("rtl") == Some("1"))
    );
}

#[test]
fn rtl_table_cells_keep_their_physical_columns_and_source_insets() {
    let p = compile(
        r#"#set text(font: "Arial", lang: "he", dir: rtl)
#table(columns: (90pt, 150pt, 60pt), gutter: 8pt, inset: (left: 4pt, right: 9pt),
 [ראשון], [שני], [שלישי], table.cell(colspan: 2)[מיזוג], [סוף])"#,
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let t = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(t.column_widths, vec![60., 8., 150., 8., 90.]);
    let first = t
        .cells
        .iter()
        .find(|c| c.paragraphs.iter().any(|p| text(p) == "ראשון"))
        .unwrap();
    assert_eq!(first.column, 4);
    assert!(first.paragraphs[0].rtl);
    assert_eq!(first.inset[1], 9.);
    assert_eq!(first.inset[3], 4.);
    let merged = t
        .cells
        .iter()
        .find(|c| c.paragraphs.iter().any(|p| text(p) == "מיזוג"))
        .unwrap();
    assert_eq!(merged.column, 2);
    assert_eq!(merged.column_span, 3);
    pptx::write(&p).unwrap();
}

#[test]
fn rtl_empty_items_and_tab_stops_use_the_same_leading_edge() {
    let p = compile(
        r#"#set text(font:"Arial",size:20pt,lang:"he",dir:rtl)
#list([], [שלום])

א #h(40pt) ב #h(30pt) ג"#,
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let b = blocks(&p);
    let items = &b.iter().find(|b| b.role == "list").unwrap().paragraphs;
    assert!((items[0].margin_right - items[1].margin_right).abs() < 0.01);
    assert!((items[0].indent - items[1].indent).abs() < 0.01);
    let tabbed = &b.iter().find(|b| b.role != "list").unwrap().paragraphs[0];
    assert_eq!(text(tabbed), "א \tב \tג");
    assert_eq!(tabbed.tab_stops.len(), 2);
    assert!((40. ..80.).contains(&tabbed.tab_stops[0]));
    assert!(tabbed.tab_stops[1] > tabbed.tab_stops[0]);
    let xml = part(&p, "ppt/slides/slide1.xml");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    for n in doc.descendants().filter(|n| {
        n.tag_name().name() == "pPr" && n.children().any(|c| c.tag_name().name() == "buChar")
    }) {
        assert!(n.attribute("marL").unwrap().parse::<i32>().unwrap() > 0);
        assert_eq!(n.attribute("marR"), Some("0"));
    }
}
