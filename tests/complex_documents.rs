//! Cross-feature fixtures stay local and reproducible; private documents are opt-in.
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn convert(path: &Path, inputs: &[(String, String)]) -> Presentation {
    let world = CompilerWorld::new(path, None, &[], inputs).unwrap();
    let document = world.compile().unwrap().0;
    let mut p = lower::convert(&document).unwrap();
    world.locate_diagnostics(&mut p);
    assert!(
        p.diagnostics.is_empty(),
        "{}: {:?}",
        path.display(),
        p.diagnostics
    );
    assert_eq!(p.slides.len(), document.pages().len());
    p
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/complex")
        .join(name)
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
        .flat_map(|p| &p.runs)
        .map(|r| r.text.as_str())
        .collect()
}
fn native_package(p: &Presentation, pictures: usize) {
    let zip = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    // Each vector picture has an SVG asset and a PNG preview.
    assert_eq!(
        zip.file_names()
            .filter(|n| n.starts_with("ppt/media/"))
            .count(),
        pictures * 2
    );
    for element in p
        .slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
    {
        assert!(!matches!(element, Element::Drawing { .. }));
    }
}

#[test]
fn continued_tables_keep_decorations_gutters_headers_and_direction() {
    for rtl in [false, true] {
        let p = convert(
            &fixture("continued-table.typ"),
            &[("rtl".into(), rtl.to_string())],
        );
        assert!(p.slides.len() >= 4);
        let mut all = String::new();
        for slide in &p.slides {
            let ts: Vec<_> = slide
                .elements
                .iter()
                .flat_map(Element::walk)
                .filter_map(|e| match e {
                    Element::Table(t) => Some(t),
                    _ => None,
                })
                .collect();
            assert_eq!(ts.len(), 1);
            let table = ts[0];
            assert_eq!(
                table.column_widths,
                if rtl {
                    vec![352., 8., 72.]
                } else {
                    vec![72., 8., 352.]
                }
            );
            assert!(
                table
                    .row_heights
                    .iter()
                    .all(|h| (*h - 28.).abs() < 0.02 || (*h - 4.).abs() < 0.02)
            );
            let contents = text(slide);
            for label in [
                "Table header",
                "Footer left",
                "Footer middle",
                "Footer right",
                "Table footer",
            ] {
                assert_eq!(contents.matches(label).count(), 1, "{label}: {contents}");
            }
            all.push_str(&contents);
        }
        for i in 0..24 {
            assert_eq!(all.matches(&format!("record-{i}-end")).count(), 1);
        }
        native_package(&p, 0);
    }
}

#[test]
fn rich_cells_keep_grids_lists_equations_and_nested_tables_editable() {
    let p = convert(&fixture("rich-cells.typ"), &[]);
    assert_eq!(p.slides.len(), 1);
    let contents = text(&p.slides[0]);
    for label in [
        "GridAlpha",
        "GridBeta",
        "GridGamma",
        "ListAlpha",
        "ListNested",
        "ListBeta",
        "EnumAlpha",
        "EnumBeta",
        "BeforeNested",
        "InnerAlpha",
        "InnerBeta",
        "AfterNested",
        "StyledCell",
    ] {
        assert_eq!(contents.matches(label).count(), 1, "{label}");
    }
    let elements: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .collect();
    assert_eq!(
        elements
            .iter()
            .filter(|e| matches!(e, Element::Table(_)))
            .count(),
        2
    );
    assert!(elements.iter().any(|e| matches!(e, Element::Shape(_))));
    assert_eq!(
        elements
            .iter()
            .filter(|e| matches!(e, Element::Picture { svg: Some(_), .. }))
            .count(),
        1
    );
    let pars = paragraphs(&p.slides[0]);
    assert_eq!(pars.iter().filter(|p| p.bullet.is_some()).count(), 5);
    assert!(pars.iter().flat_map(|p| &p.runs).any(|r| r.math.is_some()));
    let inner = elements
        .iter()
        .find_map(|e| match e {
            Element::Table(t)
                if t.cells.iter().any(|c| {
                    c.paragraphs
                        .iter()
                        .any(|p| p.runs.iter().any(|r| r.text == "InnerAlpha"))
                }) =>
            {
                Some(t)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        inner
            .cells
            .iter()
            .flat_map(|c| &c.paragraphs)
            .flat_map(|p| &p.runs)
            .all(|r| r.style.letter_spacing == 0.0 && !r.advances.is_empty())
    );
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert!(
        doc.descendants()
            .any(|n| n.tag_name().name() == "t" && n.text() == Some("\u{200b}"))
    );
    assert!(
        pars.iter()
            .flat_map(|p| &p.runs)
            .filter(|r| r.math.is_some())
            .all(|r| r.math_inline)
    );
    native_package(&p, 1);
}

#[test]
fn continued_list_bodies_keep_nested_numbering_and_inline_styles() {
    let p = convert(&fixture("continued-list.typ"), &[]);
    assert!(p.slides.len() >= 4);
    let mut all = String::new();
    for slide in &p.slides {
        let contents = text(slide);
        assert_eq!(contents.matches("List header").count(), 1);
        assert_eq!(contents.matches("List footer").count(), 1);
        all.push_str(&contents);
    }
    for i in 0..18 {
        assert_eq!(all.matches(&format!("item-{i}-end")).count(), 1);
    }
    for label in ["NestedAlpha", "NestedBeta", "Details", "raised"] {
        assert_eq!(all.matches(label).count(), 18);
    }
    let pars: Vec<_> = p.slides.iter().flat_map(paragraphs).collect();
    assert_eq!(
        pars.iter()
            .filter(|p| matches!(p.bullet, Some(Bullet::Number { .. })))
            .count(),
        36
    );
    assert_eq!(
        pars.iter()
            .flat_map(|p| &p.runs)
            .filter(|r| r.hyperlink.is_some())
            .count(),
        18
    );
    assert!(
        pars.iter()
            .flat_map(|p| &p.runs)
            .filter(|r| r.text == "raised")
            .all(|r| r.style.baseline > 0.)
    );
    native_package(&p, 0);
}

#[test]
#[ignore = "set TYPPTX_CORPUS_FILES to local document paths, separated as PATH entries"]
fn private_documents_export_without_conversion_diagnostics() {
    let files = std::env::var_os("TYPPTX_CORPUS_FILES").expect("set TYPPTX_CORPUS_FILES");
    let paths: Vec<_> = std::env::split_paths(&files).collect();
    assert!(!paths.is_empty());
    for path in paths {
        let p = convert(&path, &[]);
        pptx::write(&p).unwrap();
        assert!(
            p.slides
                .iter()
                .flat_map(|s| s.elements.iter().flat_map(Element::walk))
                .all(|e| !matches!(e, Element::Drawing { .. }))
        );
        eprintln!(
            "{}: {} slides, zero diagnostics/fallbacks",
            path.display(),
            p.slides.len()
        );
    }
}
