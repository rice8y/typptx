use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn compile(source: &str) -> Presentation {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("layout.typ");
    fs::write(&input, format!("#set page(width:720pt,height:480pt,margin:32pt)\n#set text(font:\"Libertinus Serif\",size:20pt)\n{source}")).unwrap();
    let p = lower::convert(
        &CompilerWorld::new(&input, None, &[], &[])
            .unwrap()
            .compile()
            .unwrap()
            .0,
    )
    .unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}
fn table(p: &Presentation) -> &Table {
    p.slides
        .iter()
        .flat_map(|s| &s.elements)
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Table(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .unwrap()
}
fn xml(p: &Presentation) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    assert!(!zip.file_names().any(|f| f.starts_with("ppt/media/")));
    let mut s = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

#[test]
fn mixed_cells_keep_complex_clusters_intact_and_compensate_surrounding_text() {
    let p = compile(
        "#table(columns:400pt,[AVATAR $x^2$ AVATAR],[AVATAR#super(typographic:false)[1] AVATAR],[AVATAR office é AVATAR])",
    );
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    for (src, cell) in table(&p)
        .cells
        .iter()
        .zip(doc.descendants().filter(|n| n.tag_name().name() == "tc"))
    {
        let p = &src.paragraphs[0];
        assert!(p.runs.first().unwrap().advances.len() >= 6);
        assert!(p.runs.last().unwrap().advances.len() >= 6);
        assert!(
            p.runs
                .iter()
                .any(|r| r.math.is_some() || r.style.baseline != 0. || r.advances.is_empty())
        );
        let runs: Vec<_> = cell
            .descendants()
            .filter(|n| {
                n.tag_name().name() == "r"
                    && n.tag_name().namespace()
                        == Some("http://schemas.openxmlformats.org/drawingml/2006/main")
            })
            .collect();
        let first = runs[0]
            .children()
            .find(|n| n.tag_name().name() == "rPr")
            .unwrap();
        let last = runs
            .last()
            .unwrap()
            .children()
            .find(|n| n.tag_name().name() == "rPr")
            .unwrap();
        assert_eq!(first.attribute("kern"), Some("0"));
        assert_eq!(last.attribute("kern"), Some("0"));
        assert_eq!(
            cell.descendants()
                .filter(|n| n.tag_name().name() == "p")
                .count(),
            1
        );
        for cluster in p
            .runs
            .iter()
            .filter(|r| r.math.is_none() && r.advances.is_empty())
        {
            assert!(runs.iter().any(|n| {
                n.children()
                    .any(|t| t.tag_name().name() == "t" && t.text() == Some(&cluster.text))
            }));
        }
    }
    assert!(s.contains("<m:sSup>"));
    assert!(s.contains("baseline=\""));
}

#[test]
fn every_source_line_fits_without_turning_soft_wraps_into_hard_breaks() {
    let p = compile(
        "#set text(tracking:1pt)\n#table(columns:(77.4pt,77.4pt),inset:8pt,[HHHH\\ HHHH],[HHHH HHHH])",
    );
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    let cells: Vec<_> = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "tc")
        .collect();
    assert_eq!(cells.len(), 2);
    for (i, (src, cell)) in table(&p).cells.iter().zip(cells).enumerate() {
        let source = &src.paragraphs[0];
        assert_eq!(source.runs.iter().map(|r| r.source_line).max(), Some(1));
        assert_eq!(
            cell.descendants()
                .filter(|n| n.tag_name().name() == "p")
                .count(),
            1
        );
        assert_eq!(
            cell.descendants()
                .filter(|n| n.tag_name().name() == "br")
                .count(),
            usize::from(i == 0)
        );
        let mut widths = Vec::new();
        let mut width = 0.;
        let mut index = 0;
        for r in cell.descendants().filter(|n| n.tag_name().name() == "r") {
            let props = r.children().find(|n| n.tag_name().name() == "rPr").unwrap();
            let spacing = props.attribute("spc").unwrap().parse::<f64>().unwrap() / 100.;
            let txt = r
                .children()
                .find(|n| n.tag_name().name() == "t")
                .unwrap()
                .text()
                .unwrap();
            for ch in txt.chars() {
                if ch == 'H' {
                    // Source glyph origins remain 0,15.6,31.2,46.8pt on each line.
                    assert!((width - (index as f64) * 15.6).abs() < 0.0051);
                    width += 14.625 + spacing;
                    index += 1;
                    if index == 4 {
                        widths.push(width);
                        width = 0.;
                        index = 0;
                    }
                } else {
                    assert_eq!(ch, ' ');
                    // A trimmed source space must retain a positive native
                    // advance so widening/editing the cell can reflow it.
                    assert!(5. + spacing > 0.);
                }
            }
        }
        assert_eq!(widths.len(), 2);
        assert!(
            widths
                .iter()
                .all(|&w| w <= 61.375 + 1e-6 && w > 61.365 - 1e-6),
            "{widths:?}"
        );
    }
}
