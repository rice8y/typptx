use crate::{ir::*, lower, pptx, world::CompilerWorld};
use std::{
    fs,
    io::{Cursor, Read},
};
fn convert(body: &str) -> Presentation {
    let out = convert_unchecked(body);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    out
}
fn convert_unchecked(body: &str) -> Presentation {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("svg.typ");
    let svg =
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="120">{body}</svg>"#);
    fs::write(&p,format!("#set page(width:600pt,height:400pt,margin:30pt)\n#image(bytes({}),format:\"svg\",width:200pt)",serde_json::to_string(&svg).unwrap())).unwrap();
    let (doc, _) = CompilerWorld::new(&p, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    // Exercise the internal SVG interchange used for Typst paint geometry.
    // Imported SVG files take the separate single-picture path.
    let capture = crate::capture::Capture::new(&doc);
    let image = capture.pages[0]
        .iter()
        .find_map(|leaf| {
            if let typst::layout::FrameItem::Image(image, _, _) = &leaf.item
                && let typst::visualize::ImageKind::Svg(svg) = image.kind()
            {
                return Some(svg);
            }
            None
        })
        .unwrap();
    let mut output = lower::convert(&doc).unwrap();
    output.slides[0].elements.clear();
    match super::convert(
        image.tree(),
        Rect {
            x: 30.,
            y: 30.,
            width: 200.,
            height: 120.,
        },
        None,
    ) {
        Ok(elements) => output.slides[0].elements = elements,
        Err(error) => output
            .diagnostics
            .push(crate::compiler::diagnostics::from_error(
                1,
                "svg".into(),
                "unsupported_graphics",
                "image",
                error,
                None,
            )),
    }
    output
}
fn xml(p: &Presentation) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    assert!(!z.file_names().any(|n| n.starts_with("ppt/media/")));
    let mut s = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}
#[test]
fn svg_text_is_one_editable_paragraph_with_source_runs() {
    let p = convert(
        r#"<text x="10" y="40" font-family="Libertinus Serif" font-size="20">Native <tspan fill="red" font-weight="bold">text</tspan></text>"#,
    );
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        d.descendants()
            .filter(|n| n.tag_name().name() == "txBody")
            .count(),
        1
    );
    let text: String = d
        .descendants()
        .filter(|n| n.tag_name().name() == "t")
        .filter_map(|n| n.text())
        .collect();
    assert_eq!(text, "Native text");
}

#[test]
fn svg_character_offsets_keep_one_paragraph_and_source_formatting() {
    let p = convert(
        r#"<text x="60" y="40" text-anchor="middle" font-family="Arial" font-size="20" dx="5 8 -2" dy="4 -5 5">A<tspan fill="red">B</tspan>C</text>"#,
    );
    let texts: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].paragraphs.len(), 1);
    let runs = &texts[0].paragraphs[0].runs;
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "ABC"
    );
    assert_eq!(
        runs.iter()
            .map(|r| r.style.letter_spacing)
            .collect::<Vec<_>>(),
        [8., -2., 0.]
    );
    assert_eq!(
        runs.iter().map(|r| r.style.baseline).collect::<Vec<_>>(),
        [0., 5., 0.]
    );
    assert_eq!(runs[1].style.color, [255, 0, 0, 255]);
    assert!(texts[0].bounds.x < 60.);
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        doc.descendants()
            .filter(|n| n.tag_name().name() == "txBody")
            .count(),
        1
    );
    assert!(
        doc.descendants()
            .any(|n| n.attribute("spc") == Some("-200"))
    );
}

#[test]
fn svg_relative_offsets_do_not_leak_between_source_chunks() {
    let p = convert(
        r#"<text x="10" y="30" font-family="Arial" font-size="16" dx="4 6 9 -3" dy="3 -4 7 2">AB<tspan x="90" y="70">CD</tspan></text>"#,
    );
    let texts: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 2);
    let r = &texts[0].paragraphs[0].runs;
    assert_eq!(r[0].style.letter_spacing, 6.);
    assert_eq!(r[1].style.letter_spacing, 0.);
    let r = &texts[1].paragraphs[0].runs;
    assert_eq!(r[0].style.letter_spacing, -3.);
    assert_eq!(r[1].style.baseline, -2.);
    xml(&p);
}

#[test]
fn rtl_offsets_preserve_logical_order_and_source_formatting() {
    let p = convert(
        r#"<text x="20" y="40" font-family="Arial" font-size="20" dx="8 5 0" dy="0 -5 5">א<tspan fill="red">ב</tspan>ג</text>"#,
    );
    let texts: Vec<_> = p.slides[0]
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
    assert_eq!(texts.len(), 1);
    let paragraph = &texts[0].paragraphs[0];
    assert!(paragraph.rtl);
    let runs = &paragraph.runs;
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "אבג"
    );
    assert_eq!(
        runs.iter()
            .map(|r| r.style.letter_spacing)
            .collect::<Vec<_>>(),
        [8., 5., 0.]
    );
    assert_eq!(
        runs.iter().map(|r| r.style.baseline).collect::<Vec<_>>(),
        [0., 0., -5.]
    );
    assert_eq!(runs[1].style.color, [255, 0, 0, 255]);
    let s = xml(&p);
    assert!(s.contains("rtl=\"1\""));
}

#[test]
fn combining_marks_stay_with_their_base_when_shifted() {
    for body in [
        r#"<text x="20" y="50" font-family="Arial" font-size="20" dy="0 -8 0 8">AéB</text>"#,
        r#"<text x="20" y="50" font-family="Arial" font-size="20" dy="0 -8 0 8">A<tspan fill="red">é</tspan>B</text>"#,
        r#"<text x="20" y="50" font-family="Arial" font-size="20" dy="6 0 -4">אָב</text>"#,
    ] {
        let p = convert(body);
        let texts: Vec<_> = p.slides[0]
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
        assert_eq!(texts.len(), 1);
        let runs = &texts[0].paragraphs[0].runs;
        assert!(runs.iter().any(|r| r.text == "é" || r.text == "אָ"));
        assert!(runs.iter().any(|r| r.style.baseline.abs() > 1.));
        xml(&p);
    }
}

#[test]
fn offsets_requiring_separate_glyph_objects_remain_explicit_diagnostics() {
    for body in [
        r#"<text x="30" y="15" writing-mode="tb" font-family="Arial" font-size="16" dx="0 5">AB</text>"#,
        r#"<text x="30" y="40" font-family="Arial" font-size="16" dx="0 0 5">Aאב</text>"#,
        r#"<text x="30" y="40" font-family="Arial" font-size="16" rotate="0 20">AB</text>"#,
        r#"<text x="30" y="40" font-family="Arial" font-size="16" dx="0 -25">AB</text>"#,
        r#"<text x="30" y="40" font-family="Arial" font-size="16" dy="0 0 5">AéB</text>"#,
        r#"<text x="30" y="40" font-family="Arial" font-size="16" dx="0 0 0 5">AéB</text>"#,
        r#"<text x="30" y="40" font-family="Arial" font-size="16" dx="0 5">مرحبا</text>"#,
    ] {
        let p = convert_unchecked(body);
        assert!(!p.diagnostics.is_empty());
        assert!(
            !p.slides[0]
                .elements
                .iter()
                .flat_map(Element::walk)
                .any(|e| matches!(e, Element::Picture { .. }))
        );
    }
}

#[test]
fn mixed_direction_offsets_keep_logical_text_in_one_paragraph() {
    for source in [r#"AאבB"#, r#"A<tspan fill="red">א</tspan>בB"#] {
        let p = convert(&format!(
            r#"<text x="20" y="50" font-family="Arial" font-size="20" dx="0 7 0 0" dy="0 -5 3 2">{source}</text>"#
        ));
        let texts: Vec<_> = p.slides[0]
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
        assert_eq!(texts.len(), 1);
        let paragraph = &texts[0].paragraphs[0];
        assert!(!paragraph.rtl);
        assert_eq!(
            paragraph
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "AאבB"
        );
        assert!(
            paragraph
                .runs
                .iter()
                .any(|r| r.text == "א" && r.style.rtl && r.style.letter_spacing == 7.)
        );
        assert!(
            paragraph
                .runs
                .iter()
                .any(|r| r.text == "ב" && r.style.rtl && r.style.baseline == -3.)
        );
        assert!(
            paragraph
                .runs
                .iter()
                .any(|r| r.text == "א" && r.style.baseline == 2.)
        );
        xml(&p);
    }
}

#[test]
fn mixed_direction_baselines_use_svg_paragraph_order_even_when_rtl_comes_first() {
    let p = convert(
        r#"<text x="20" y="50" font-family="Arial" font-size="20" dy="0 3 -3 2 -2">אבA12</text>"#,
    );
    let paragraph = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Text(t) = e {
                Some(&t.paragraphs[0])
            } else {
                None
            }
        })
        .unwrap();
    assert!(!paragraph.rtl);
    assert_eq!(
        paragraph
            .runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>(),
        "אבA12"
    );
    assert!(paragraph.runs.iter().any(|r| r.style.rtl));
    assert!(paragraph.runs.iter().any(|r| !r.style.rtl));
    xml(&p);
}

#[test]
fn svg_shadows_and_blur_are_effects_on_native_groups() {
    let p = convert(
        r##"<defs><filter id="s" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="120"><feDropShadow dx="6" dy="4" stdDeviation="2" flood-color="#345678" flood-opacity="0.5"/></filter><filter id="b" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="120" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="2"/></filter></defs><g filter="url(#s)"><rect x="20" y="20" width="35" height="25" fill="red"/><rect x="40" y="30" width="35" height="25" fill="blue"/></g><text x="100" y="75" font-family="Arial" font-size="18" filter="url(#b)">Blur</text>"##,
    );
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    let shadow = doc
        .descendants()
        .find(|n| n.tag_name().name() == "outerShdw")
        .unwrap();
    assert_eq!(shadow.attribute("blurRad"), Some("50800"));
    assert!(
        shadow
            .descendants()
            .any(|n| n.attribute("val") == Some("345678"))
    );
    let blur = doc
        .descendants()
        .find(|n| n.tag_name().name() == "blur")
        .unwrap();
    assert_eq!(blur.attribute("rad"), Some("50800"));
    assert!(s.contains(">Blur<"));
    assert_eq!(
        doc.descendants()
            .filter(|n| n.tag_name().name() == "txBody")
            .count(),
        1
    );
    let shadow_group = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Group(g) = e {
                g.effect
                    .as_ref()
                    .is_some_and(|e| matches!(e, Effect::Shadow { .. }))
                    .then_some(g)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(shadow_group.elements.len(), 2);
}

#[test]
fn sequential_filters_keep_effect_order_and_group_opacity() {
    let p = convert(
        r##"<defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="120" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="1" result="b"/><feDropShadow in="b" dx="4" dy="3" stdDeviation="1"/></filter></defs><g opacity="0.4" filter="url(#f)"><rect x="30" y="30" width="50" height="30"/></g>"##,
    );
    let s = xml(&p);
    let doc = roxmltree::Document::parse(&s).unwrap();
    assert!(
        doc.descendants()
            .any(|n| n.tag_name().name() == "alphaModFix" && n.attribute("amt") == Some("40000"))
    );
    let effects: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| {
            if let Element::Group(g) = e {
                g.effect.as_ref()
            } else {
                None
            }
        })
        .collect();
    assert!(matches!(
        effects.as_slice(),
        [Effect::Shadow { .. }, Effect::Blur { .. }]
    ));
}

#[test]
fn expanded_svg_shadows_match_drop_shadow_and_keep_editable_content() {
    let wrap = |primitives: &str| {
        format!(
            r##"<defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="120">{primitives}</filter></defs><g filter="url(#f)" opacity="0.6"><rect x="35" y="35" width="40" height="20"/><text x="40" y="75" font-family="Arial" font-size="16">Native</text></g>"##
        )
    };
    let expected = convert(&wrap(
        r##"<feDropShadow stdDeviation="2" dx="6" dy="4" flood-color="#345678" flood-opacity="0.5"/>"##,
    ));
    let effect = |p: &Presentation| {
        p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .find_map(|e| {
                if let Element::Group(g) = e {
                    g.effect.clone()
                } else {
                    None
                }
            })
            .unwrap()
    };
    for ending in [
        r#"<feMerge><feMergeNode in="shadow"/><feMergeNode in="SourceGraphic"/></feMerge>"#,
        r#"<feComposite in="SourceGraphic" in2="shadow" operator="over"/>"#,
    ] {
        let p = convert(&wrap(&format!(
            r##"<feGaussianBlur in="SourceAlpha" stdDeviation="2" result="b"/><feOffset in="b" dx="6" dy="4" result="b"/><feFlood flood-color="#345678" flood-opacity="0.5" result="color"/><feComposite in="color" in2="b" operator="in" result="shadow"/>{ending}"##
        )));
        assert_eq!(effect(&p), effect(&expected));
        let s = xml(&p);
        assert!(s.contains(">Native<"));
        let d = roxmltree::Document::parse(&s).unwrap();
        assert_eq!(
            d.descendants()
                .filter(|n| n.tag_name().name() == "outerShdw")
                .count(),
            1
        );
        assert!(
            d.descendants().any(
                |n| n.tag_name().name() == "alphaModFix" && n.attribute("amt") == Some("60000")
            )
        );
    }
}

#[test]
fn alpha_only_shadows_and_reordered_dependencies_are_supported() {
    for primitives in [
        r#"<feOffset in="SourceAlpha" dx="-4" dy="3" result="a"/><feGaussianBlur in="a" stdDeviation="1" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge>"#,
        r#"<feFlood result="color"/><feGaussianBlur in="SourceAlpha" stdDeviation="1" result="b"/><feOffset in="b" dx="-4" dy="3" result="o"/><feComposite in="color" in2="o" operator="in" result="s"/><feMerge><feMergeNode in="s"/><feMergeNode in="SourceGraphic"/></feMerge>"#,
    ] {
        let p = convert(&format!(
            r##"<defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="120">{primitives}</filter></defs><rect x="30" y="30" width="60" height="40" filter="url(#f)"/>"##
        ));
        assert!(p.slides[0].elements.iter().flat_map(Element::walk).any(|e| matches!(e, Element::Group(g) if g.effect == Some(Effect::Shadow { radius: 2., offset: [-4.,3.], color:[0,0,0,255] }))));
        xml(&p);
    }
}

#[test]
fn expanded_shadow_clips_and_different_compositing_are_not_silently_ignored() {
    for primitives in [
        r#"<feGaussianBlur in="SourceAlpha" stdDeviation="2" x="30" y="30" width="60" height="40" result="b"/><feOffset in="b" dx="8" dy="5" result="s"/><feMerge><feMergeNode in="s"/><feMergeNode in="SourceGraphic"/></feMerge>"#,
        r#"<feGaussianBlur in="SourceGraphic" stdDeviation="2" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge>"#,
        r#"<feGaussianBlur in="SourceAlpha" stdDeviation="2" result="b"/><feFlood x="40" y="40" width="10" height="10" result="c"/><feComposite in="c" in2="b" operator="in" result="s"/><feMerge><feMergeNode in="s"/><feMergeNode in="SourceGraphic"/></feMerge>"#,
        r#"<feGaussianBlur in="SourceAlpha" stdDeviation="2" result="b"/><feMerge><feMergeNode in="SourceGraphic"/><feMergeNode in="b"/></feMerge>"#,
    ] {
        let p = convert_unchecked(&format!(
            r##"<defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="120">{primitives}</filter></defs><rect x="30" y="30" width="60" height="40" filter="url(#f)"/>"##
        ));
        assert!(!p.diagnostics.is_empty(), "{primitives}");
        assert!(pptx::write(&p).is_err());
    }
}

#[test]
fn filters_that_need_color_conversion_compositing_or_clipping_are_diagnosed() {
    for filter in [
        r#"<feGaussianBlur stdDeviation="2"/>"#,
        r#"<feGaussianBlur stdDeviation="2 4" color-interpolation-filters="sRGB"/>"#,
        r#"<feGaussianBlur in="SourceAlpha" stdDeviation="2" color-interpolation-filters="sRGB"/>"#,
        r#"<feTurbulence baseFrequency="0.1"/>"#,
        r#"<feGaussianBlur stdDeviation="2" result="b" color-interpolation-filters="sRGB"/><feDropShadow in="SourceGraphic"/>"#,
    ] {
        let p = convert_unchecked(&format!(
            r##"<defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="120">{filter}</filter></defs><rect x="30" y="30" width="60" height="40" filter="url(#f)"/>"##
        ));
        assert!(!p.diagnostics.is_empty(), "{filter}");
        assert!(pptx::write(&p).is_err());
    }
    for attributes in [
        r#"x="30" y="30" width="60" height="40""#,
        r#"x="0" y="0" width="200" height="120""#,
    ] {
        let p = convert_unchecked(&format!(
            r##"<defs><filter id="f" filterUnits="userSpaceOnUse" {attributes} color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="4"/></filter></defs><rect x="2" y="2" width="60" height="40" filter="url(#f)"/>"##
        ));
        assert!(!p.diagnostics.is_empty());
    }
}
#[test]
fn gradients_even_odd_and_clips_stay_editable_without_images() {
    let p = convert(
        r##"<defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><clipPath id="c"><circle cx="70" cy="60" r="40"/></clipPath></defs><g clip-path="url(#c)"><path fill="url(#g)" fill-rule="evenodd" d="M0 0H150V100H0Z M60 50H80V70H60Z"/></g>"##,
    );
    let s = xml(&p);
    assert!(s.contains("<a:gradFill"));
    assert!(s.contains("<a:custGeom>"));
    let shapes: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| {
            if let Element::Shape(s) = e {
                Some(s)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(shapes.len(), 1);
    assert!(shapes[0].bounds.width <= 80.01 && shapes[0].bounds.height <= 80.01);
    assert!(
        shapes[0]
            .commands
            .iter()
            .filter(|p| matches!(p, PathCommand::Close))
            .count()
            >= 2
    );
}
#[test]
fn nonuniform_strokes_and_dash_offsets_become_native_outlines() {
    let p = convert(
        r#"<g transform="translate(20 20) scale(2 1)"><path d="M0 0 L60 50" fill="none" stroke="blue" stroke-width="5" stroke-dasharray="8 4" stroke-dashoffset="3"/></g>"#,
    );
    let s = xml(&p);
    assert!(s.contains("<a:custGeom>"));
    assert!(
        p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .any(|e| matches!(e,Element::Shape(s) if s.stroke.is_none() && s.fill.is_some()))
    );
}
#[test]
fn source_text_gradients_and_outlines_are_native_character_properties() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("text.typ");
    fs::write(&p,"#set page(width:600pt,height:300pt)\n#text(fill:gradient.linear(red,blue),stroke:0.2pt+black)[Editable text]").unwrap();
    let (doc, _) = CompilerWorld::new(&p, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let s = xml(&p);
    assert!(s.contains("<a:gradFill"));
    assert!(s.contains("<a:ln "));
}

#[test]
fn group_opacity_is_applied_once_to_overlapping_children() {
    let p = convert(
        r#"<g opacity="0.5"><rect width="80" height="80" fill="red"/><rect x="20" width="80" height="80" fill="blue"/></g>"#,
    );
    let s = xml(&p);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        d.descendants()
            .filter(|n| n.tag_name().name() == "alphaModFix" && n.attribute("amt") == Some("50000"))
            .count(),
        1
    );
    let group = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Group(g) = e {
                (g.opacity < 1.).then_some(g)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(group.elements.len(), 2);
    assert!(group.elements.iter().all(
        |e| matches!(e,Element::Shape(s) if matches!(s.fill,Some(Brush::Solid{color:[_,_,_,255]})))
    ));
}

#[test]
fn patterns_are_repeated_native_shapes_clipped_to_the_painted_path() {
    let p = convert(
        r##"<defs><pattern id="p" width="20" height="20" patternUnits="userSpaceOnUse"><rect width="10" height="10" fill="red"/></pattern></defs><rect x="10" y="10" width="65" height="45" fill="url(#p)"/>"##,
    );
    let shapes: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| {
            if let Element::Shape(s) = e {
                Some(s)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(shapes.len(), 1);
    assert_eq!(
        shapes[0]
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::Close))
            .count(),
        6
    );
    for s in shapes {
        assert!(
            s.bounds.x >= 40. - 0.01
                && s.bounds.right() <= 105. + 0.01
                && s.bounds.y >= 40. - 0.01
                && s.bounds.bottom() <= 85. + 0.01
        );
    }
    xml(&p);
}

#[test]
fn opaque_luminance_masks_preserve_holes_as_native_geometry() {
    let p = convert(
        r##"<defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100"><rect width="100" height="100" fill="white"/><circle cx="50" cy="50" r="20" fill="black"/></mask></defs><rect width="100" height="100" mask="url(#m)" fill="blue"/>"##,
    );
    let shape = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .find_map(|e| {
            if let Element::Shape(s) = e {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        shape
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::Close))
            .count(),
        2
    );
    xml(&p);
}

#[test]
fn svg_text_clipped_across_a_line_keeps_source_text_as_one_paragraph() {
    let p = convert(
        r##"<defs><clipPath id="c"><rect x="0" y="32" width="180" height="8"/></clipPath></defs><text x="10" y="40" font-family="Libertinus Serif" font-size="16" clip-path="url(#c)">Editable clipped text</text>"##,
    );
    let s = xml(&p);
    assert!(s.contains("Editable clipped text"));
    assert!(s.contains("horzOverflow=\"clip\""));
    assert!(s.contains("vertOverflow=\"clip\""));
}

#[test]
fn repeated_and_reflected_radial_gradients_remain_native_color_bands() {
    for spread in ["repeat", "reflect"] {
        let p = convert(&format!(
            r##"<defs><radialGradient id="g" r="0.15" spreadMethod="{spread}"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></radialGradient></defs><rect width="180" height="110" fill="url(#g)"/>"##
        ));
        let s = xml(&p);
        assert!(s.contains("FF0000"));
        assert!(s.contains("0000FF"));
        assert!(s.contains("<p:grpSp>"));
    }
}
