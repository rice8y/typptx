use std::fs;
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn over(dst: &mut [f64; 4], src: [f64; 4]) {
    for i in 0..3 {
        dst[i] = src[i] + dst[i] * (1. - src[3]);
    }
    dst[3] = src[3] + dst[3] * (1. - src[3]);
}
fn sample(elements: &[Element], point: [f64; 2]) -> [f64; 4] {
    let mut color = [0.; 4];
    for element in elements {
        let c = match element {
            Element::Group(g) => {
                assert_eq!(g.rotation, 0.);
                let mut c = sample(&g.elements, point);
                for v in &mut c {
                    *v *= g.opacity;
                }
                c
            }
            Element::Shape(s) => {
                let (x, y) = (point[0] - s.bounds.x, point[1] - s.bounds.y);
                let mut winding = 0;
                let mut start = [0.; 2];
                let mut last = start;
                let mut edge = |a: [f64; 2], b: [f64; 2]| {
                    let cross = (b[0] - a[0]) * (y - a[1]) - (x - a[0]) * (b[1] - a[1]);
                    if a[1] <= y && b[1] > y && cross > 0. {
                        winding += 1;
                    }
                    if a[1] > y && b[1] <= y && cross < 0. {
                        winding -= 1;
                    }
                };
                for c in &s.commands {
                    match *c {
                        PathCommand::Move(p) => {
                            start = p;
                            last = p;
                        }
                        PathCommand::Line(p) => {
                            edge(last, p);
                            last = p;
                        }
                        PathCommand::Close => edge(last, start),
                        _ => panic!("gradient bands should already be native polygons"),
                    }
                }
                if winding == 0 {
                    continue;
                }
                let Some(Brush::Solid { color: c }) = s.fill else {
                    panic!("nonlinear paint was not lowered")
                };
                let alpha = f64::from(c[3]) / 255.;
                [
                    f64::from(c[0]) * alpha / 255.,
                    f64::from(c[1]) * alpha / 255.,
                    f64::from(c[2]) * alpha / 255.,
                    alpha,
                ]
            }
            _ => panic!("a gradient must not become a picture"),
        };
        over(&mut color, c);
    }
    color
}

#[test]
fn native_radial_and_conic_colors_match_typsts_rendering() {
    for paint in [
        "gradient.radial(red,blue,radius:25%)",
        "gradient.radial(red,blue,center:(60%,50%),focal-center:(40%,50%),focal-radius:10%,radius:50%)",
        "gradient.radial(red.transparentize(50%),blue.transparentize(50%))",
        "gradient.conic(red,green,blue)",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("gradient.typ");
        fs::write(&input,format!("#set page(width:160pt,height:120pt,margin:0pt)\n#rect(width:160pt,height:120pt,stroke:none,fill:{paint})")).unwrap();
        let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
            .unwrap()
            .compile()
            .unwrap();
        let p = lower::convert(&doc).unwrap();
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        let reference = typst_render::render(&doc.pages()[0], &Default::default());
        for [x, y] in [[43, 42], [71, 51], [90, 77], [125, 88], [28, 96]] {
            let mut actual = [1.; 4];
            over(
                &mut actual,
                sample(
                    &p.slides[0].elements,
                    [f64::from(x) + 0.25, f64::from(y) + 0.25],
                ),
            );
            let expected = reference.pixel(x * 2, y * 2).unwrap();
            for (a, b) in
                actual[..3]
                    .iter()
                    .zip([expected.red(), expected.green(), expected.blue()])
            {
                assert!(
                    (a * 255. - f64::from(b)).abs() < 8.,
                    "{paint} at {x},{y}: {actual:?} vs {expected:?}"
                );
            }
        }
        pptx::write(&p).unwrap();
    }
}

#[test]
fn typst_tiling_coordinates_match_the_renderer_including_parent_relative_paints() {
    for text in [false, true] {
        for (index, source) in [
            "#rect(width:160pt,height:120pt,stroke:none,fill:p)",
            "#place(dx:13pt,dy:17pt)[#rect(width:110pt,height:80pt,stroke:none,fill:p)]",
            "#set page(fill:p)\n",
            "#table(columns:(80pt,80pt),rows:(60pt,60pt),stroke:none,fill:p,[],[],[],[])",
        ]
        .into_iter()
        .enumerate()
        {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("tiling.typ");
            let body = if text {
                r##"[#place(rect(width:10pt,height:12pt,fill:blue,stroke:none))#text(size:1pt,fill:rgb("#00000000"))[A]]"##
            } else {
                "rect(width:10pt,height:12pt,fill:blue,stroke:none)"
            };
            fs::write(&input,format!("#set page(width:160pt,height:120pt,margin:0pt)\n#let p = tiling(relative:{},size:(20pt,20pt),spacing:(3pt,5pt),offset:(4pt,7pt),{body})\n{source}", if index==1 { "\"parent\"" } else { "\"self\"" })).unwrap();
            let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
                .unwrap()
                .compile()
                .unwrap();
            let p = lower::convert(&doc).unwrap();
            assert!(p.diagnostics.is_empty(), "{source}: {:?}", p.diagnostics);
            let elements: Vec<_> = p.slides[0]
                .elements
                .iter()
                .flat_map(Element::walk)
                .filter(|e| matches!(e, Element::Shape(_)))
                .cloned()
                .collect();
            assert!(!elements.is_empty());
            let reference = typst_render::render(&doc.pages()[0], &Default::default());
            for [x, y] in [[8, 10], [18, 20], [35, 39], [71, 51], [102, 90], [132, 80]] {
                let mut actual = [1.; 4];
                over(
                    &mut actual,
                    sample(&elements, [f64::from(x) + 0.25, f64::from(y) + 0.25]),
                );
                let expected = reference.pixel(x * 2, y * 2).unwrap();
                for (a, b) in
                    actual[..3]
                        .iter()
                        .zip([expected.red(), expected.green(), expected.blue()])
                {
                    assert!(
                        (a * 255. - f64::from(b) - f64::from(255 - expected.alpha())).abs() < 8.,
                        "{source} at {x},{y}: {actual:?} vs {expected:?}"
                    );
                }
            }
            let bytes = pptx::write(&p).unwrap();
            let z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            assert!(!z.file_names().any(|n| n.starts_with("ppt/media/")));
        }
    }
}

#[test]
fn native_even_odd_fills_preserve_holes_with_and_without_clipping() {
    for clipped in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("holes.typ");
        let body = r#"#curve(fill:blue,fill-rule:"even-odd",stroke:none,
            curve.line((100pt,0pt)),curve.line((100pt,100pt)),curve.line((0pt,100pt)),curve.close(),
            curve.move((20pt,20pt)),curve.line((80pt,20pt)),curve.line((80pt,80pt)),curve.line((20pt,80pt)),curve.close())"#;
        let body = if clipped {
            format!("#box(width:90pt,height:90pt,clip:true)[{body}]")
        } else {
            body.into()
        };
        fs::write(
            &input,
            format!("#set page(width:120pt,height:120pt,margin:0pt)\n{body}"),
        )
        .unwrap();
        let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
            .unwrap()
            .compile()
            .unwrap();
        let p = lower::convert(&doc).unwrap();
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        assert_eq!(sample(&p.slides[0].elements, [40., 40.]), [0.; 4]);
        assert!(sample(&p.slides[0].elements, [10., 40.])[3] > 0.99);
        assert_eq!(
            sample(&p.slides[0].elements, [95., 40.])[3] > 0.99,
            !clipped
        );
        pptx::write(&p).unwrap();
    }
}

#[test]
fn tiling_text_retains_paragraphs_clipping_and_fonts() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("text-pattern.typ");
    fs::write(&input, "#set page(width:120pt,height:120pt,margin:0pt)\n#rect(width:120pt,height:96pt,stroke:none,fill:tiling(size:(30pt,30pt))[*Text*])").unwrap();
    let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    let p = lower::convert(&doc).unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let blocks: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .filter_map(|e| match e {
            Element::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 16);
    assert!(blocks.iter().all(|b| {
        b.paragraphs.len() == 1
            && b.paragraphs[0]
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>()
                == "Text"
    }));
    assert!(blocks.iter().any(|b| b.clip.is_some()));
    assert!(blocks.iter().all(|b| b.paragraphs[0].runs[0].style.bold));
    assert!(!p.fonts.is_empty());
    assert!(
        p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .all(|e| matches!(e, Element::Text(_) | Element::Group(_)))
    );
    pptx::write(&p).unwrap();
}

#[test]
fn text_pattern_backgrounds_embed_fonts_and_respect_svg_math_option() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("text-background.typ");
    fs::write(
        &input,
        "#set page(width:90pt,height:90pt,margin:0pt,fill:tiling(size:(45pt,45pt))[Label $x^2$])",
    )
    .unwrap();
    let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    let p = lower::convert_with_options(
        &doc,
        &lower::Options {
            math_format: lower::MathFormat::Svg,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert!(!p.fonts.is_empty());
    let objects: Vec<_> = p.slides[0]
        .elements
        .iter()
        .flat_map(Element::walk)
        .collect();
    assert_eq!(
        objects
            .iter()
            .filter(|e| matches!(e, Element::MathSvg { .. }))
            .count(),
        4
    );
    pptx::write(&p).unwrap();
}

#[test]
fn a_pattern_cannot_silently_outline_text_cut_by_a_curved_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("text-clip.typ");
    fs::write(
        &input,
        "#circle(radius:50pt,fill:tiling(size:(30pt,30pt))[Text])",
    )
    .unwrap();
    let (doc, _) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    let p = lower::convert(&doc).unwrap();
    assert!(
        p.diagnostics
            .iter()
            .any(|d| d.message.contains("rectangular clip")),
        "{:?}",
        p.diagnostics
    );
    assert!(pptx::write(&p).is_err());
}
