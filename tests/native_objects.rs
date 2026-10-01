use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::capture::{Capture, Kind};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};
use typst::layout::FrameItem;

fn compile(source: &str) -> Presentation {
    compile_with_capture(source).0
}

fn compile_with_capture(source: &str) -> (Presentation, Capture) {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("test.typ");
    fs::write(&input,format!("#set page(width:600pt,height:400pt,margin:25pt)\n#set text(font:\"Libertinus Serif\",size:20pt)\n{source}")).unwrap();
    let doc = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0;
    (lower::convert(&doc).unwrap(), Capture::new(&doc))
}
fn xml(p: &Presentation) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    assert!(
        !zip.file_names().any(|n| n.starts_with("ppt/media/")),
        "unexpected media fallback"
    );
    let mut s = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

#[test]
fn source_kerning_and_inline_feature_overrides_survive_capture() {
    let (p, c) = compile_with_capture(
        r#"
        #set text(kerning: false)
        AVATAR #text(kerning:true)[WAVE] #text(features:(kern:1))[VA]
        #text(kerning:true,features:(kern:0))[TA]
        #text(kerning:true)[#text(kerning:false)[AA] BB] CC
    "#,
    );
    let expected = [
        ("AVATAR", false),
        ("WAVE", true),
        ("VA", true),
        ("TA", false),
        ("AA", false),
        ("BB", true),
        ("CC", false),
    ];
    for (word, kern) in expected {
        let leaves: Vec<_> = c
            .pages
            .iter()
            .flatten()
            .filter(|l| matches!(&l.item, FrameItem::Text(t) if t.text.trim() == word))
            .collect();
        assert_eq!(leaves.len(), 1, "{word}");
        assert_eq!(leaves[0].kerning, kern, "{word}");
        let runs: Vec<_> = p
            .slides
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
            .flat_map(|t| &t.paragraphs)
            .flat_map(|p| &p.runs)
            .filter(|r| r.text.split_whitespace().any(|s| s == word))
            .collect();
        assert!(!runs.is_empty());
        assert!(runs.iter().all(|r| r.style.kerning == kern));
    }
    let s = xml(&p);
    assert!(s.contains("kern=\"0\""));
    assert!(s.contains("kern=\"1\""));
}

#[test]
fn header_footer_and_grid_labels_are_editable_text() {
    let p = compile("#set page(header:[Header],footer:[Footer])\n#grid(columns:2,[Left],[Right])");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    let text: String = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "t")
        .filter_map(|n| n.text())
        .collect();
    for t in ["Header", "Footer", "Left", "Right"] {
        assert!(text.contains(t), "{t}");
    }
    assert!(
        !p.slides[0]
            .elements
            .iter()
            .any(|e| matches!(e, Element::Drawing { .. }))
    );
}

#[test]
fn continued_lists_keep_page_decorations_separate() {
    let (p, capture) = compile_with_capture(
        r#"
        #set page(header:[Header],footer:context grid(columns:(1fr,1fr,1fr),[Footer],[#rect(width:20pt,height:8pt,fill:blue)],[#counter(page).display()]))
        #list(..range(32).map(i=>[Item #i]))
    "#,
    );
    assert!(p.slides.len() >= 3);
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let mut items = Vec::new();
    for (page, slide) in p.slides.iter().enumerate() {
        let lists: Vec<_> = slide
            .elements
            .iter()
            .flat_map(Element::walk)
            .filter_map(|e| match e {
                Element::Text(t) if t.role == "list" => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(lists.len(), 1);
        let list = lists[0];
        assert!(list.bounds.y > 10.);
        assert!(list.bounds.bottom() < 380.);
        for paragraph in &list.paragraphs {
            assert!(paragraph.bullet.is_some());
            items.push(
                paragraph
                    .runs
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<String>(),
            );
        }
        for leaf in &capture.pages[page] {
            if leaf
                .ancestors
                .iter()
                .any(|&i| capture.nodes[i].content.elem().name() == "artifact")
            {
                assert!(capture.nearest(leaf, Kind::List).is_none());
            }
        }
        let content: String = slide
            .elements
            .iter()
            .flat_map(Element::walk)
            .filter_map(|e| match e {
                Element::Text(t) if t.role != "list" => Some(
                    t.paragraphs
                        .iter()
                        .flat_map(|p| &p.runs)
                        .map(|r| r.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(content.matches("Header").count(), 1);
        assert_eq!(content.matches("Footer").count(), 1);
    }
    assert_eq!(
        items,
        (0..32).map(|i| format!("Item {i}")).collect::<Vec<_>>()
    );
    xml(&p);
}

#[test]
fn paths_and_gradient_fills_are_native_drawingml() {
    let p = compile(
        "#rect(width:80pt,height:40pt,fill:gradient.linear(red,blue))\n#circle(radius:30pt,fill:gradient.radial(white,blue,center:(30%,20%),radius:80%))\n#line(length:80pt,stroke:(paint:red,thickness:2pt,dash:\"dashed\"))",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    for tag in ["<a:custGeom>", "<a:gradFill", "<a:custDash>"] {
        assert!(s.contains(tag), "{tag}");
    }
    assert_eq!(
        p.slides[0]
            .elements
            .iter()
            .filter(|e| matches!(e, Element::Shape(_)))
            .count(),
        2
    );
}

#[test]
fn fractions_roots_and_scripts_are_equation_structures() {
    let p = compile("An inline $x_1^2$ expression.\n\n$ frac(a+b,c) + sqrt(x) $");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    for tag in ["<a14:m", "<m:sSubSup>", "<m:f>", "<m:rad>"] {
        assert!(s.contains(tag), "{tag}");
    }
    roxmltree::Document::parse(&s).unwrap();
}

#[test]
fn hidden_math_does_not_reveal_later_animation_steps() {
    let p = compile("$ f(x) = #hide[$x^2 + 42$] $");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    let hidden = d
        .descendants()
        .find(|n| n.tag_name().name() == "phant")
        .unwrap();
    assert!(hidden.descendants().any(|n| {
        n.tag_name().name() == "show"
            && n.attributes()
                .any(|a| a.name() == "val" && a.value() == "0")
    }));
    assert!(
        d.descendants()
            .filter(|n| n.tag_name().name() == "t" && n.text().is_some_and(|t| t.contains("42")))
            .all(|n| n.ancestors().any(|p| p == hidden))
    );
}

#[test]
fn unsupported_math_is_reported_instead_of_losing_its_structure() {
    let p = compile("$ x #h(-1em) y $");
    assert!(!p.diagnostics.is_empty());
    assert!(pptx::write(&p).is_err());
}

#[test]
fn alignment_comes_from_resolved_styles_and_keeps_the_container_width() {
    let p = compile(
        "#align(center)[Same text]\n#align(right)[Same text]\n#block(width:200pt)[#set align(center)\nSame text]\nSame text",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let blocks: Vec<_> = p.slides[0]
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 4);
    for (block, (alignment, width)) in
        blocks
            .iter()
            .zip([("ctr", 550.), ("r", 550.), ("ctr", 200.), ("l", 550.)])
    {
        assert_eq!(block.paragraphs[0].alignment, alignment);
        assert!((block.bounds.x - 25.).abs() < 0.01, "{:?}", block.bounds);
        assert!(
            (block.bounds.width - width).abs() < 0.01,
            "{:?}",
            block.bounds
        );
    }
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    let alignments: Vec<_> = d
        .descendants()
        .filter(|n| n.tag_name().name() == "pPr")
        .filter_map(|n| n.attribute("algn"))
        .collect();
    assert_eq!(alignments, ["ctr", "r", "ctr", "l"]);
}

#[test]
fn centered_grid_labels_keep_their_own_columns_after_frame_flattening() {
    let p = compile(
        "#align(center)[#grid(columns:3, column-gutter:20pt, [*Left title* #rect(width:100pt,height:60pt)], [*Middle title*#footnote[Note] #v(10pt) Middle content], [*Right title* #rect(width:100pt,height:60pt)])]",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let labels: Vec<_> = p.slides[0]
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Text(t)
                if t.paragraphs[0]
                    .runs
                    .iter()
                    .filter(|r| r.style.bold)
                    .map(|r| r.text.as_str())
                    .collect::<String>()
                    .ends_with("title") =>
            {
                Some(t)
            }
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 3);
    assert!(labels.iter().all(|t| t.paragraphs[0].alignment == "ctr"));
    assert!((labels[0].bounds.width - 100.).abs() < 0.01);
    assert!((labels[2].bounds.width - 100.).abs() < 0.01);
    for pair in labels.windows(2) {
        assert!((pair[1].bounds.x - pair[0].bounds.right() - 20.).abs() < 0.01);
    }
    xml(&p);
}

#[test]
fn bibliography_labels_share_paragraphs_with_wrapped_entries() {
    let data = "short:\n  type: Book\n  title: Brief work\n  author: Example, Alice\n  date: 2024\nlong:\n  type: Web\n  title: A longer reference whose title wraps across several lines while the number stays on the first line\n  author: Example, Bob\n  date: 2025\n  url: https://example.org/reference\n";
    let p = compile(&format!(
        "#set text(size:12pt)\n#set page(width:300pt)\n#set par(leading:7pt)\n#bibliography(bytes({}), title:none, full:true, style:\"ieee\")",
        serde_json::to_string(data).unwrap()
    ));
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let blocks: Vec<_> = p.slides[0]
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 1, "labels must not be separate text boxes");
    let bibliography = blocks[0];
    assert_eq!(bibliography.role, "bibliography");
    assert_eq!(bibliography.paragraphs.len(), 2);
    for (i, paragraph) in bibliography.paragraphs.iter().enumerate() {
        let text: String = paragraph.runs.iter().map(|r| r.text.as_str()).collect();
        assert!(text.starts_with(&format!("[{}]\t", i + 1)), "{text}");
        assert!(text.len() > 20);
        assert!(paragraph.margin_left > 0.);
        assert!((paragraph.indent + paragraph.margin_left).abs() < 0.01);
        assert_eq!(paragraph.tab_stops.first(), Some(&0.));
    }
    assert!(
        bibliography
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .any(|r| r.hyperlink.as_deref() == Some("https://example.org/reference"))
    );
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    let paragraphs: Vec<_> = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "p")
        .collect();
    assert_eq!(paragraphs.len(), 2);
    for paragraph in paragraphs {
        let properties = paragraph
            .children()
            .find(|n| n.tag_name().name() == "pPr")
            .unwrap();
        let tab = properties
            .descendants()
            .find(|n| n.tag_name().name() == "tab")
            .unwrap();
        assert_eq!(tab.attribute("pos"), properties.attribute("marL"));
    }
}

#[test]
fn bibliography_continuations_do_not_repeat_labels() {
    let data: String = (1..=12).map(|i| format!(
        "work{i}:\n  type: Book\n  title: Reference {i} with a long title that wraps over multiple lines and continues as an editable paragraph\n  author: Example, Alice\n  date: 2025\n"
    )).collect();
    let p = compile(&format!(
        "#set text(size:12pt)\n#set page(width:230pt,height:160pt)\n#bibliography(bytes({}), title:none, full:true, style:\"ieee\")",
        serde_json::to_string(&data).unwrap()
    ));
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert!(p.slides.len() > 1);
    let blocks: Vec<_> = p
        .slides
        .iter()
        .flat_map(|s| &s.elements)
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    let mut labels = Vec::new();
    let mut continuations = 0;
    for block in blocks {
        assert_eq!(block.role, "bibliography");
        for paragraph in &block.paragraphs {
            let text: String = paragraph.runs.iter().map(|r| r.text.as_str()).collect();
            if let Some((label, _)) = text.split_once('\t') {
                labels.push(label.to_owned());
            } else {
                continuations += 1;
                assert_eq!(paragraph.indent, 0.);
            }
        }
    }
    assert_eq!(
        labels,
        (1..=12).map(|i| format!("[{i}]")).collect::<Vec<_>>()
    );
    assert!(
        continuations > 0,
        "fixture must split an entry across pages"
    );
    pptx::write(&p).unwrap();
}

#[test]
fn text_scripts_keep_native_baseline_and_font_size() {
    let p = compile(
        "Normal#super[1] text#footnote[Note].\n\nCustom#super(typographic:false, size:12pt, baseline:-5pt)[abc] H#sub(typographic:false, size:10pt, baseline:3pt)[2]O",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let runs: Vec<_> = p.slides[0]
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .flat_map(|t| &t.paragraphs)
        .flat_map(|p| &p.runs)
        .collect();
    let supers: Vec<_> = runs.iter().filter(|r| r.text == "1").collect();
    assert!(!supers.is_empty());
    assert!(
        supers
            .iter()
            .all(|r| r.style.baseline > 0. && r.style.size < 20.)
    );
    let custom = runs.iter().find(|r| r.text == "abc").unwrap();
    assert!((custom.style.baseline - 5.).abs() < 0.01);
    assert!((custom.style.size - 12.).abs() < 0.01);
    let sub = runs.iter().find(|r| r.text == "2").unwrap();
    assert!((sub.style.baseline + 3.).abs() < 0.01);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    let custom_run = d
        .descendants()
        .find(|n| n.tag_name().name() == "t" && n.text() == Some("abc"))
        .unwrap()
        .parent()
        .unwrap();
    let properties = custom_run
        .children()
        .find(|n| n.tag_name().name() == "rPr")
        .unwrap();
    assert_eq!(properties.attribute("sz"), Some("1800"));
    assert_eq!(properties.attribute("baseline"), Some("27778"));
    assert!(d.descendants().any(|n| {
        n.tag_name().name() == "rPr"
            && n.attribute("baseline")
                .is_some_and(|v| v.parse::<i32>().unwrap() > 0)
    }));
    assert!(d.descendants().any(|n| {
        n.tag_name().name() == "rPr"
            && n.attribute("baseline")
                .is_some_and(|v| v.parse::<i32>().unwrap() < 0)
    }));
}

#[test]
fn horizontal_layout_spaces_become_editable_tab_stops() {
    let p = compile("#block(width:250pt)[#h(1fr)Date#h(1fr)7 / 15#h(1fr)]");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let t = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(t.paragraphs.len(), 1);
    assert_eq!(t.paragraphs[0].tab_stops.len(), 1);
    assert!(t.paragraphs[0].runs.iter().any(|r| r.text.contains('\t')));
    assert_eq!(t.paragraphs[0].alignment, "l");
    assert!(xml(&p).contains("<a:tab "));
}

#[test]
fn intrinsic_titles_and_code_keep_source_line_boundaries() {
    let p = compile(
        "#align(center)[#block(width:auto)[A short title]]\n\n```html\n<table>\n  <tr><td>Text</td></tr>\n</table>\n```",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let blocks: Vec<_> = p.slides[0]
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 2);
    assert!(!blocks[0].wrap);
    assert!(!blocks[1].wrap);
    assert!(blocks[1].paragraphs[0].break_latin);
    assert!(blocks[1].font_scale.is_some_and(|v| v > 0.9 && v <= 1.0));
    assert!(xml(&p).contains("latinLnBrk=\"1\""));
    assert!(xml(&p).contains("<a:normAutofit"));
}

#[test]
fn raw_soft_wraps_match_realized_lines_without_splitting_text_boxes() {
    let (p, capture) = compile_with_capture(
        "#set text(size:11pt)\n#block(width:165pt)[\n```xml\n<math display=\"block\" xmlns=\"https://example.org/math\">\n  <mi>SomeLongIdentifierThatNeedsWrapping</mi>\n\n  <mo>=</mo>\n</math>\n```\n]",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let blocks: Vec<_> = p.slides[0]
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].paragraphs.len(), 1);
    assert!(!blocks[0].wrap);
    let mut lines: Vec<(f64, String)> = Vec::new();
    for leaf in &capture.pages[0] {
        if let FrameItem::Text(text) = &leaf.item {
            if lines
                .last()
                .is_none_or(|(y, _)| (y - leaf.position.1).abs() > 0.1)
            {
                lines.push((leaf.position.1, String::new()));
            }
            lines.last_mut().unwrap().1.push_str(&text.text);
        }
    }
    assert!(lines.len() > 5, "fixture must actually soft-wrap");
    let native: String = blocks[0].paragraphs[0]
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect();
    assert!(
        native.contains("\n\n"),
        "blank source line lost: {native:?}"
    );
    assert_eq!(
        native.lines().filter(|l| !l.is_empty()).collect::<Vec<_>>(),
        lines.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>()
    );
    assert!(
        blocks[0].paragraphs[0].runs.len() > 1,
        "syntax colours lost"
    );
    assert!(xml(&p).contains("wrap=\"none\""));
}

#[test]
fn inline_raw_keeps_normal_paragraph_reflow() {
    let p = compile(
        "#block(width:190pt)[`inline` code begins a longer prose paragraph that wraps naturally.]",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let t = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert!(t.wrap);
    assert!(t.font_scale.is_none());
    assert!(!t.paragraphs[0].runs.iter().any(|r| r.text.contains('\n')));
}

#[test]
fn list_spacing_matches_source_through_the_last_item() {
    for (leading, tight) in [(2, true), (14, true), (7, false)] {
        for marker in ["-", "+"] {
            let source = format!(
                "#set page(height:800pt)\n#set text(top-edge:0.8em,bottom-edge:-0.2em)\n#set par(leading:{leading}pt)\n#set list(tight:{tight},spacing:40pt)\n#set enum(tight:{tight},spacing:40pt)\n{marker} First\\\n  continuation\n{marker} Second\n{marker} Third\n{marker} Fourth\n{marker} Fifth\n{marker} Sixth"
            );
            let (p, capture) = compile_with_capture(&source);
            assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
            let list = p.slides[0]
                .elements
                .iter()
                .find_map(|e| match e {
                    Element::Text(t) if t.role == "list" => Some(t),
                    _ => None,
                })
                .unwrap();
            assert_eq!(list.paragraphs.len(), 6);
            assert!(list.paragraphs.iter().all(|p| p.bullet.is_some()));
            let baselines: Vec<_> = capture.pages[0]
                .iter()
                .filter_map(|leaf| {
                    (matches!(leaf.item, FrameItem::Text(_))
                        && capture.nearest(leaf, Kind::Label).is_some())
                    .then_some(leaf.position.1)
                })
                .collect();
            assert_eq!(baselines.len(), 6);
            let mut predicted = baselines[0];
            for (i, paragraph) in list.paragraphs.iter().enumerate() {
                assert!((paragraph.line_spacing - (20. + f64::from(leading))).abs() < 0.01);
                if i > 0 {
                    let previous = &list.paragraphs[i - 1];
                    let hard_breaks = previous
                        .runs
                        .iter()
                        .map(|r| r.text.matches('\n').count())
                        .sum::<usize>();
                    predicted += previous.line_spacing * hard_breaks as f64
                        + previous.space_after
                        + paragraph.space_before
                        + paragraph.line_spacing;
                }
                assert!(
                    (predicted - baselines[i]).abs() < 0.01,
                    "item {i}: source {}, native {predicted}",
                    baselines[i]
                );
            }
            xml(&p);
        }
    }
}

#[test]
fn fonts_are_embedded_whole_for_rendering_and_future_edits() {
    let p = compile("Normal *bold* _italic_ *_both_*\n\n`Code`");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert!(p.fonts.len() >= 5);
    let bytes = pptx::write(&p).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("ppt/presentation.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let d = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(d.root_element().attribute("embedTrueTypeFonts"), Some("1"));
    for f in d
        .descendants()
        .filter(|n| n.tag_name().name() == "embeddedFont")
    {
        let names: Vec<_> = f
            .children()
            .filter(|n| n.is_element())
            .map(|n| n.tag_name().name())
            .collect();
        let order = ["font", "regular", "bold", "italic", "boldItalic"];
        assert!(
            names
                .windows(2)
                .all(|pair| order.iter().position(|&n| n == pair[0])
                    < order.iter().position(|&n| n == pair[1]))
        );
    }
    let rels = zip_text(&mut zip, "ppt/_rels/presentation.xml.rels");
    let rels = roxmltree::Document::parse(&rels).unwrap();
    let fonts: Vec<_> = rels
        .descendants()
        .filter(|n| n.attribute("Type").is_some_and(|t| t.ends_with("/font")))
        .collect();
    assert_eq!(fonts.len(), p.fonts.len());
    for rel in fonts {
        let id = rel.attribute("Id").unwrap();
        assert!(d.descendants().any(|n| n.attribute((
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
            "id"
        )) == Some(id)));
        let path = format!("ppt/{}", rel.attribute("Target").unwrap());
        let mut data = Vec::new();
        zip.by_name(&path).unwrap().read_to_end(&mut data).unwrap();
        let uint = |at| u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(uint(0), data.len());
        assert_eq!(uint(8), 0x20001);
        assert_eq!(uint(12), 0); // not subsetted, compressed, or encrypted
        let font = &data[data.len() - uint(4)..];
        assert!(font.starts_with(&[0, 1, 0, 0]) || font.starts_with(b"OTTO"));
        assert!(uint(4) > 10000);
    }
}

#[test]
fn single_line_table_cells_do_not_grow_rows_from_rounding() {
    let p = compile(
        "#table(columns:(auto,65pt), inset:8pt, [AVATAR], [This text wraps over several lines])",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let t = p.slides[0]
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
    assert!(!t.cells[0].wrap);
    assert!(t.cells[1].wrap);
    let source = &t.cells[0].paragraphs[0].runs[0];
    assert!(source.advances.iter().any(|a| a.shaped < a.nominal));
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    for wrap in ["none", "square"] {
        assert!(
            d.descendants()
                .any(|n| n.tag_name().name() == "bodyPr" && n.attribute("wrap") == Some(wrap))
        );
    }
}

#[test]
fn matrices_vectors_and_cases_keep_editable_cells_and_delimiters() {
    let p = compile(
        "#set math.mat(delim: \"[\", align: right)\n$ mat(1, 2; 3, 4) + vec(a, b) $\n\n$ f(x) = cases(x^2 \"if\" x > 0, 0 \"otherwise\") $",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    let matrices: Vec<_> = d
        .descendants()
        .filter(|n| {
            n.tag_name().name() == "m"
                && n.tag_name().namespace()
                    == Some("http://schemas.openxmlformats.org/officeDocument/2006/math")
        })
        .collect();
    assert_eq!(matrices.len(), 3);
    for (matrix, columns) in matrices.iter().zip([2, 1, 1]) {
        let rows: Vec<_> = matrix
            .children()
            .filter(|n| n.tag_name().name() == "mr")
            .collect();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(
                row.children()
                    .filter(|n| n.tag_name().name() == "e")
                    .count(),
                columns
            );
        }
    }
    for (tag, value) in [("begChr", "["), ("mcJc", "right"), ("endChr", "")] {
        assert_math_property(&d, tag, value);
    }
}

#[test]
fn operators_and_math_styles_have_office_math_properties() {
    let p = compile(
        "$ sum_(i=1)^n i + integral_0^1 x dif x $\n\n$ bold(A) + upright(B) + bb(R) + cal(L) + arrow(v) $\n\n$ lim_(x -> 0) x $\n\nInline $sum_(i=1)^n i$.",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    for tag in ["nary", "limLow", "acc"] {
        assert!(
            d.descendants().any(|n| n.has_tag_name((MATH, tag))),
            "missing {tag}"
        );
    }
    for (tag, value) in [
        ("limLoc", "undOvr"),
        ("limLoc", "subSup"),
        ("sty", "bi"),
        ("sty", "p"),
        ("scr", "double-struck"),
        ("scr", "script"),
    ] {
        assert_math_property(&d, tag, value);
    }
}

const MATH: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
fn assert_math_property(d: &roxmltree::Document<'_>, tag: &str, value: &str) {
    assert!(
        d.descendants()
            .any(|n| n.has_tag_name((MATH, tag)) && n.attribute((MATH, "val")) == Some(value)),
        "missing {tag}={value}"
    );
}

fn resolve_part(source: &str, target: &str) -> String {
    let mut parts: Vec<_> = if target.starts_with('/') {
        vec![]
    } else {
        source.split('/').collect()
    };
    if !target.starts_with('/') {
        parts.pop();
    }
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

fn zip_text(zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str) -> String {
    let mut text = String::new();
    zip.by_name(name)
        .unwrap_or_else(|e| panic!("missing {name}: {e}"))
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[test]
fn notes_are_attached_to_their_pages_as_editable_body_placeholders() {
    let p = compile(
        "First\n#metadata((typptx: \"speaker-note\", text: \"日本語 & <notes>\\nSecond line\"))\n#metadata((typptx: \"speaker-note\", text: [Another note]))\n#pagebreak()\nNo note\n#pagebreak()\nThird\n#metadata((typptx: \"speaker-note\", text: \"Third page\"))",
    );
    assert_eq!(p.slides.len(), 3);
    assert_eq!(
        p.slides[0].notes.as_deref(),
        Some("日本語 & <notes>\nSecond line\n\nAnother note")
    );
    assert_eq!(p.slides[1].notes, None);
    assert_eq!(p.slides[2].notes.as_deref(), Some("Third page"));
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let mut note_paths = Vec::new();
    for slide in 1..=3 {
        let rels = zip_text(&mut zip, &format!("ppt/slides/_rels/slide{slide}.xml.rels"));
        let d = roxmltree::Document::parse(&rels).unwrap();
        let target = d
            .descendants()
            .find(|n| {
                n.attribute("Type")
                    .is_some_and(|t| t.ends_with("/notesSlide"))
            })
            .map(|n| n.attribute("Target").unwrap().to_owned());
        assert_eq!(target.is_some(), slide != 2);
        if let Some(target) = target {
            note_paths.push(target.trim_start_matches('/').replace("../", "ppt/"));
        }
    }
    assert_ne!(note_paths[0], note_paths[1]);
    let notes = zip_text(&mut zip, &note_paths[0]);
    let d = roxmltree::Document::parse(&notes).unwrap();
    assert!(
        d.descendants()
            .any(|n| n.tag_name().name() == "ph" && n.attribute("type") == Some("body"))
    );
    let text: Vec<_> = d
        .descendants()
        .filter(|n| n.tag_name().name() == "t")
        .filter_map(|n| n.text())
        .collect();
    assert_eq!(text, ["日本語 & <notes>", "Second line", "Another note"]);
    let name = note_paths[1].rsplit('/').next().unwrap();
    let notes = zip_text(&mut zip, &note_paths[1]);
    assert!(notes.contains("Third page"));
    let rels = zip_text(&mut zip, &format!("ppt/notesSlides/_rels/{name}.rels"));
    assert!(rels.contains("/slides/slide3.xml"));
    assert!(rels.contains("/notesMasters/notesMaster1.xml"));
    assert!(zip_text(&mut zip, "ppt/presentation.xml").contains("<p:notesMasterIdLst>"));
    let rels = zip_text(&mut zip, "ppt/notesMasters/_rels/notesMaster1.xml.rels");
    let d = roxmltree::Document::parse(&rels).unwrap();
    let target = d
        .descendants()
        .find(|n| n.attribute("Type").is_some_and(|s| s.ends_with("/theme")))
        .unwrap()
        .attribute("Target")
        .unwrap();
    let path = resolve_part("ppt/notesMasters/notesMaster1.xml", target);
    assert!(zip.by_name(&path).is_ok());
    let rels = zip_text(&mut zip, "ppt/slideMasters/_rels/slideMaster1.xml.rels");
    let master = roxmltree::Document::parse(&rels).unwrap();
    let target = master
        .descendants()
        .find(|n| n.attribute("Type").is_some_and(|s| s.ends_with("/theme")))
        .unwrap()
        .attribute("Target")
        .unwrap();
    assert_ne!(
        path,
        target.trim_start_matches('/').replace("../", "ppt/"),
        "PowerPoint needs a separate notes theme even when its colors match"
    );
}

#[test]
fn pdfpc_notes_use_physical_page_indices_without_duplication() {
    let p = compile(
        "First\n#metadata((pages: ((idx: 0, note: \"First note\"), (idx: 2, note: \"Third note\")))) <pdfpc-file>\n#pagebreak()\nSecond\n#pagebreak()\nThird",
    );
    assert_eq!(
        p.slides
            .iter()
            .map(|s| s.notes.as_deref())
            .collect::<Vec<_>>(),
        [Some("First note"), None, Some("Third note")]
    );
}

#[test]
fn table_spans_preserve_the_logical_grid_and_store_text_only_at_the_origin() {
    let p = compile(
        "#table(columns:(100pt, 120pt, 140pt), rows:(40pt, 50pt, 60pt), inset:6pt, table.cell(colspan:2,rowspan:2)[Merged], [C], [F], [G], [H], [I])",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let table = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.column_widths, [100., 120., 140.]);
    assert_eq!(table.row_heights, [40., 50., 60.]);
    assert_eq!(table.cells.len(), 6);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    let cells: Vec<_> = d
        .descendants()
        .filter(|n| n.tag_name().name() == "tc")
        .collect();
    assert_eq!(cells.len(), 9);
    assert_eq!(cells[0].attribute("rowSpan"), Some("2"));
    assert_eq!(cells[0].attribute("gridSpan"), Some("2"));
    assert_eq!(cells[1].attribute("rowSpan"), Some("2"));
    assert_eq!(cells[1].attribute("hMerge"), Some("1"));
    assert_eq!(cells[3].attribute("gridSpan"), Some("2"));
    assert_eq!(cells[3].attribute("vMerge"), Some("1"));
    assert_eq!(cells[4].attribute("hMerge"), Some("1"));
    assert_eq!(cells[4].attribute("vMerge"), Some("1"));
    for i in [1, 3, 4] {
        assert!(!cells[i].descendants().any(|n| n.tag_name().name() == "t"));
    }
    assert_eq!(
        cells
            .iter()
            .filter(|c| {
                c.descendants()
                    .filter(|n| n.tag_name().name() == "t")
                    .filter_map(|n| n.text())
                    .collect::<String>()
                    == "Merged"
            })
            .count(),
        1
    );
}

#[test]
fn table_cells_keep_paragraphs_nested_bullets_numbering_and_empty_cells() {
    let p = compile(
        "#table(columns:(300pt,100pt), inset:8pt, [Before\n\n- First\n  - Nested\n- Second\n\nAfter\n\n#enum(start:3)[Third][Fourth]\n\nEnd], [])",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let table = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    let paragraphs = &table.cells[0].paragraphs;
    let text: Vec<_> = paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(
        text,
        [
            "Before", "First", "Nested", "Second", "After", "Third", "Fourth", "End"
        ]
    );
    assert!(paragraphs[0].bullet.is_none());
    assert!(matches!(
        paragraphs[1].bullet,
        Some(Bullet::Character { .. })
    ));
    assert_eq!(paragraphs[2].level, 1);
    assert!(paragraphs[4].bullet.is_none());
    for paragraph in &paragraphs[5..7] {
        assert!(matches!(
            paragraph.bullet,
            Some(Bullet::Number { start: Some(3), .. })
        ));
    }
    assert!(paragraphs[7].bullet.is_none());
    assert!(table.cells[1].paragraphs.is_empty());
    let s = xml(&p);
    assert_eq!(s.matches("<a:buChar").count(), 3);
    assert_eq!(s.matches("<a:buAutoNum").count(), 2);
}

#[test]
fn hard_breaks_survive_generated_content_typography_and_inline_math() {
    let p = compile(
        r#"
#let generated = context [#text(fill: red)[generated]]
- A "quoted" #generated value\
  continuation with $x^2$\
  final line
- Before#footnote[Footnote body] text\
  after reference
"#,
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let list = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Text(t) if t.role == "list" => Some(t),
            _ => None,
        })
        .unwrap();
    let text: Vec<_> = list
        .paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect();
    assert_eq!(text.len(), 2);
    assert!(
        text[0].contains("generated value\ncontinuation"),
        "{text:?}"
    );
    assert!(text[0].contains("\nfinal line"), "{text:?}");
    assert!(text[1].contains("text\nafter reference"), "{text:?}");
    assert_eq!(
        text.iter().map(|s| s.matches('\n').count()).sum::<usize>(),
        3
    );
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        d.descendants()
            .filter(|n| n.tag_name().name() == "br"
                && !n.ancestors().any(|a| a.tag_name().name() == "Fallback"))
            .count(),
        3
    );
}

#[test]
fn multi_symbol_enum_pattern_uses_native_numbering_for_each_level() {
    let p = compile(
        "#set enum(numbering: \"1.a.\")\n+ First\n  + Nested A\n    + Deep A\n  + Nested B\n+ Second",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let list = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Text(t) if t.role == "list" => Some(t),
            _ => None,
        })
        .unwrap();
    let schemes: Vec<_> = list
        .paragraphs
        .iter()
        .map(|p| match p.bullet.as_ref().unwrap() {
            Bullet::Number { scheme, .. } => scheme.as_str(),
            _ => panic!("expected numbering"),
        })
        .collect();
    assert_eq!(
        schemes,
        [
            "arabicPeriod",
            "alphaLcPeriod",
            "alphaLcPeriod",
            "alphaLcPeriod",
            "arabicPeriod"
        ]
    );
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        d.descendants()
            .filter(|n| n.tag_name().name() == "buAutoNum"
                && n.parent().is_some_and(|p| p.tag_name().name() == "pPr"))
            .count(),
        5
    );
}

#[test]
fn booktabs_rules_and_cell_lists_become_native_table_borders() {
    let p = compile(
        "#table(columns:(180pt,180pt), stroke:none, inset:8pt, table.hline(stroke:2pt), [Header A], [Header B], table.hline(stroke:1pt), [- Item\n- More], [Value], table.hline(stroke:2pt))",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let table = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    for (row, width) in table.horizontal_borders.iter().zip([2., 1., 2.]) {
        assert!(row.iter().all(|s| s.as_ref().unwrap().width == width));
    }
    assert!(table.vertical_borders.iter().flatten().all(Option::is_none));
    let s = xml(&p);
    assert!(s.contains("<a:lnT w=\"25400\""));
    assert!(s.contains("<a:lnB w=\"12700\""));
    assert_eq!(s.matches("<a:buChar").count(), 2);
    assert_eq!(
        p.slides[0].elements.len(),
        1,
        "rules must belong to the table"
    );
}

#[test]
fn partial_rules_and_cell_specific_borders_keep_resolved_edges() {
    let p = compile(
        "#table(columns:(100pt,100pt,100pt), stroke:none, table.hline(start:1,end:3,stroke:2pt+red), [A], [B], [C], table.vline(x:1,stroke:(paint:blue,thickness:1pt,dash:\"dashed\")), table.cell(stroke:(bottom:3pt+green))[D], [E], [F])",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let table = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert!(table.horizontal_borders[0][0].is_none());
    assert_eq!(table.horizontal_borders[0][1].as_ref().unwrap().width, 2.);
    assert_eq!(table.horizontal_borders[2][0].as_ref().unwrap().width, 3.);
    assert!(table.horizontal_borders[2][1].is_none());
    assert!(
        !table.vertical_borders[0][1]
            .as_ref()
            .unwrap()
            .dash
            .is_empty()
    );
    assert!(xml(&p).contains("<a:custDash>"));
}

#[test]
fn table_text_metrics_do_not_add_default_leading_or_empty_line_height() {
    let p = compile("#table(columns:(100pt,100pt), inset:8pt, [], [Header], [Value], [Value])");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let t = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    let cell = &t.cells[1];
    assert_eq!(cell.inset, [8.; 4]);
    assert!(cell.text_inset[0] < cell.inset[0]);
    assert!(cell.text_inset[2] < cell.inset[2]);
    assert!(cell.paragraphs[0].line_spacing < cell.paragraphs[0].runs[0].style.size * 1.2);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    for (tag, attr, value) in [("endParaRPr", "sz", "100"), ("spcPts", "val", "0")] {
        assert!(
            d.descendants()
                .any(|n| n.tag_name().name() == tag && n.attribute(attr) == Some(value))
        );
    }

    // Large explicit leading must not increase the first line's baseline or
    // the native table's minimum height. Office accepts signed cell margins.
    let p = compile(
        "#set par(leading:20pt)\n#table(columns:250pt, inset:8pt, [\n- First\\\n  continuation\n- Second\n])",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let t = p.slides[0]
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert!(t.cells[0].text_inset[0] < 0.0);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(d.descendants().any(|n| n.tag_name().name() == "tcPr"
        && n.attribute("marT").is_some_and(|s| s.starts_with('-'))));
}

#[test]
fn image_fallback_requires_explicit_opt_in() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("fallback.typ");
    fs::write(&input, "#skew(ax:20deg)[Skewed text]").unwrap();
    let doc = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0;
    assert!(pptx::write(&lower::convert(&doc).unwrap()).is_err());
    let p = lower::convert_with_options(
        &doc,
        &lower::Options {
            allow_image_fallback: true,
            ..Default::default()
        },
    )
    .unwrap();
    let zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    assert!(zip.file_names().any(|name| name.ends_with(".svg")));
}

#[test]
fn equations_in_tables_have_a_compatible_native_equation_branch() {
    let p = compile("#table(columns:2, [Value], [$x^2$])");
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    let equation = d
        .descendants()
        .find(|n| n.tag_name().name() == "oMath")
        .unwrap();
    assert!(
        equation
            .ancestors()
            .any(|n| n.tag_name().name() == "Choice")
    );
    assert!(equation.ancestors().any(|n| n.tag_name().name() == "tc"));
}

#[test]
fn unsupported_native_export_never_succeeds_with_svg() {
    let p = compile("#rect(fill:gradient.radial(red.transparentize(80%),blue))");
    assert!(
        p.diagnostics
            .iter()
            .any(|d| d.code == "unsupported_element")
    );
    assert!(pptx::write(&p).is_err());
    assert!(
        !p.slides[0]
            .elements
            .iter()
            .any(|e| matches!(e, Element::Drawing { .. }))
    );
}
