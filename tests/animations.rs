//! Overlay states must retain the same native objects, geometry and stacking
//! order as static export, while OOXML targets remain valid after grouping.
use std::{
    collections::HashSet,
    io::{Cursor, Read},
    path::Path,
    sync::Mutex,
};
use typptx::{
    ir::*,
    lower::{self, AnimationFormat, Options},
    pptx,
    world::CompilerWorld,
};

static PACKAGES: Mutex<()> = Mutex::new(());

fn pair(name: &str, inputs: &[(&str, &str)]) -> (Presentation, Presentation) {
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
    let animated = lower::convert(&doc).unwrap();
    let static_pages = lower::convert_with_options(
        &doc,
        &Options {
            animations: AnimationFormat::Slides,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        animated.diagnostics.is_empty(),
        "{:?}",
        animated.diagnostics
    );
    assert!(
        static_pages.diagnostics.is_empty(),
        "{:?}",
        static_pages.diagnostics
    );
    (animated, static_pages)
}

fn normalize(element: &mut Element) {
    match element {
        Element::Text(t) => t.source_id.clear(),
        Element::Table(t) => t.source_id.clear(),
        Element::MathSvg { source_id, .. } => source_id.clear(),
        Element::Linked { element, .. } => normalize(element),
        Element::Group(g) => g.elements.iter_mut().for_each(normalize),
        _ => {}
    }
}

fn assert_states(animated: &Presentation, static_pages: &Presentation) {
    let mut pages = 0;
    for slide in &animated.slides {
        if let Some(animation) = &slide.animation {
            assert_eq!(animation.steps.len(), animation.source_pages.len());
            for (step, &page) in animation.steps.iter().zip(&animation.source_pages) {
                assert_eq!(page, pages + 1);
                let mut actual: Vec<_> = step
                    .visible
                    .iter()
                    .map(|&i| slide.elements[i].clone())
                    .collect();
                let mut expected = static_pages.slides[pages].elements.clone();
                actual.iter_mut().for_each(normalize);
                expected.iter_mut().for_each(normalize);
                assert_eq!(actual, expected, "source page {page}");
                pages += 1;
            }
        } else {
            pages += 1;
        }
    }
    assert_eq!(pages, static_pages.slides.len());
}

fn assert_timing(presentation: &Presentation) {
    let mut package =
        zip::ZipArchive::new(Cursor::new(pptx::write(presentation).unwrap())).unwrap();
    for (i, slide) in presentation.slides.iter().enumerate() {
        let mut xml = String::new();
        package
            .by_name(&format!("ppt/slides/slide{}.xml", i + 1))
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        let tree = roxmltree::Document::parse(&xml).unwrap();
        let timing = tree.descendants().find(|n| n.tag_name().name() == "timing");
        let Some(animation) = &slide.animation else {
            assert!(timing.is_none());
            continue;
        };
        let timing = timing.unwrap();
        let shape_ids: HashSet<_> = tree
            .descendants()
            .filter(|n| n.tag_name().name() == "cNvPr")
            .filter_map(|n| n.attribute("id"))
            .collect();
        let mut time_ids = HashSet::new();
        for node in timing.descendants() {
            if node.tag_name().name() == "spTgt" {
                assert!(shape_ids.contains(node.attribute("spid").unwrap()));
            }
            if node.tag_name().name() == "cTn" {
                assert!(time_ids.insert(node.attribute("id").unwrap()));
            }
        }
        let clicks = timing
            .descendants()
            .filter(|n| n.attribute("delay") == Some("indefinite"))
            .count();
        assert_eq!(clicks, animation.steps.len() - 1);
        assert_eq!(
            timing
                .descendants()
                .filter(|n| n.attribute("nodeType") == Some("clickEffect"))
                .count(),
            clicks
        );
        assert!(
            timing
                .descendants()
                .any(|n| n.attribute("nodeType") == Some("mainSeq"))
        );
    }
}

#[test]
fn touying_pause_meanwhile_only_and_alternatives_become_clicks() {
    let (animated, static_pages) = pair("touying/overlays.typ", &[]);
    assert_eq!(animated.slides.len(), 3);
    assert_eq!(
        animated
            .slides
            .iter()
            .map(|s| s.animation.as_ref().unwrap().steps.len())
            .collect::<Vec<_>>(),
        [3, 2, 2]
    );
    assert_states(&animated, &static_pages);
    assert_timing(&animated);
}

#[test]
fn touying_animations_do_not_require_pdfpc_export() {
    let (animated, static_pages) = pair("touying/overlays.typ", &[("pdfpc", "false")]);
    assert_eq!(animated.slides.len(), 3);
    assert_states(&animated, &static_pages);
    assert_timing(&animated);
}

#[test]
fn touying_overflow_pages_keep_the_logical_slide_boundary() {
    let (animated, static_pages) = pair("touying/overflow.typ", &[]);
    assert_eq!(animated.slides.len(), 2);
    assert!(static_pages.slides.len() > 3);
    assert_eq!(
        animated.slides[0].animation.as_ref().unwrap().steps.len(),
        static_pages.slides.len() - 1
    );
    assert_states(&animated, &static_pages);
    assert_timing(&animated);
}

#[test]
fn full_package_showcases_have_native_timing() {
    for (name, slides, source_pages) in
        [("touying/sample.typ", 20, 31), ("polylux/sample.typ", 3, 4)]
    {
        let (animated, static_pages) = pair(name, &[]);
        assert_eq!(animated.slides.len(), slides);
        assert_eq!(static_pages.slides.len(), source_pages);
        assert_eq!(
            animated
                .slides
                .iter()
                .map(|s| s.animation.as_ref().map_or(1, |a| a.steps.len()))
                .sum::<usize>(),
            source_pages
        );
        assert!(
            animated
                .slides
                .iter()
                .flat_map(|s| &s.elements)
                .flat_map(Element::walk)
                .all(|e| !matches!(
                    e,
                    Element::Drawing { .. } | Element::Picture { .. } | Element::MathSvg { .. }
                ))
        );
        assert_timing(&animated);
    }
}

#[test]
fn polylux_uncover_only_and_item_by_item_become_clicks() {
    let (animated, static_pages) = pair("polylux/overlays.typ", &[]);
    assert_eq!(animated.slides.len(), 3);
    assert_eq!(
        animated
            .slides
            .iter()
            .map(|s| s.animation.as_ref().unwrap().steps.len())
            .collect::<Vec<_>>(),
        [2, 3, 2]
    );
    assert_states(&animated, &static_pages);
    assert_timing(&animated);
}

#[test]
fn code_cover_colors_and_repeated_visibility_keep_each_state() {
    let (animated, static_pages) = pair("polylux/code.typ", &[]);
    assert_eq!(animated.slides.len(), 1);
    assert_states(&animated, &static_pages);
    assert_timing(&animated);
}

#[test]
fn tables_math_and_shapes_animate_as_native_objects() {
    let (animated, static_pages) = pair("powerpoint/animations.typ", &[]);
    assert_eq!(animated.slides.len(), 3);
    assert_states(&animated, &static_pages);
    assert_timing(&animated);
    let slide = &animated.slides[0];
    let states = &slide.animation.as_ref().unwrap().steps;
    // The transient paragraph reuses the same editable object when it returns.
    assert!(
        states[0]
            .visible
            .iter()
            .any(|i| !states[1].visible.contains(i) && states[2].visible.contains(i))
    );
    assert!(
        animated.slides[1]
            .elements
            .iter()
            .any(|e| matches!(e, Element::Table(_)))
    );
}

#[test]
fn handouts_and_diatypst_remain_static() {
    for name in [
        "touying/overlays.typ",
        "polylux/overlays.typ",
        "diatypst/navigation.typ",
    ] {
        let (animated, static_pages) = pair(name, &[("handout", "true")]);
        assert_eq!(animated.slides.len(), static_pages.slides.len());
        assert!(animated.slides.iter().all(|s| s.animation.is_none()));
        assert_timing(&animated);
    }
}

#[test]
fn empty_clicks_links_and_notes_survive_slide_coalescing() {
    let (animated, static_pages) = pair("polylux/animation-cases.typ", &[]);
    assert_eq!(animated.slides.len(), 3);
    assert_eq!(static_pages.slides.len(), 6);
    let states = &animated.slides[0].animation.as_ref().unwrap().steps;
    assert_eq!(states.len(), 3);
    assert_eq!(states[0].visible, states[1].visible);
    assert_ne!(states[1].visible, states[2].visible);
    assert_eq!(
        animated.slides[1].notes.as_deref(),
        Some("Shared package note\n\nSecond step note")
    );
    let targets: Vec<_> = animated
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
        .filter_map(|r| r.hyperlink.as_ref())
        .collect();
    assert!(targets.contains(&&LinkTarget::Slide(3)));
    assert!(
        targets
            .iter()
            .all(|target| !matches!(target, LinkTarget::Slide(n) if *n > 3))
    );
    assert!(targets.contains(&&LinkTarget::Url("https://typst.app/".into())));
    assert_timing(&animated);
}
