use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};
fn compile(source: &str) -> Presentation {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("transform.typ");
    fs::write(&p,format!("#set page(width:600pt,height:400pt,margin:30pt)\n#set text(font:\"Libertinus Serif\",size:20pt)\n{source}")).unwrap();
    let (doc, _) = CompilerWorld::new(&p, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let bytes = pptx::write(&p).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("<p:grpSp>"));
    assert!(!z.file_names().any(|n| n.starts_with("ppt/media/")));
    p
}
#[test]
fn rotated_and_mirrored_paragraphs_stay_whole() {
    for src in [
        "#rotate(25deg)[A whole paragraph]",
        "#scale(x:-100%,y:100%)[A whole paragraph]",
    ] {
        let p = compile(src);
        let text: Vec<_> = p.slides[0]
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
        assert_eq!(text.len(), 1);
        assert_eq!(
            text[0].paragraphs[0]
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "A whole paragraph"
        );
    }
}
#[test]
fn rotated_lists_preserve_semantic_objects() {
    let p = compile("#rotate(-10deg)[\n- First\n- Second\n]");
    assert!(p.slides[0].elements.iter().flat_map(Element::walk).any(|e|matches!(e,Element::Text(t) if t.paragraphs.iter().filter(|p|p.bullet.is_some()).count()==2)));
}
