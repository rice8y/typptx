//! Package versions and source markers are fixed in tests/fixtures.
use std::{
    collections::HashSet,
    io::{Cursor, Read},
    path::Path,
    sync::Mutex,
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

// Avoid redundant downloads and memory-heavy simultaneous showcase compiles.
static PACKAGES: Mutex<()> = Mutex::new(());

fn compile(name: &str, inputs: &[(&str, &str)]) -> Presentation {
    let _lock = PACKAGES.lock().unwrap_or_else(|e| e.into_inner());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let inputs: Vec<_> = inputs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let (doc, warnings) = CompilerWorld::new(&root.join(name), Some(&root), &[], &inputs)
        .unwrap()
        .compile()
        .unwrap();
    assert!(warnings.is_empty(), "{name}: {warnings:?}");
    let p = lower::convert_with_options(
        &doc,
        &lower::Options {
            animations: lower::AnimationFormat::Slides,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(p.diagnostics.is_empty(), "{name}: {:?}", p.diagnostics);
    for e in p
        .slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
    {
        assert!(
            !matches!(
                e,
                Element::Drawing { .. } | Element::MathSvg { .. } | Element::Picture { .. }
            ),
            "{name}: fallback"
        );
    }
    // This also runs the OOXML schema validation in the writer.
    let mut zip = zip::ZipArchive::new(Cursor::new(
        pptx::write(&p).unwrap_or_else(|e| panic!("{name}: {e:#}")),
    ))
    .unwrap();
    let expected_media: HashSet<Vec<u8>> = p
        .slides
        .iter()
        .flat_map(paragraphs)
        .filter_map(|p| match &p.bullet {
            Some(Bullet::Picture { bytes, svg, .. }) => Some(
                std::iter::once(bytes.clone()).chain(svg.iter().map(|s| s.as_bytes().to_vec())),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    let names: Vec<_> = zip
        .file_names()
        .filter(|n| n.starts_with("ppt/media/"))
        .map(str::to_owned)
        .collect();
    let actual_media: HashSet<Vec<u8>> = names
        .iter()
        .map(|name| {
            let mut bytes = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
            bytes
        })
        .collect();
    assert_eq!(
        actual_media, expected_media,
        "{name}: media must belong to native picture bullets"
    );
    for page in 1..=p.slides.len() {
        let mut xml = String::new();
        zip.by_name(&format!("ppt/slides/slide{page}.xml"))
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        let tree = roxmltree::Document::parse(&xml).unwrap();
        assert!(
            !tree.descendants().any(|n| n.tag_name().name() == "pic"),
            "{name}: standalone picture"
        );
        assert!(
            tree.descendants().any(|n| n.tag_name().name() == "txBody"),
            "{name}: page {page}"
        );
    }
    p
}

fn paragraphs(slide: &Slide) -> Vec<&Paragraph> {
    slide
        .elements
        .iter()
        .flat_map(Element::walk)
        .flat_map(|e| match e {
            Element::Text(t) => t.paragraphs.iter().collect::<Vec<_>>(),
            Element::Table(t) => t.cells.iter().flat_map(|c| &c.paragraphs).collect(),
            _ => vec![],
        })
        .collect()
}

fn text(slide: &Slide) -> String {
    paragraphs(slide)
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn markers(p: &Presentation, expected: &[&[&str]], universe: &[&str]) {
    assert_eq!(p.slides.len(), expected.len());
    for (page, (slide, expected)) in p.slides.iter().zip(expected).enumerate() {
        let content = text(slide);
        for marker in universe {
            assert_eq!(
                content.contains(marker),
                expected.contains(marker),
                "page {}: {marker}\n{content}",
                page + 1
            );
        }
    }
}

fn structure(package: &str) {
    let p = compile(&format!("{package}/structure.typ"), &[]);
    assert_eq!(p.slides.len(), 2);
    for s in &p.slides {
        assert!((s.width / s.height - 16. / 9.).abs() < 1e-6);
    }
    let first = &p.slides[0];
    let bullets: Vec<_> = paragraphs(first)
        .into_iter()
        .filter_map(|p| p.bullet.as_ref())
        .collect();
    // Diatypst uses a function that paints bold, colored number markers.
    // Those keep their appearance as picture bullets; plain numbering stays automatic.
    let custom_numbers = package == "diatypst";
    assert_eq!(
        bullets
            .iter()
            .filter(|b| matches!(b, Bullet::Character { .. }))
            .count(),
        3
    );
    assert_eq!(
        bullets
            .iter()
            .filter(|b| matches!(b, Bullet::Picture { .. }))
            .count(),
        if custom_numbers { 2 } else { 0 }
    );
    assert_eq!(
        bullets
            .iter()
            .filter(|b| matches!(b, Bullet::Number { .. }))
            .count(),
        if custom_numbers { 0 } else { 2 }
    );
    let tables: Vec<_> = first
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| match e {
            Element::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].column_widths.len(), 3);
    assert_eq!(tables[0].row_heights.len(), 2);
    assert!(tables[0].cells.iter().any(|c| c.column_span == 2));
    for marker in ["Parent marker", "Nested marker", "Merged marker", "Cell C"] {
        assert!(text(first).contains(marker));
    }
    let second = &p.slides[1];
    for marker in ["Left column marker", "Right column marker", "editable code"] {
        assert!(text(second).contains(marker));
    }
    let runs: Vec<_> = paragraphs(second)
        .into_iter()
        .flat_map(|p| &p.runs)
        .collect();
    assert!(runs.iter().any(|r| r.math.is_some()));
    assert!(
        runs.iter()
            .any(|r| r.hyperlink == Some(LinkTarget::Url("https://typst.app/".into())))
    );
}

#[test]
fn touying_native_structure() {
    structure("touying");
}
#[test]
fn polylux_native_structure() {
    structure("polylux");
}
#[test]
fn diatypst_native_structure() {
    structure("diatypst");
}

#[test]
fn touying_overlays_and_handout() {
    let universe = [
        "Alpha marker",
        "Beta marker",
        "Gamma marker",
        "Only first marker",
        "Later marker",
        "Alternative one",
        "Alternative two",
        "Parallel A",
        "Parallel B",
        "Parallel C",
        "Parallel D",
    ];
    let expected: &[&[&str]] = &[
        &["Alpha marker"],
        &["Alpha marker", "Beta marker"],
        &["Alpha marker", "Beta marker", "Gamma marker"],
        &["Only first marker", "Alternative one"],
        &["Later marker", "Alternative two"],
        &["Parallel A", "Parallel C"],
        &["Parallel A", "Parallel B", "Parallel C", "Parallel D"],
    ];
    markers(&compile("touying/overlays.typ", &[]), expected, &universe);
    markers(
        &compile("touying/overlays.typ", &[("handout", "true")]),
        &[expected[2], expected[4], expected[6]],
        &universe,
    );
}

#[test]
fn polylux_overlays_and_handout() {
    let universe = [
        "Always marker",
        "Only first marker",
        "Later marker",
        "Alpha marker",
        "Beta marker",
        "Gamma marker",
        "First list marker",
        "Second list marker",
    ];
    let expected: &[&[&str]] = &[
        &["Always marker", "Only first marker"],
        &["Always marker", "Later marker"],
        &["Alpha marker"],
        &["Alpha marker", "Beta marker"],
        &["Alpha marker", "Beta marker", "Gamma marker"],
        &["First list marker"],
        &["First list marker", "Second list marker"],
    ];
    markers(&compile("polylux/overlays.typ", &[]), expected, &universe);
    markers(
        &compile("polylux/overlays.typ", &[("handout", "true")]),
        &[
            &["Always marker", "Only first marker", "Later marker"],
            expected[4],
            expected[6],
        ],
        &universe,
    );
}

#[test]
fn polylux_code_reveal_preserves_text_and_cover_color() {
    let p = compile("polylux/code.typ", &[]);
    markers(
        &p,
        &[
            &["CODE_FIRST"],
            &["CODE_FIRST", "CODE_SECOND"],
            &["CODE_FIRST", "CODE_SECOND", "CODE_THIRD"],
        ],
        &["CODE_FIRST", "CODE_SECOND", "CODE_THIRD"],
    );
    let color = |i: usize| {
        paragraphs(&p.slides[i])
            .into_iter()
            .flat_map(|p| &p.runs)
            .find(|r| r.text.contains("CODE_FIRST"))
            .unwrap()
            .style
            .color
    };
    assert_ne!(color(0), color(1));
    assert_eq!(color(0), color(2));
}

#[test]
fn diatypst_navigation_and_themes() {
    for theme in ["normal", "full"] {
        let p = compile("diatypst/navigation.typ", &[("theme", theme)]);
        assert_eq!(p.slides.len(), 6);
        assert!(
            p.slides
                .iter()
                .all(|s| (s.width / s.height - 4. / 3.).abs() < 1e-6)
        );
        assert!(text(&p.slides[0]).contains("Navigation title"));
        let alpha = p
            .slides
            .iter()
            .position(|s| text(s).contains("Alpha body marker"))
            .unwrap();
        let beta = p
            .slides
            .iter()
            .position(|s| text(s).contains("Beta body marker"))
            .unwrap();
        assert!(alpha < beta);
        assert!(
            paragraphs(&p.slides[beta])
                .iter()
                .flat_map(|p| &p.runs)
                .any(|r| r.hyperlink == Some(LinkTarget::Slide(alpha + 1)))
        );
        assert!(
            p.slides
                .iter()
                .flat_map(paragraphs)
                .flat_map(|p| &p.runs)
                .filter_map(|r| r.hyperlink.as_ref())
                .all(|target| !matches!(target,LinkTarget::Slide(n) if *n==0 || *n>p.slides.len()))
        );
        assert!(text(&p.slides[beta]).contains("Navigation footer"));
    }
}

#[test]
fn package_showcases_remain_native() {
    for (package, pages) in [("touying", 31), ("polylux", 4), ("diatypst", 15)] {
        let p = compile(&format!("{package}/sample.typ"), &[]);
        assert_eq!(p.slides.len(), pages, "{package}");
        if package == "touying" {
            assert!(p.slides.iter().any(|s| {
                s.notes
                    .as_ref()
                    .is_some_and(|n| n.contains("This is a speaker note"))
            }));
            // Regression: nested headers/footers must not widen the body text.
            for page in [27, 28] {
                let body = p.slides[page]
                    .elements
                    .iter()
                    .filter_map(|e| match e {
                        Element::Text(t) if t.wrap => Some(t),
                        _ => None,
                    })
                    .max_by_key(|t| {
                        t.paragraphs
                            .iter()
                            .flat_map(|p| &p.runs)
                            .map(|r| r.text.len())
                            .sum::<usize>()
                    })
                    .unwrap();
                assert!(body.bounds.right() < p.slides[page].width - 45.);
            }
        }
    }
}
