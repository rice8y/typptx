use std::{
    fs,
    io::{Cursor, Read},
};
use typptx::{capture::Capture, ir::*, lower, pptx, world::CompilerWorld};
use typst::LibraryExt;
use typst::layout::FrameItem;

// Compile the same source with Typst's unmodified routines. Instrumentation
// must never change its shaping or wrapping while collecting source styles.
struct VanillaWorld {
    inner: CompilerWorld,
    library: typst::utils::LazyHash<typst::Library>,
}
impl typst::World for VanillaWorld {
    fn library(&self) -> &typst::utils::LazyHash<typst::Library> {
        &self.library
    }
    fn book(&self) -> &typst::utils::LazyHash<typst::text::FontBook> {
        self.inner.book()
    }
    fn main(&self) -> typst::syntax::FileId {
        self.inner.main()
    }
    fn source(&self, id: typst::syntax::FileId) -> typst::diag::FileResult<typst::syntax::Source> {
        self.inner.source(id)
    }
    fn file(
        &self,
        id: typst::syntax::FileId,
    ) -> typst::diag::FileResult<typst::foundations::Bytes> {
        self.inner.file(id)
    }
    fn font(&self, index: usize) -> Option<typst::text::Font> {
        self.inner.font(index)
    }
    fn today(
        &self,
        offset: Option<typst::foundations::Duration>,
    ) -> Option<typst::foundations::Datetime> {
        self.inner.today(offset)
    }
}

#[test]
fn collecting_text_styles_preserves_typsts_shaping_and_line_breaks() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("original.typ");
    fs::write(
        &input,
        r#"
        #set page(width:280pt,height:500pt,margin:20pt)
        #set text(font:"Libertinus Serif",size:20pt,tracking:1pt)
        HHHH HHHH HHHH HHHH HHHH HHHH HHHH

        office “quoted words” #text(tracking:-.5pt)[INNER WORDS] outer end

        #text(tracking:0pt)[é office #text(kerning:false)[AVATAR] AVATAR]

        #text(lang:"ja")[日本語とABC]

        #text(lang:"ar",dir:rtl,tracking:0pt)[مرحبا بالعالم]

        #for i in range(4) {[H]}

        #text("H")#text("H")#h(0pt)#text("H")
    "#,
    )
    .unwrap();
    let world = CompilerWorld::new(&input, None, &[], &[]).unwrap();
    let ours = world.compile().unwrap().0;
    // Typst intentionally omits routine function pointers from the library
    // hash. Without clearing its memoization, it can reuse our instrumented
    // frames for the vanilla compilation and hide instrumentation regressions.
    typst::comemo::evict(0);
    let vanilla = VanillaWorld {
        inner: world,
        library: typst::utils::LazyHash::new(typst::Library::default()),
    };
    let original = typst::compile::<typst_layout::PagedDocument>(&vanilla)
        .output
        .unwrap();
    let measurements = |doc: &typst_layout::PagedDocument| {
        Capture::new(doc)
            .pages
            .iter()
            .enumerate()
            .flat_map(|(page, leaves)| {
                leaves
                    .iter()
                    .flat_map(move |leaf| {
                        let mut measured = Vec::new();
                        if let FrameItem::Text(t) = &leaf.item {
                            let mut x = leaf.position.0;
                            for g in &t.glyphs {
                                measured.push((
                                    page,
                                    g.id,
                                    x,
                                    leaf.position.1,
                                    g.x_advance.at(t.size).to_pt(),
                                ));
                                x += g.x_advance.at(t.size).to_pt();
                            }
                        }
                        measured
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let source = measurements(&original);
    let instrumented = measurements(&ours);
    assert_eq!(source.len(), instrumented.len());
    for (before, after) in source.iter().zip(&instrumented) {
        assert_eq!((before.0, before.1), (after.0, after.1));
        near(after.2, before.2);
        near(after.3, before.3);
        near(after.4, before.4);
    }
}

#[test]
fn soft_wrap_keeps_source_tracking_before_trimmed_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("wrap.typ");
    fs::write(
        &input,
        r#"
        #set page(width:300pt,height:200pt,margin:20pt)
        #set text(font:"Libertinus Serif",size:20pt,kerning:false,tracking:1pt)
        #block(width:61.4pt)[HHHH HHHH]
    "#,
    )
    .unwrap();
    let world = CompilerWorld::new(&input, None, &[], &[]).unwrap();
    let instrumented = Capture::new(&world.compile().unwrap().0);
    typst::comemo::evict(0);
    let vanilla = VanillaWorld {
        inner: world,
        library: typst::utils::LazyHash::new(typst::Library::default()),
    };
    let original = Capture::new(
        &typst::compile::<typst_layout::PagedDocument>(&vanilla)
            .output
            .unwrap(),
    );
    for capture in [&original, &instrumented] {
        let texts: Vec<_> = capture
            .pages
            .iter()
            .flatten()
            .filter_map(|leaf| match &leaf.item {
                FrameItem::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["HHHH ", "HHHH"]
        );
        near(texts[0].width().to_pt(), 62.4);
        near(texts[0].glyphs[3].x_advance.at(texts[0].size).to_pt(), 15.6);
        near(texts[0].glyphs[4].x_advance.at(texts[0].size).to_pt(), 0.);
        near(texts[1].width().to_pt(), 61.4);
        near(texts[1].glyphs[3].x_advance.at(texts[1].size).to_pt(), 14.6);
    }
}

fn compile(source: &str) -> (Presentation, Capture) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("tracking.typ");
    fs::write(&input, format!(
        "#set page(width:600pt,height:450pt,margin:25pt)\n#set text(font:\"Libertinus Serif\",size:20pt,kerning:false)\n{source}"
    )).unwrap();
    let document = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0;
    let presentation = lower::convert(&document).unwrap();
    assert!(
        presentation.diagnostics.is_empty(),
        "{:?}",
        presentation.diagnostics
    );
    (presentation, Capture::new(&document))
}

fn runs(p: &Presentation) -> Vec<&Run> {
    p.slides
        .iter()
        .flat_map(|s| &s.elements)
        .flat_map(Element::walk)
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .flat_map(|t| &t.paragraphs)
        .flat_map(|p| &p.runs)
        .collect()
}

fn spacing(p: &Presentation, word: &str) -> f64 {
    let matches: Vec<_> = runs(p)
        .into_iter()
        .filter(|r| r.text.split_whitespace().any(|s| s == word))
        .collect();
    assert_eq!(matches.len(), 1, "{word}");
    matches[0].style.letter_spacing
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

fn slide_xml(p: &Presentation) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    assert!(!zip.file_names().any(|n| n.starts_with("ppt/media/")));
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[test]
fn explicit_tracking_is_distinct_from_shaped_advance_adjustments() {
    let (plain, plain_capture) = compile("HHHH");
    let (tracked, tracked_capture) = compile("#text(tracking:1pt)[HHHH]");
    let width = |c: &Capture| {
        c.pages
            .iter()
            .flatten()
            .filter_map(|leaf| match &leaf.item {
                FrameItem::Text(t) if t.text == "HHHH" => Some(t.width().to_pt()),
                _ => None,
            })
            .sum::<f64>()
    };
    // Typst applies tracking only between the four glyph clusters: three gaps.
    near(width(&tracked_capture) - width(&plain_capture), 3.);
    near(spacing(&plain, "HHHH"), 0.);
    near(spacing(&tracked, "HHHH"), 1.);
    let run = runs(&tracked)[0];
    near(
        run.advances.iter().map(|a| a.shaped * run.style.size).sum(),
        width(&tracked_capture),
    );
    // This must remain an editable paragraph, with native spacing properties.
    let xml = slide_xml(&tracked);
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        document
            .descendants()
            .filter(|n| n.tag_name().name() == "txBody")
            .count(),
        1
    );
    let mut source = run.advances.iter();
    let mut actual: f64 = 0.;
    let mut expected: f64 = 0.;
    for native in document
        .descendants()
        .filter(|n| n.tag_name().name() == "r")
    {
        let props = native
            .children()
            .find(|n| n.tag_name().name() == "rPr")
            .unwrap();
        let text = native
            .children()
            .find(|n| n.tag_name().name() == "t")
            .unwrap()
            .text()
            .unwrap();
        let size = props.attribute("sz").unwrap().parse::<f64>().unwrap() / 100.;
        let spacing = props.attribute("spc").unwrap().parse::<f64>().unwrap() / 100.;
        for character in text.chars() {
            let advance = source.next().unwrap();
            assert_eq!(character, advance.character);
            assert!(
                (actual - expected).abs() <= 0.0051,
                "glyph origin {actual} != {expected}"
            );
            actual += (advance.nominal * size * 8.).round() / 8. + spacing;
            expected += advance.shaped * run.style.size;
        }
    }
    assert!(source.next().is_none());
    assert!(
        (actual - expected).abs() <= 0.0051,
        "terminal advance {actual} != {expected}"
    );
}

#[test]
fn inline_tracking_overrides_restore_the_parent_style() {
    let (p, c) = compile(
        r#"
        #set text(tracking:1pt)
        OUTER #text(tracking:-.5pt)[INNER #text(tracking:0pt)[ZERO] INNEREND] OUTEREND

        #text(tracking:2pt)[LOCAL] NEXT
    "#,
    );
    for (word, expected) in [
        ("OUTER", 1.),
        ("INNER", -0.5),
        ("ZERO", 0.),
        ("INNEREND", -0.5),
        ("OUTEREND", 1.),
        ("LOCAL", 2.),
        ("NEXT", 1.),
    ] {
        near(spacing(&p, word), expected);
        let leaves: Vec<_> = c
            .pages
            .iter()
            .flatten()
            .filter(|l| matches!(&l.item, FrameItem::Text(t) if t.text.trim() == word))
            .collect();
        assert_eq!(leaves.len(), 1, "{word}");
        near(leaves[0].tracking, expected);
    }
}

#[test]
fn em_tracking_resolves_with_font_size_and_geometric_scale() {
    let (p, c) = compile(
        r#"
        #text(tracking:.1em)[NORMAL]

        #text(size:30pt,tracking:.1em)[LARGE]

        #scale(x:150%,y:150%,reflow:true)[#text(tracking:1pt)[SCALED]]

        #text(tracking:.1em)[BASE #super(typographic:false)[SUP] END]
    "#,
    );
    near(spacing(&p, "NORMAL"), 2.);
    near(spacing(&p, "LARGE"), 3.);
    // The native group carries the scale, so its child's font and tracking
    // stay in local coordinates rather than being scaled a second time.
    near(spacing(&p, "SCALED"), 1.);
    let group = p.slides.iter().flat_map(|s| &s.elements).flat_map(Element::walk)
        .find_map(|e| match e {
            Element::Group(g) if g.elements.iter().flat_map(Element::walk).any(|e| matches!(e, Element::Text(t) if t.paragraphs.iter().flat_map(|p| &p.runs).any(|r| r.text == "SCALED"))) => Some(g),
            _ => None,
        }).unwrap();
    near(
        spacing(&p, "SCALED") * group.bounds.width / group.content_bounds.width,
        1.5,
    );
    let sup = c
        .pages
        .iter()
        .flatten()
        .find(|l| matches!(&l.item, FrameItem::Text(t) if t.text == "SUP"))
        .unwrap();
    let FrameItem::Text(t) = &sup.item else {
        unreachable!()
    };
    near(sup.tracking, t.size.to_pt() * 0.1);
    near(spacing(&p, "SUP"), sup.tracking * sup.scale());
}

#[test]
fn ligatures_and_combining_clusters_keep_native_tracking_and_shaping() {
    let (p, _) = compile("#text(tracking:1pt)[office é]");
    let rs = runs(&p);
    assert_eq!(
        rs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "office é"
    );
    assert!(rs.iter().all(|r| r.style.letter_spacing == 1.));
    assert!(
        rs.iter().any(|r| r.advances.is_empty()),
        "complex glyph clusters must keep native shaping"
    );
    let xml = slide_xml(&p);
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        document
            .descendants()
            .filter(|n| n.tag_name().name() == "txBody")
            .count(),
        1
    );
    assert!(
        document
            .descendants()
            .any(|n| n.tag_name().name() == "rPr" && n.attribute("spc") == Some("100"))
    );
}
