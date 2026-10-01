use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{lower, pptx, world::CompilerWorld};

const MATH: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

fn convert(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("math.typ");
    fs::write(
        &input,
        format!("#set page(width:720pt,height:540pt,margin:32pt)\n#set text(size:20pt)\n{source}"),
    )
    .unwrap();
    let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    assert!(!zip.file_names().any(|n| n.starts_with("ppt/media/")));
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn contents(node: roxmltree::Node) -> String {
    node.descendants()
        .filter(|n| n.has_tag_name((MATH, "t")))
        .filter_map(|n| n.text())
        .collect()
}

#[test]
fn inline_equations_keep_text_mode_and_base_font_size_in_cells() {
    let xml = convert(
        r#"#table(columns: 300pt, [$frac(a, b)$], [Before $frac(c, d)$ after])
    $ frac(e, f) $"#,
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let equations: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "oMath")))
        .collect();
    assert_eq!(equations.len(), 3);
    fn paragraph<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> roxmltree::Node<'a, 'i> {
        n.ancestors().find(|n| n.tag_name().name() == "p").unwrap()
    }
    let inline = paragraph(equations[0]);
    assert!(
        inline
            .descendants()
            .any(|n| n.tag_name().name() == "t" && n.text() == Some("\u{200b}"))
    );
    assert!(
        inline
            .descendants()
            .filter(|n| n.has_tag_name((
                "http://schemas.openxmlformats.org/drawingml/2006/main",
                "rPr"
            )))
            .all(|n| n.attribute("sz") == Some("2000"))
    );
    assert!(
        !paragraph(equations[1])
            .descendants()
            .any(|n| n.tag_name().name() == "br")
    );
    assert!(
        equations[2]
            .ancestors()
            .any(|n| n.has_tag_name((MATH, "oMathPara")))
    );
    assert!(
        paragraph(equations[2])
            .descendants()
            .any(|n| n.has_tag_name((MATH, "jc")) && n.attribute((MATH, "val")) == Some("center"))
    );
}

#[test]
fn alignment_points_stay_in_native_equation_arrays_and_literal_ampersands_stay_visible() {
    let xml = convert(
        r#"$ x &= 1 \ alpha + beta &= frac(3,4) \ z^2 &= 123 $
        $ a &= b & c &= d \ a+a &= b+b & c+c &= d+d $
        $ "literal &" + x $ $ a &= b $"#,
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let arrays: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "eqArr")))
        .collect();
    assert_eq!(arrays.len(), 2);
    for (array, rows, marks) in [(arrays[0], 3, 1), (arrays[1], 2, 3)] {
        let row: Vec<_> = array
            .children()
            .filter(|n| n.has_tag_name((MATH, "e")))
            .collect();
        assert_eq!(row.len(), rows);
        for r in row {
            assert_eq!(contents(r).matches('&').count(), marks);
        }
        let paragraph = array
            .ancestors()
            .find(|n| n.tag_name().name() == "p")
            .unwrap();
        assert!(
            paragraph
                .descendants()
                .any(|n| n.tag_name().name() == "spcPct" && n.attribute("val") == Some("100000"))
        );
    }
    let ampersands: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "t")) && n.text().is_some_and(|t| t.contains('&')))
        .collect();
    assert_eq!(ampersands.len(), 10);
    let literal = ampersands
        .iter()
        .find(|n| n.text() == Some("literal &"))
        .unwrap();
    assert!(
        literal
            .parent()
            .unwrap()
            .descendants()
            .any(|n| n.has_tag_name((MATH, "lit")))
    );
    assert!(
        doc.descendants()
            .filter(|n| n.has_tag_name((MATH, "oMath")))
            .any(|n| contents(n) == "a = b")
    );
}

#[test]
fn hidden_math_preserves_native_dimensions_inside_fractions_and_matrices() {
    let xml = convert(
        r#"$ a + #hide[$frac(x+1,y+2)$] + b $
        $ mat(1, std.hide(alpha+beta); 4, 5) $
        $ frac(1, #hide[$sqrt(2)$]) $"#,
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let hidden: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "phant")))
        .collect();
    assert_eq!(hidden.len(), 3);
    for node in &hidden {
        assert!(
            node.descendants()
                .any(|n| n.has_tag_name((MATH, "show")) && n.attribute((MATH, "val")) == Some("0"))
        );
        assert!(
            !node
                .descendants()
                .any(|n| ["zeroWid", "zeroAsc", "zeroDesc"].contains(&n.tag_name().name()))
        );
        assert!(!contents(*node).is_empty());
    }
    assert!(hidden[0].descendants().any(|n| n.has_tag_name((MATH, "f"))));
    assert!(hidden[1].ancestors().any(|n| n.has_tag_name((MATH, "m"))));
    let matrix = hidden[1]
        .ancestors()
        .find(|n| n.has_tag_name((MATH, "m")))
        .unwrap();
    assert_eq!(
        matrix
            .children()
            .filter(|n| n.has_tag_name((MATH, "mr")))
            .count(),
        2
    );
    assert!(hidden[2].ancestors().any(|n| n.has_tag_name((MATH, "den"))));
}

#[test]
fn prescripts_keep_both_sides_and_omml_argument_order() {
    let xml = convert("$ attach(x, tl: a, bl: b, tr: c, br: d) + attach(y, tl: n) $");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let prescripts: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "sPre")))
        .collect();
    assert_eq!(prescripts.len(), 2);
    assert_eq!(
        prescripts[0]
            .children()
            .filter(|n| n.is_element())
            .map(|n| n.tag_name().name())
            .collect::<Vec<_>>(),
        ["sub", "sup", "e"]
    );
    assert_eq!(contents(prescripts[0]), "bax");
    let right = prescripts[0]
        .ancestors()
        .find(|n| n.has_tag_name((MATH, "sSubSup")))
        .unwrap();
    assert_eq!(
        contents(
            right
                .children()
                .find(|n| n.has_tag_name((MATH, "sub")))
                .unwrap()
        ),
        "d"
    );
    assert_eq!(
        contents(
            right
                .children()
                .find(|n| n.has_tag_name((MATH, "sup")))
                .unwrap()
        ),
        "c"
    );
    assert_eq!(
        contents(
            prescripts[1]
                .children()
                .find(|n| n.has_tag_name((MATH, "sub")))
                .unwrap()
        ),
        "\u{200b}"
    );
}

#[test]
fn centered_limits_and_corner_attachments_coexist() {
    let xml = convert("$ attach(limits(x), t: u, b: v, tl: a, bl: b, tr: c, br: d) $");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    for (tag, expected) in [("limLow", "v"), ("limUpp", "u")] {
        let limit = doc
            .descendants()
            .find(|n| n.has_tag_name((MATH, tag)))
            .unwrap();
        assert_eq!(
            contents(
                limit
                    .children()
                    .find(|n| n.has_tag_name((MATH, "lim")))
                    .unwrap()
            ),
            expected
        );
    }
    assert_eq!(
        doc.descendants()
            .filter(|n| n.has_tag_name((MATH, "sPre")))
            .count(),
        1
    );
    assert_eq!(
        doc.descendants()
            .filter(|n| n.has_tag_name((MATH, "sSubSup")))
            .count(),
        1
    );
}

#[test]
fn group_characters_and_annotations_remain_structured() {
    let xml = convert(
        "$ underbrace(a+b) + overbrace(x+y, k) + underbracket(a) + overbracket(b, j) + underparen(c) + overparen(d, l) $",
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let groups: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "groupChr")))
        .collect();
    assert_eq!(groups.len(), 6);
    for (i, group) in groups.iter().enumerate() {
        let pos = group
            .descendants()
            .find(|n| n.has_tag_name((MATH, "pos")))
            .unwrap();
        assert_eq!(
            pos.attribute((MATH, "val")),
            Some(if i % 2 == 0 { "bot" } else { "top" })
        );
    }
    let limits: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "limUpp")))
        .collect();
    assert_eq!(limits.len(), 3);
    for (limit, annotation) in limits.into_iter().zip(["k", "j", "l"]) {
        assert_eq!(
            contents(
                limit
                    .children()
                    .find(|n| n.has_tag_name((MATH, "lim")))
                    .unwrap()
            ),
            annotation
        );
    }
}

#[test]
fn bars_binomials_and_fraction_styles_have_native_properties() {
    let xml = convert(
        "$ underline(a+b) + overline(x+y) + binom(n, k_1, k_2) $\n\n$ frac(a,b,style:\"skewed\") + frac(a,b) $\n\n#set math.frac(style:\"horizontal\")\n$ (a+b)/(c+d) $",
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        doc.descendants()
            .filter(|n| n.has_tag_name((MATH, "bar")))
            .count(),
        2
    );
    let kinds: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((MATH, "type")))
        .map(|n| n.attribute((MATH, "val")).unwrap())
        .collect();
    assert_eq!(kinds, ["noBar", "skw", "bar", "lin"]);
    let binom = doc
        .descendants()
        .find(|n| n.has_tag_name((MATH, "f")))
        .unwrap();
    assert_eq!(contents(binom), "nk1,k2");
    let horizontal = doc
        .descendants()
        .rfind(|n| n.has_tag_name((MATH, "f")))
        .unwrap();
    assert_eq!(
        horizontal
            .descendants()
            .filter(|n| n.has_tag_name((MATH, "d")))
            .count(),
        2,
        "horizontal fraction lost grouping parentheses"
    );
}

#[test]
fn resolved_attachment_styles_are_preserved() {
    let xml = convert("#set math.attach(tl: $a$, bl: $b$)\n$ attach(x, tr: c) $");
    assert!(xml.contains("<m:sPre>"));
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let left = doc
        .descendants()
        .find(|n| n.has_tag_name((MATH, "sPre")))
        .unwrap();
    assert_eq!(contents(left), "bax");
}

#[test]
fn nary_operands_do_not_display_empty_input_boxes() {
    let xml = convert("$ sum_(i=1)^n i + integral_0^1 x dif x $");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    for nary in doc.descendants().filter(|n| n.has_tag_name((MATH, "nary"))) {
        let operand = nary
            .children()
            .find(|n| n.has_tag_name((MATH, "e")))
            .unwrap();
        assert_eq!(contents(operand), "\u{200b}");
        assert!(nary.descendants().any(|n| n.has_tag_name((MATH, "ctrlPr"))));
    }
}

#[test]
fn vertical_augmentation_has_editable_segments_and_stretching_separators() {
    let xml = convert(
        "#set math.mat(delim:\"[\", align:right)\n$ mat(1,0,2;0,1,3;augment:#(-1)) $\n\n$ mat(a,b,c;d,e,f;augment:#(vline:(2,1,-1))) $",
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let delimiters: Vec<_> = doc
        .descendants()
        .filter(|n| {
            n.has_tag_name((MATH, "d"))
                && n.children()
                    .filter(|n| n.has_tag_name((MATH, "dPr")))
                    .any(|n| n.descendants().any(|n| n.has_tag_name((MATH, "sepChr"))))
        })
        .collect();
    assert_eq!(delimiters.len(), 2);
    for (delimiter, widths) in delimiters.iter().zip([vec![2, 1], vec![1, 1, 1]]) {
        let matrices: Vec<_> = delimiter
            .descendants()
            .filter(|n| n.has_tag_name((MATH, "m")))
            .collect();
        assert_eq!(matrices.len(), widths.len());
        for (matrix, width) in matrices.iter().zip(widths) {
            let rows: Vec<_> = matrix
                .children()
                .filter(|n| n.has_tag_name((MATH, "mr")))
                .collect();
            assert_eq!(rows.len(), 2);
            for row in rows {
                assert_eq!(
                    row.children()
                        .filter(|n| n.has_tag_name((MATH, "e")))
                        .count(),
                    width
                );
            }
        }
    }
    assert!(
        doc.descendants()
            .any(|n| n.has_tag_name((MATH, "mcJc")) && n.attribute((MATH, "val")) == Some("right"))
    );
    assert!(
        doc.descendants()
            .any(|n| n.has_tag_name((MATH, "begChr")) && n.attribute((MATH, "val")) == Some("["))
    );
}

#[test]
fn unsupported_layouts_are_rejected_and_have_explicit_svg_support() {
    for (expression, reason) in [
        ("x #h(-1em) y", "negative or excessive math spacing"),
        (
            "mat(1,2;3,4;augment:#(hline:1))",
            "layout information from Office",
        ),
        (
            "mat(frac(a,b),2;3,sqrt(x);augment:#(vline:1,stroke:red))",
            "layout information from Office",
        ),
        (
            "cancel(frac(a,b),angle:#35deg)",
            "layout information from Office",
        ),
        ("cancel(x,stroke:#red)", "layout information from Office"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("math.typ");
        fs::write(&input, format!("$ {expression} $")).unwrap();
        let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
            .unwrap()
            .compile()
            .unwrap();
        let presentation = lower::convert(&doc).unwrap();
        assert!(
            presentation
                .diagnostics
                .iter()
                .any(|d| d.message.contains(reason))
        );
        assert!(pptx::write(&presentation).is_err());
        let svg = lower::convert_with_options(
            &doc,
            &lower::Options {
                math_format: lower::MathFormat::Svg,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(svg.diagnostics.is_empty(), "{:?}", svg.diagnostics);
        assert!(pptx::write(&svg).is_ok());
    }
}

#[test]
fn tall_augmented_matrix_segments_share_row_height_without_adding_column_width() {
    let s = convert(
        r#"$ mat(frac(a,b), 2, sqrt(x); 3, mat(1;2), sum_(i=1)^n i; augment:#(vline:(1,2))) $"#,
    );
    let doc = roxmltree::Document::parse(&s).unwrap();
    let delimiter = doc
        .descendants()
        .find(|n| {
            n.has_tag_name((MATH, "d"))
                && n.children().any(|p| {
                    p.has_tag_name((MATH, "dPr"))
                        && p.children().any(|c| {
                            c.has_tag_name((MATH, "sepChr"))
                                && c.attribute((MATH, "val")) == Some("|")
                        })
                })
        })
        .unwrap();
    let parts: Vec<_> = delimiter
        .children()
        .filter(|n| n.has_tag_name((MATH, "e")))
        .collect();
    assert_eq!(parts.len(), 3);
    let mut reference = None;
    for part in parts {
        let matrix = part
            .children()
            .find(|n| n.has_tag_name((MATH, "m")))
            .unwrap();
        let rows: Vec<_> = matrix
            .children()
            .filter(|n| n.has_tag_name((MATH, "mr")))
            .collect();
        assert_eq!(rows.len(), 2);
        let mut hidden = Vec::new();
        for row in rows {
            let cell = row
                .children()
                .find(|n| n.has_tag_name((MATH, "e")))
                .unwrap();
            let phantom = cell
                .children()
                .find(|n| n.has_tag_name((MATH, "phant")))
                .unwrap();
            assert!(
                phantom
                    .descendants()
                    .any(|n| n.has_tag_name((MATH, "show"))
                        && n.attribute((MATH, "val")) == Some("0"))
            );
            assert!(
                phantom
                    .descendants()
                    .any(|n| n.has_tag_name((MATH, "zeroWid"))
                        && n.attribute((MATH, "val")) == Some("1"))
            );
            assert!(
                !phantom.descendants().any(
                    |n| n.has_tag_name((MATH, "zeroAsc")) || n.has_tag_name((MATH, "zeroDesc"))
                )
            );
            hidden.push(contents(phantom));
        }
        if let Some(expected) = &reference {
            assert_eq!(&hidden, expected);
        } else {
            reference = Some(hidden);
        }
    }
}

#[test]
fn display_math_uses_the_equation_frame_instead_of_the_first_glyph() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("math.typ");
    fs::write(&input, "#set page(width:600pt,height:400pt,margin:30pt)\n$ attach(x, tl: a, bl: b) $\n\n$ underbrace(a+b) $").unwrap();
    let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    let capture = typptx::capture::Capture::new(&doc);
    let presentation = lower::convert(&doc).unwrap();
    for element in &presentation.slides[0].elements {
        let typptx::ir::Element::Text(block) = element else {
            panic!("unexpected non-text object")
        };
        assert_eq!(block.role, "equation");
        let node = capture
            .nodes
            .iter()
            .find(|n| n.id() == block.source_id)
            .unwrap();
        if let Some((_, bounds)) = node.pages[&0].layout {
            assert_eq!(block.bounds, bounds);
        } else {
            assert!((block.bounds.x + block.bounds.width / 2. - 300.).abs() < 1.);
            let first = &capture.pages[0][node.pages[&0].leaves[0]];
            assert!(
                block.bounds.x < first.position.0,
                "prescripts or grouping characters were excluded from the bounds"
            );
        }
        assert_eq!(block.paragraphs[0].alignment, "ctr");
        assert!(!block.wrap);
        assert!(
            block.paragraphs[0]
                .runs
                .iter()
                .all(|r| r.style.font == "Cambria Math")
        );
    }
}

#[test]
fn lower_annotations_and_cancellation_remain_office_math() {
    let xml = convert(
        "$ underbrace(a+b,n) + underbracket(c+d,k) + underparen(x+y,m) $\n\n$ cancel(x) + cancel(y,cross:#true) + cancel(z,angle:#90deg) $",
    );
    let d = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        d.descendants()
            .filter(|n| n.has_tag_name((MATH, "eqArr")))
            .count(),
        3
    );
    assert_eq!(
        d.descendants()
            .filter(|n| n.has_tag_name((MATH, "borderBox")))
            .count(),
        3
    );
    assert_eq!(
        d.descendants()
            .filter(|n| n.has_tag_name((MATH, "strikeBLTR")))
            .count(),
        2
    );
    assert_eq!(
        d.descendants()
            .filter(|n| n.has_tag_name((MATH, "strikeTLBR")))
            .count(),
        1
    );
    assert_eq!(
        d.descendants()
            .filter(|n| n.has_tag_name((MATH, "strikeH")))
            .count(),
        1
    );
}
