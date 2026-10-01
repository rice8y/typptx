use std::{
    fs,
    io::{Cursor, Read},
    process::Command,
};
use typptx::{
    ir::*,
    lower::{self, MathFormat, Options},
    pptx,
    world::CompilerWorld,
};

fn document(source: &str) -> typst_layout::PagedDocument {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("math.typ");
    fs::write(&input, format!("#set page(width:600pt,height:600pt,margin:25pt)\n#set text(font: \"Libertinus Serif\",size:20pt)\n{source}")).unwrap();
    CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0
}

fn svg(doc: &typst_layout::PagedDocument, dpi: Option<u32>) -> Presentation {
    let p = lower::convert_with_options(
        doc,
        &Options {
            math_format: MathFormat::Svg,
            image_dpi: dpi,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}

fn slide_xml(p: &Presentation) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    roxmltree::Document::parse(&xml).unwrap();
    xml
}

fn images(p: &Presentation) -> Vec<(&Rect, &str, &[u8])> {
    p.slides
        .iter()
        .flat_map(|s| &s.elements)
        .filter_map(|e| match e {
            Element::MathSvg {
                bounds, svg, png, ..
            } => Some((bounds, svg.as_str(), png.as_slice())),
            _ => None,
        })
        .collect()
}

#[test]
fn compound_marker_math_stays_inside_the_picture_bullet() {
    let doc = document(
        "#set list(marker:box(width:30pt)[#box(rect(width:8pt,height:8pt,fill:red))$x^2$])\n- First\n- Second\n\n$y^2$",
    );
    let office = lower::convert(&doc).unwrap();
    let vector = svg(&doc, None);
    let markers = |p: &Presentation| {
        p.slides
            .iter()
            .flat_map(|s| &s.elements)
            .flat_map(Element::walk)
            .filter_map(|e| match e {
                Element::Text(t) if t.role == "list" => Some(t),
                _ => None,
            })
            .flat_map(|t| &t.paragraphs)
            .filter_map(|p| match &p.bullet {
                Some(Bullet::Picture { svg, .. }) => svg.clone(),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(markers(&office).len(), 2);
    assert_eq!(markers(&vector), markers(&office));
    assert_eq!(
        images(&vector).len(),
        1,
        "only the standalone equation is an SVG object"
    );
}

#[test]
fn math_only_markers_are_identical_in_office_and_svg_modes() {
    for marker in ["$x^2$", "$frac(1,2)$", "$sqrt(x)$"] {
        let doc = document(&format!(
            "#set list(marker:[{marker}])\n- First\n- Second\n\n$y^2$"
        ));
        let office = lower::convert(&doc).unwrap();
        let vector = svg(&doc, None);
        assert!(office.diagnostics.is_empty(), "{:?}", office.diagnostics);
        let markers = |p: &Presentation| {
            p.slides
                .iter()
                .flat_map(|s| &s.elements)
                .flat_map(Element::walk)
                .filter_map(|e| match e {
                    Element::Text(t) if t.role == "list" => Some(t),
                    _ => None,
                })
                .flat_map(|t| &t.paragraphs)
                .filter_map(|p| match &p.bullet {
                    Some(Bullet::Picture { svg, .. }) => svg.clone(),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(markers(&office).len(), 2);
        assert_eq!(markers(&vector), markers(&office));
        assert_eq!(images(&vector).len(), 1);
    }
}

#[test]
fn office_is_default_and_svg_keeps_native_text_lists_and_tables() {
    let doc = document(
        "Inline $frac(a,b)$ after.\n\n$ frac(a,b) + sqrt(x) $\n\n- Before $x^2$ after.\n- Second item\n\n#table(columns:2,[$frac(a,b)$],[after])",
    );
    let office = lower::convert(&doc).unwrap();
    assert!(office.diagnostics.is_empty(), "{:?}", office.diagnostics);
    assert!(images(&office).is_empty());
    assert!(slide_xml(&office).contains("<m:f>"));
    let p = svg(&doc, None);
    assert_eq!(images(&p).len(), 4);
    let xml = slide_xml(&p);
    assert!(!xml.contains("<m:oMath"));
    assert_eq!(xml.matches("<asvg:svgBlip ").count(), 4);
    assert!(xml.contains("<a:tbl>"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        parsed
            .descendants()
            .filter(|n| n.tag_name().name() == "buChar"
                && n.parent().is_some_and(|p| p.tag_name().name() == "pPr"))
            .count(),
        2
    );
    let text: String = parsed
        .descendants()
        .filter(|n| n.tag_name().name() == "t")
        .filter_map(|n| n.text())
        .collect();
    assert!(text.contains("Inline ") && text.contains(" after."));
    assert!(
        p.slides[0]
            .elements
            .iter()
            .all(|e| !matches!(e, Element::Drawing { .. }))
    );
    for (bounds, source, png) in images(&p) {
        assert!(bounds.width < 120. && bounds.height < 100., "{bounds:?}");
        assert!(source.contains("<path") && !source.contains("<text"));
        assert!(
            image::load_from_memory(png)
                .unwrap()
                .to_rgba8()
                .pixels()
                .any(|p| p[3] == 0)
        );
    }
    let list = p.slides[0]
        .elements
        .iter()
        .find_map(|e| {
            if let Element::Text(t) = e {
                (t.role == "list").then_some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(list.paragraphs.len(), 2);
    assert!(!list.wrap);
    assert!(!list.paragraphs[0].tab_stops.is_empty());
}

#[test]
fn svg_handles_unsupported_math_numbering_and_non_text_content() {
    let doc = document(
        "#set math.equation(numbering: \"(1)\")\n$ mat(1, 2; 3, 4; augment: #1) $\n\n$ attach(x, tl: a, bl: b) $\n\n$ #rect(width:24pt,height:12pt,fill:red) $\n\n- $mat(1, 2; 3, 4; augment: #1)$",
    );
    assert!(pptx::write(&lower::convert(&doc).unwrap()).is_err());
    let p = svg(&doc, None);
    assert_eq!(images(&p).len(), 4);
    assert!(slide_xml(&p).contains("<a:buChar "));
    assert!(images(&p)[0].0.width > 250. && images(&p)[0].0.right() > 570.);
}

#[test]
fn dpi_changes_only_svg_compatibility_png_and_not_equation_placement() {
    let doc = document("$ frac(a+b,c) + sqrt(x) $");
    let low = svg(&doc, Some(72));
    let high = svg(&doc, Some(288));
    let low = images(&low)[0];
    let high = images(&high)[0];
    assert_eq!(low.0, high.0);
    assert_eq!(low.1, high.1);
    let a = image::load_from_memory(low.2).unwrap();
    let b = image::load_from_memory(high.2).unwrap();
    assert!((b.width() as i64 - a.width() as i64 * 4).abs() <= 4);
    assert!((b.height() as i64 - a.height() as i64 * 4).abs() <= 4);
}

#[test]
fn hidden_nested_math_stays_hidden_and_raw_code_stays_text() {
    let hidden = svg(&document("$ f(x) = #hide[$x^2 + 42$] $\n\n`$x^2$`"), None);
    let visible = svg(&document("$ f(x) = x^2 + 42 $"), None);
    assert_eq!(images(&hidden).len(), 1);
    assert!(images(&hidden)[0].0.width < images(&visible)[0].0.width);
    assert!(slide_xml(&hidden).contains("$x^2$"));
}

#[test]
fn svg_preserves_paragraph_line_breaks_and_supports_strict_cli() {
    let source = "#set page(width:350pt,height:300pt,margin:25pt)\n#set text(font:\"Libertinus Serif\",size:18pt)\n#block(width:160pt)[Before $x^2$ and enough words to wrap onto the next line after $y^3$.]";
    let p = svg(&document(source), None);
    let text = p.slides[0]
        .elements
        .iter()
        .find_map(|e| {
            if let Element::Text(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert!(!text.wrap);
    assert!(
        text.paragraphs[0]
            .runs
            .iter()
            .any(|r| r.text.contains('\n'))
    );
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.typ");
    let output = dir.path().join("source.pptx");
    fs::write(
        &input,
        "#set text(font:\"Libertinus Serif\")\n$mat(1,2;3,4; augment:#1)$",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_typptx"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--math-format", "svg", "--strict"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read(&output).unwrap();
    let invalid = Command::new(env!("CARGO_BIN_EXE_typptx"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--math-format", "unknown"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert_eq!(original, fs::read(output).unwrap());
}

#[test]
fn svg_keeps_transforms_clipping_and_equations_without_text() {
    let p = svg(
        &document(
            "#rotate(20deg)[$ frac(a,b) $]\n\n#box(width:18pt,height:22pt,clip:true)[$frac(a+b,c+d)$]",
        ),
        None,
    );
    assert_eq!(images(&p).len(), 2);
    assert!(images(&p).iter().any(|(_, s, _)| s.contains("clipPath")));
    pptx::write(&p).unwrap();
    let p = svg(
        &document("$ #rect(width:24pt,height:12pt,fill:red) $"),
        None,
    );
    assert_eq!(p.slides[0].elements.len(), 1);
    assert_eq!(images(&p).len(), 1);
}

#[test]
fn svg_mode_does_not_enable_unrelated_image_fallback() {
    let doc = document("#table(columns:2, [$x^2$],[#skew(ax:15deg)[text]])");
    let p = lower::convert_with_options(
        &doc,
        &Options {
            math_format: MathFormat::Svg,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        p.diagnostics
            .iter()
            .any(|d| d.code == "unsupported_structure")
    );
    assert!(pptx::write(&p).is_err());
    let p = lower::convert_with_options(
        &doc,
        &Options {
            math_format: MathFormat::Svg,
            allow_image_fallback: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        images(&p).is_empty(),
        "parent fallback must not duplicate its equations"
    );
    assert!(
        p.slides[0]
            .elements
            .iter()
            .any(|e| matches!(e, Element::Drawing { .. }))
    );
    pptx::write(&p).unwrap();
}

#[test]
fn equation_tabs_are_local_to_each_source_line_without_repeating_bullets() {
    let p = svg(
        &document("- A much longer prefix $x^2$ after. \\\n  Short $y^3$ after.\n- Next item"),
        None,
    );
    let list = p.slides[0]
        .elements
        .iter()
        .find_map(|e| {
            if let Element::Text(t) = e {
                (t.role == "list").then_some(t)
            } else {
                None
            }
        })
        .unwrap();
    let p = &list.paragraphs[0];
    assert_eq!(p.lines.len(), 2);
    assert!(p.lines[0].tab_stops[0] > p.lines[1].tab_stops[0]);
    let xml = slide_xml(&svg(
        &document("- A much longer prefix $x^2$ after. \\\n  Short $y^3$ after.\n- Next item"),
        None,
    ));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let paragraphs: Vec<_> = parsed
        .descendants()
        .filter(|n| n.tag_name().name() == "pPr")
        .collect();
    assert_eq!(paragraphs.len(), 3);
    assert!(
        paragraphs[0]
            .children()
            .any(|n| n.tag_name().name() == "buChar")
    );
    assert!(
        paragraphs[1]
            .children()
            .any(|n| n.tag_name().name() == "buNone")
    );
    for p in &paragraphs[..2] {
        assert_eq!(
            p.descendants()
                .filter(|n| n.tag_name().name() == "tab")
                .count(),
            1
        );
    }
}

#[test]
fn centered_table_text_keeps_its_realized_offset_beside_svg_math() {
    let p = svg(
        &document(
            "#table(columns:(240pt,240pt), align:center, inset:10pt, [Before $x^2$ after],[Other])",
        ),
        None,
    );
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
    let cell = &table.cells[0];
    let line = &cell.paragraphs[0].lines[0];
    let expected = line.x - table.bounds.x - cell.text_inset[3];
    assert!(expected > 20.);
    let xml = slide_xml(&p);
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let props = parsed
        .descendants()
        .find(|n| n.tag_name().name() == "pPr")
        .unwrap();
    assert_eq!(props.attribute("algn"), Some("l"));
    assert!(
        (props.attribute("marL").unwrap().parse::<f64>().unwrap() / 12700. - expected).abs()
            < 0.001
    );
}

#[test]
fn non_math_items_in_the_same_list_keep_their_wrapped_lines() {
    let p = svg(
        &document(
            "#block(width:180pt)[\n- An $x^2$ expression.\n- This second item contains no equation and must still wrap onto several lines.\n]",
        ),
        None,
    );
    let list = p.slides[0]
        .elements
        .iter()
        .find_map(|e| {
            if let Element::Text(t) = e {
                (t.role == "list").then_some(t)
            } else {
                None
            }
        })
        .unwrap();
    assert!(!list.wrap);
    assert!(list.paragraphs[1].lines.len() > 1);
    assert!(
        list.paragraphs[1]
            .runs
            .iter()
            .any(|r| r.text.contains('\n'))
    );
}

#[test]
fn rtl_svg_equations_reserve_their_advance_from_the_right_edge() {
    let doc = document(
        r#"#set text(font:"Arial", lang:"he", dir:rtl)
משוואה $x^2$ סוף

- ראשון $frac(a,b)$ אחרון \ המשך

#table(columns:2,[משוואה $x^2$ סוף],[תא שני])"#,
    );
    let p = svg(&doc, None);
    assert_eq!(images(&p).len(), 3);
    for element in p.slides[0].elements.iter().flat_map(Element::walk) {
        let pars: Vec<_> = match element {
            Element::Text(t) => t.paragraphs.iter().collect(),
            Element::Table(t) => t.cells.iter().flat_map(|c| &c.paragraphs).collect(),
            _ => vec![],
        };
        for par in pars {
            assert!(par.rtl);
            for line in &par.lines {
                assert!(line.right > line.x);
                assert!(
                    line.tab_stops
                        .iter()
                        .all(|&tab| tab >= 0. && tab <= line.right - line.x + 0.01)
                );
            }
        }
    }
    let xml = slide_xml(&p);
    let tree = roxmltree::Document::parse(&xml).unwrap();
    assert!(
        tree.descendants()
            .filter(|n| n.tag_name().name() == "tab")
            .all(|n| n.attribute("algn") == Some("r"))
    );
}
