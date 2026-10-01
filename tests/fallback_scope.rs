//! Opting into fallbacks must not replace supported neighboring objects.
use std::fs;
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn convert(body: &str, allow: bool) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("icon.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="blue"/></svg>"#).unwrap();
    let source = dir.path().join("main.typ");
    fs::write(&source, format!("#set page(width:400pt,height:400pt,margin:20pt)\n#set text(font:\"Libertinus Serif\")\n{body}")).unwrap();
    let doc = CompilerWorld::new(&source, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0;
    lower::convert_with_options(
        &doc,
        &lower::Options {
            allow_image_fallback: allow,
            ..Default::default()
        },
    )
    .unwrap()
}
fn objects(p: &Presentation) -> Vec<&Element> {
    p.slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
        .filter(|e| !matches!(e, Element::Group(_)))
        .collect()
}
#[test]
fn enabling_fallback_preserves_native_shapes_pictures_alt_and_links() {
    let body = r#"
#rect(width:100pt,height:40pt,fill:blue)
#circle(radius:20pt,fill:red)
#link("https://example.com/")[#rotate(20deg)[#image("icon.svg",width:80pt,alt:"Rotated icon")]]
"#;
    let original = convert(body, false);
    let fallback = convert(body, true);
    assert!(fallback.diagnostics.is_empty());
    assert_eq!(
        serde_json::to_value(&original.slides).unwrap(),
        serde_json::to_value(&fallback.slides).unwrap()
    );
    assert_eq!(
        objects(&fallback)
            .iter()
            .filter(|e| matches!(e, Element::Shape(_)))
            .count(),
        2
    );
    assert!(
        objects(&fallback)
            .iter()
            .any(|e| matches!(e,Element::Picture {alt:Some(alt),..} if alt=="Rotated icon"))
    );
    pptx::write(&fallback).unwrap();
}
#[test]
fn only_failed_structures_are_drawings_and_each_failure_is_reported() {
    let p = convert(
        r#"
#rect(width:100pt,height:20pt,fill:blue)
- Editable body #rect(width:8pt,height:8pt,fill:red)
#image("icon.svg",width:40pt,alt:"Keep description")
#table(columns:1,[Editable cell])
#skew(ax:20deg)[Unsupported shear]
#circle(radius:15pt,fill:red)
"#,
        true,
    );
    assert_eq!(p.diagnostics.len(), 1, "{:?}", p.diagnostics);
    assert!(p.diagnostics.iter().all(|d| d.code == "drawing_fallback"));
    let objects = objects(&p);
    assert_eq!(
        objects
            .iter()
            .filter(|e| matches!(e, Element::Drawing { .. }))
            .count(),
        1
    );
    assert_eq!(
        objects
            .iter()
            .filter(|e| matches!(e, Element::Shape(_)))
            .count(),
        3
    );
    assert_eq!(
        objects
            .iter()
            .filter(|e| matches!(e, Element::Table(_)))
            .count(),
        1
    );
    assert!(
        objects
            .iter()
            .any(|e| matches!(e,Element::Picture {alt:Some(alt),..} if alt=="Keep description"))
    );
    for e in objects {
        if let Element::Drawing { bounds, .. } = e {
            assert!(bounds.height < 100.);
        }
    }
    pptx::write(&p).unwrap();
}
#[test]
fn unsupported_background_falls_back_without_absorbing_text() {
    let p = convert(
        "#set page(fill:gradient.radial(red.transparentize(80%),blue))\nEditable heading",
        true,
    );
    assert_eq!(p.diagnostics.len(), 1);
    assert_eq!(p.diagnostics[0].code, "drawing_fallback");
    assert!(matches!(p.slides[0].elements[0], Element::Drawing { .. }));
    assert!(objects(&p).iter().any(|e| matches!(e, Element::Text(_))));
    pptx::write(&p).unwrap();
}
