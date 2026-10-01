//! Imported asset metadata and shared package parts survive separate placements.
use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn compile(body: &str) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("icon.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="blue"/></svg>"#).unwrap();
    image::RgbImage::from_pixel(40, 20, image::Rgb([30, 90, 150]))
        .save(dir.path().join("icon.png"))
        .unwrap();
    let input = dir.path().join("main.typ");
    fs::write(&input, format!("#set page(width:400pt,height:300pt,margin:20pt)\n#set text(font:\"Libertinus Serif\")\n{body}")).unwrap();
    let (doc, warnings) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}
fn xml(zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str) -> String {
    let mut out = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
    out
}

#[test]
fn shapes_images_and_text_keep_external_links_without_duplicate_relationships() {
    let p = compile(
        r#"
#link("https://example.com/?a=1&b=2")[#rect(width:100pt,height:30pt,fill:red)]
#link("https://example.com/?a=1&b=2")[#image("icon.svg",width:40pt)]
#link("https://example.com/?a=1&b=2")[Text link]
#link(<target>)[Next slide]
#pagebreak()
Target <target>
"#,
    );
    assert!(
        p.slides[0]
            .links
            .iter()
            .any(|l| l.target == LinkTarget::Slide(2))
    );
    assert_eq!(
        p.slides[0]
            .links
            .iter()
            .filter(|l| matches!(l.target, LinkTarget::Url(_)))
            .count(),
        3
    );
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let rels = xml(&mut zip, "ppt/slides/_rels/slide1.xml.rels");
    let d = roxmltree::Document::parse(&rels).unwrap();
    let external: Vec<_> = d
        .descendants()
        .filter(|n| n.attribute("TargetMode") == Some("External"))
        .collect();
    assert_eq!(external.len(), 1);
    assert_eq!(
        external[0].attribute("Target"),
        Some("https://example.com/?a=1&b=2")
    );
    let id = external[0].attribute("Id").unwrap();
    let slide = xml(&mut zip, "ppt/slides/slide1.xml");
    let d = roxmltree::Document::parse(&slide).unwrap();
    let links: Vec<_> = d
        .descendants()
        .filter(|n| n.tag_name().name() == "hlinkClick")
        .collect();
    assert_eq!(
        links
            .iter()
            .filter(|n| n.attribute((
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                "id"
            )) == Some(id))
            .count(),
        4
    );
    assert!(
        links
            .iter()
            .any(|n| n.attribute("action") == Some("ppaction://hlinksldjump"))
    );
}

#[test]
fn clipped_rotated_links_keep_the_click_region_and_hidden_links_disappear() {
    let p = compile(
        r#"
#place(top+left)[#box(width:60pt,height:30pt,clip:true)[#link("https://example.com/crop")[#image("icon.svg",width:120pt)]]]
#place(top+left,dx:160pt)[#rotate(30deg)[#link("https://example.com/rotate")[#image("icon.svg",width:40pt)]]]
#hide[#link("https://example.com/hidden")[#image("icon.svg",width:40pt)]]
"#,
    );
    assert_eq!(p.slides[0].links.len(), 2);
    let crop = p.slides[0]
        .links
        .iter()
        .find(|l| l.target == LinkTarget::Url("https://example.com/crop".into()))
        .unwrap();
    assert!((crop.region.bounds.width - 60.).abs() < 0.01);
    assert!((crop.region.bounds.height - 30.).abs() < 0.01);
    let rotated = p.slides[0]
        .links
        .iter()
        .find(|l| l.target == LinkTarget::Url("https://example.com/rotate".into()))
        .unwrap();
    let points: Vec<_> = rotated
        .region
        .commands
        .iter()
        .filter_map(|c| match c {
            PathCommand::Move(p) | PathCommand::Line(p) => Some(p),
            _ => None,
        })
        .collect();
    assert!(
        points
            .windows(2)
            .any(|p| (p[0][0] - p[1][0]).abs() > 1. && (p[0][1] - p[1][1]).abs() > 1.)
    );
    pptx::write(&p).unwrap();
}

#[test]
fn shared_svg_and_raster_parts_keep_distinct_descriptions_and_placements() {
    let p = compile(
        r#"
#image("icon.svg",width:40pt,alt:"First vector & description")
#image("icon.svg",width:40pt,alt:"Second vector")
#image("icon.png",width:40pt,alt:"First raster")
#pagebreak()
#image("icon.svg",width:40pt,alt:"Third vector")
#image("icon.png",width:40pt,alt:"Second raster")
"#,
    );
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    assert_eq!(
        zip.file_names()
            .filter(|n| n.starts_with("ppt/media/"))
            .count(),
        3
    );
    let mut descriptions = Vec::new();
    let mut targets = Vec::new();
    for page in 1..=2 {
        let slide = xml(&mut zip, &format!("ppt/slides/slide{page}.xml"));
        let d = roxmltree::Document::parse(&slide).unwrap();
        descriptions.extend(
            d.descendants()
                .filter(|n| n.tag_name().name() == "cNvPr")
                .filter_map(|n| n.attribute("descr"))
                .map(str::to_owned),
        );
        let rels = xml(&mut zip, &format!("ppt/slides/_rels/slide{page}.xml.rels"));
        let r = roxmltree::Document::parse(&rels).unwrap();
        targets.push(
            r.descendants()
                .filter(|n| n.attribute("Type").is_some_and(|t| t.ends_with("/image")))
                .map(|n| n.attribute("Target").unwrap().to_owned())
                .collect::<std::collections::BTreeSet<_>>(),
        );
    }
    assert_eq!(
        descriptions,
        [
            "First vector & description",
            "Second vector",
            "First raster",
            "Third vector",
            "Second raster"
        ]
    );
    assert_eq!(targets[0], targets[1]);
}

#[test]
fn different_previews_share_svg_without_substituting_image_data() {
    let p = compile(
        r#"
#image("icon.svg",width:40pt)
#image("icon.svg",width:80pt)
#image("icon.png",width:40pt)
"#,
    );
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let names: Vec<_> = zip
        .file_names()
        .filter(|n| n.starts_with("ppt/media/"))
        .map(str::to_owned)
        .collect();
    assert_eq!(names.iter().filter(|n| n.ends_with(".svg")).count(), 1);
    let mut sizes = Vec::new();
    for name in names.iter().filter(|n| n.ends_with(".png")) {
        let mut bytes = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
        let png = image::load_from_memory(&bytes).unwrap();
        sizes.push((png.width(), png.height()));
    }
    sizes.sort();
    assert_eq!(sizes, [(40, 20), (80, 40), (160, 80)]);
}
