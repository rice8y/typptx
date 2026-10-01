use std::fs;
use typptx::{capture::Capture, graphics, world::CompilerWorld};
use typst::{LibraryExt, layout::FrameItem};

struct VanillaWorld<'a> {
    inner: &'a CompilerWorld,
    library: typst::utils::LazyHash<typst::Library>,
}
impl typst::World for VanillaWorld<'_> {
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
fn math_source_layout_matches_vanilla_glyphs_paths_and_page_breaks() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("geometry.typ");
    fs::write(
        &input,
        r#"
        #set page(width:320pt,height:400pt,margin:24pt)
        #set text(size:20pt)
        $ frac(a+b,c+d) + sqrt(x) + cancel(y,angle:#32deg,cross:#true) $

        $ mat(1,frac(a,b);sqrt(x),mat(1;2);augment:#(hline:1,vline:1)) $

        $ attach(sum_(i=1)^n i,tl:a,bl:b) + underbrace(x+y,z) $

        $ x &= 1 \\ alpha + beta &= 2 $

        Before $upright("office") + a b$ after.

        #table(columns:2, [$frac(1,2)$], [$cancel(x,angle:#20deg)$])
        #let make(f,..args) = f(..args)
        $ make(#math.cancel,frac(a,b),angle:#35deg) + make(#math.frac,c,d) $
    "#,
    )
    .unwrap();
    let world = CompilerWorld::new(&input, None, &[], &[]).unwrap();
    let ours = world.compile().unwrap().0;
    // Routines intentionally omit function pointers from their hash. Clear
    // memoized layouts before changing compilers or this comparison can reuse
    // the instrumented result and miss geometry changes introduced by tags.
    typst::comemo::evict(0);
    let vanilla = VanillaWorld {
        inner: &world,
        library: typst::utils::LazyHash::new(typst::Library::default()),
    };
    let original = typst::compile::<typst_layout::PagedDocument>(&vanilla)
        .output
        .unwrap();
    fn measure(document: &typst_layout::PagedDocument) -> serde_json::Value {
        let capture = Capture::new(document);
        serde_json::Value::Array(capture.pages.iter().map(|leaves| {
            let mut marks = Vec::new();
            for leaf in leaves {
                match &leaf.item {
                    FrameItem::Text(text) => {
                        let mut advance = 0.;
                        for glyph in &text.glyphs {
                            let x = advance + glyph.x_offset.at(text.size).to_pt();
                            let y = -glyph.y_offset.at(text.size).to_pt();
                            marks.push(serde_json::json!({
                                "glyph":glyph.id,"size":text.size.to_pt(),
                                "position":[leaf.position.0+leaf.transform.sx.get()*x+leaf.transform.kx.get()*y,
                                    leaf.position.1+leaf.transform.ky.get()*x+leaf.transform.sy.get()*y],
                                "advance":glyph.x_advance.at(text.size).to_pt(),
                            }));
                            advance += glyph.x_advance.at(text.size).to_pt();
                        }
                    }
                    FrameItem::Shape(..) => marks.push(serde_json::to_value(graphics::shape(leaf).unwrap()).unwrap()),
                    _ => {}
                }
            }
            serde_json::Value::Array(marks)
        }).collect())
    }
    fn compare(a: &serde_json::Value, b: &serde_json::Value, path: String) {
        match (a, b) {
            (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
                assert!(
                    (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9,
                    "{path}: {a} != {b}"
                );
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{path}: different object count");
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    compare(a, b, format!("{path}[{i}]"));
                }
            }
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                assert_eq!(
                    a.keys().collect::<Vec<_>>(),
                    b.keys().collect::<Vec<_>>(),
                    "{path}"
                );
                for (key, a) in a {
                    compare(a, &b[key], format!("{path}.{key}"));
                }
            }
            _ => assert_eq!(a, b, "{path}"),
        }
    }
    compare(&measure(&ours), &measure(&original), "pages".into());
    typst::comemo::evict(0);
}
