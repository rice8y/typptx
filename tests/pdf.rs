//! Small explicit PDF programs exercise the interpreter-to-native boundary.
use std::{fs, io::Cursor};
use typptx::{ir::*, lower, pptx, world::CompilerWorld};

fn pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\x80\x81\x82\x83\n".to_vec();
    let mut offsets = vec![0];
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(obj);
        out.extend_from_slice(b"\nendobj\n");
    }
    let start = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    out
}
fn stream(dict: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", bytes.len()).into_bytes();
    out.extend(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}
fn page(content: &str, resources: &str, extras: Vec<Vec<u8>>) -> Vec<u8> {
    let mut objects=vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << {resources} >> /Contents 4 0 R >>").into_bytes(),
        stream("",content.as_bytes()),
    ];
    objects.extend(extras);
    pdf(objects)
}
fn compile(bytes: &[u8], body: &str, options: &lower::Options) -> Presentation {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("asset.pdf"), bytes).unwrap();
    let input = temp.path().join("test.typ");
    fs::write(
        &input,
        format!("#set page(width:400pt,height:240pt,margin:0pt)\n#place(top+left)[{body}]"),
    )
    .unwrap();
    let (doc, warnings) = CompilerWorld::new(&input, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    lower::convert_with_options(&doc, options).unwrap()
}
fn convert(bytes: &[u8]) -> Presentation {
    let p = compile(
        bytes,
        "#image(\"asset.pdf\",width:200pt,height:100pt)",
        &Default::default(),
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}
fn elements(p: &Presentation) -> Vec<&Element> {
    p.slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
        .collect()
}
fn no_media(p: &Presentation) {
    let zip = zip::ZipArchive::new(Cursor::new(pptx::write(p).unwrap())).unwrap();
    assert!(!zip.file_names().any(|p| p.starts_with("ppt/media/")));
}

#[test]
fn pdf_paths_strokes_and_even_odd_clips_are_native() {
    let source = page(
        "1 0 0 rg 10 20 60 30 re f\n0 0 1 RG 2 w [4 2] 0 d 90 20 m 100 60 130 80 150 30 c S\nq 0 0 100 100 re 20 20 60 60 re W* n 0 1 0 rg 0 0 100 100 re f Q",
        "",
        vec![],
    );
    let p = convert(&source);
    no_media(&p);
    let all = elements(&p);
    let red = all
        .iter()
        .find_map(|e| match e {
            Element::Shape(s)
                if s.fill
                    == Some(Brush::Solid {
                        color: [255, 0, 0, 255],
                    }) =>
            {
                Some(s)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        red.bounds,
        Rect {
            x: 10.,
            y: 50.,
            width: 60.,
            height: 30.
        }
    );
    let stroke = all
        .iter()
        .find_map(|e| match e {
            Element::Shape(s) if s.stroke.is_some() => s.stroke.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(stroke.width, 2.);
    assert_eq!(stroke.dash, vec![4., 2.]);
    let green = all
        .iter()
        .find_map(|e| match e {
            Element::Shape(s)
                if s.fill
                    == Some(Brush::Solid {
                        color: [0, 255, 0, 255],
                    }) =>
            {
                Some(s)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        green
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::Move(_)))
            .count(),
        2
    );
}

#[test]
fn pdf_page_selection_crop_and_rotation_use_the_selected_page() {
    let source=pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_vec(),
        stream("",b"1 0 0 rg 0 0 200 100 re f"),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 150] /CropBox [20 30 120 80] /Rotate 90 /Contents 6 0 R >>".to_vec(),
        stream("",b"0 0 1 rg 20 30 100 50 re f"),
    ]);
    let p = compile(
        &source,
        "#image(\"asset.pdf\",page:2,width:50pt,height:100pt)",
        &Default::default(),
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    no_media(&p);
    let shapes: Vec<_> = elements(&p)
        .into_iter()
        .filter_map(|e| match e {
            Element::Shape(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(shapes.len(), 1);
    assert_eq!(
        shapes[0].bounds,
        Rect {
            x: 0.,
            y: 0.,
            width: 50.,
            height: 100.
        }
    );
    assert_eq!(
        shapes[0].fill,
        Some(Brush::Solid {
            color: [0, 0, 255, 255]
        })
    );
}

#[test]
fn pdf_images_keep_pixels_and_alpha_in_separate_picture_objects() {
    let source = page(
        "q 80 0 0 40 10 30 cm /Im0 Do Q",
        "/XObject << /Im0 5 0 R >>",
        vec![
            stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 6 0 R",
                &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            ),
            stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[255, 0, 128, 255],
            ),
        ],
    );
    let p = convert(&source);
    let pictures: Vec<_> = elements(&p)
        .into_iter()
        .filter_map(|e| match e {
            Element::Picture { bytes, bounds, .. } => Some((bytes, bounds)),
            _ => None,
        })
        .collect();
    assert_eq!(pictures.len(), 1);
    // The source has only 2×2 pixels, but Office's picture itself must have
    // its final physical extent before any rotation/reflection group.
    assert!((pictures[0].1.width - 80.).abs() < 1e-6);
    assert!((pictures[0].1.height - 40.).abs() < 1e-6);
    let image = image::load_from_memory(pictures[0].0).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (2, 2));
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(1, 0).0[3], 0);
    assert_eq!(image.get_pixel(0, 1).0, [0, 0, 255, 128]);
    pptx::write(&p).unwrap();
}

fn font_pdf(text: &str) -> Vec<u8> {
    let (font, _) = typst_kit::fonts::embedded()
        .find(|(_, i)| i.family == "Libertinus Serif" && i.variant == Default::default())
        .unwrap();
    let font = font.clone().instantiate(
        font.info().variant,
        typst::layout::Abs::pt(20.),
        &Default::default(),
    );
    let mut chars = std::collections::BTreeMap::new();
    let mut codes = String::new();
    for c in text.chars() {
        let id = font.ttf().glyph_index(c).unwrap();
        codes.push_str(&format!("{:04X}", id.0));
        chars.insert(c, id.0);
    }
    let widths = chars
        .values()
        .map(|id| {
            format!(
                "{id} [{}]",
                font.x_advance(*id)
                    .unwrap()
                    .at(typst::layout::Abs::pt(1000.))
                    .to_pt()
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mappings = chars
        .iter()
        .map(|(c, id)| format!("<{id:04X}> <{:04X}>", *c as u32))
        .collect::<Vec<_>>()
        .join("\n");
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Test def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange {} beginbfchar {mappings} endbfchar endcmap CMapName currentdict /CMap defineresource pop end end",
        chars.len()
    );
    page(&format!("BT /F0 20 Tf 1 0 0 1 10 70 Tm <{codes}> Tj 1 0 0 1 10 35 Tm <{codes}> Tj ET"),"/Font << /F0 5 0 R >>",vec![
        b"<< /Type /Font /Subtype /Type0 /BaseFont /LibertinusSerif /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>".to_vec(),
        format!("<< /Type /Font /Subtype /CIDFontType0 /BaseFont /LibertinusSerif /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /W [{widths}] >>").into_bytes(),
        b"<< /Type /FontDescriptor /FontName /LibertinusSerif /Flags 4 /FontBBox [-1000 -1000 2000 2000] /ItalicAngle 0 /Ascent 900 /Descent -300 /CapHeight 700 /StemV 80 /FontFile3 8 0 R >>".to_vec(),
        stream("/Subtype /OpenType",font.data().as_slice()),stream("",cmap.as_bytes()),
    ])
}

#[test]
fn pdf_text_with_an_embedded_font_remains_two_editable_lines() {
    let p = convert(&font_pdf("Native ABC Ω"));
    no_media(&p);
    let texts: Vec<_> = elements(&p)
        .into_iter()
        .filter_map(|e| match e {
            Element::Text(t) if t.role == "pdf_text" => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 2);
    for t in texts {
        assert_eq!(t.paragraphs.len(), 1);
        assert_eq!(
            t.paragraphs[0]
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "Native ABC Ω"
        );
    }
    assert_eq!(p.fonts.len(), 1);
}

#[test]
fn pdf_standard_fonts_without_an_opentype_program_remain_native_outlines() {
    let p = convert(&page(
        "BT /F0 18 Tf 10 50 Td (Vector ABC) Tj ET",
        "/Font << /F0 5 0 R >>",
        vec![b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()],
    ));
    no_media(&p);
    assert!(
        elements(&p)
            .iter()
            .filter(|e| matches!(e, Element::Shape(_)))
            .count()
            >= 8
    );
}

#[test]
fn pdf_axial_gradients_keep_their_color_direction_and_finite_extent() {
    let p=convert(&page("/S0 sh","/Shading << /S0 5 0 R >>",vec![
        b"<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [20 0 180 0] /Function 6 0 R /Extend [false false] >>".to_vec(),
        b"<< /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >>".to_vec(),
    ]));
    no_media(&p);
    let (shape, stops, angle) = elements(&p)
        .into_iter()
        .find_map(|e| match e {
            Element::Shape(s) => match &s.fill {
                Some(Brush::Linear { stops, angle }) => Some((s, stops, *angle)),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    assert_eq!(
        shape.bounds,
        Rect {
            x: 20.,
            y: 0.,
            width: 160.,
            height: 100.
        }
    );
    assert!(angle.abs() < 1e-6);
    assert_eq!(stops.first().unwrap().1, [255, 0, 0, 255]);
    assert_eq!(stops.last().unwrap().1, [0, 0, 255, 255]);
}

#[test]
fn unsupported_pdf_blending_is_reported_and_requires_explicit_fallback() {
    let source = page(
        "/GS0 gs 1 0 0 rg 10 10 50 50 re f",
        "/ExtGState << /GS0 5 0 R >>",
        vec![b"<< /Type /ExtGState /BM /Multiply >>".to_vec()],
    );
    let p = compile(
        &source,
        "#image(\"asset.pdf\",width:200pt,height:100pt)",
        &Default::default(),
    );
    assert!(
        p.diagnostics
            .iter()
            .any(|d| d.message.contains("PDF blend modes"))
    );
    assert!(pptx::write(&p).is_err());
    let p = compile(
        &source,
        "#image(\"asset.pdf\",width:200pt,height:100pt)",
        &lower::Options {
            allow_image_fallback: true,
            ..Default::default()
        },
    );
    assert!(
        elements(&p)
            .iter()
            .any(|e| matches!(e, Element::Drawing { .. }))
    );
    pptx::write(&p).unwrap();
}

#[test]
fn pdf_tiling_patterns_keep_clipped_native_tiles() {
    let source = page(
        "/Pattern cs /P0 scn 20 20 60 40 re f",
        "/Pattern << /P0 5 0 R >>",
        vec![stream(
            "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 /Resources << >>",
            b"1 0 0 rg 0 0 5 5 re f",
        )],
    );
    let p = convert(&source);
    no_media(&p);
    let shapes: Vec<_> = elements(&p)
        .into_iter()
        .filter_map(|e| match e {
            Element::Shape(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(shapes.len(), 24);
    for s in shapes {
        assert_eq!(
            s.fill,
            Some(Brush::Solid {
                color: [255, 0, 0, 255]
            })
        );
        assert!(
            s.bounds.x >= 20.
                && s.bounds.right() <= 80.
                && s.bounds.y >= 40.
                && s.bounds.bottom() <= 80.
        );
    }
}

#[test]
fn extremely_dense_pdf_patterns_fail_before_expanding_tiles() {
    let source = page(
        "/Pattern cs /P0 scn 0 0 200 100 re f",
        "/Pattern << /P0 5 0 R >>",
        vec![stream(
            "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [-10000000000 -10000000000 10000000000 10000000000] /XStep 1 /YStep 1 /Resources << >>",
            b"1 0 0 rg 0 0 5 5 re f",
        )],
    );
    let p = compile(
        &source,
        "#image(\"asset.pdf\",width:200pt,height:100pt)",
        &Default::default(),
    );
    assert!(
        p.diagnostics
            .iter()
            .any(|d| d.message.contains("exceeds 16384 native tiles")),
        "{:?}",
        p.diagnostics
    );
    assert!(pptx::write(&p).is_err());
}

#[test]
fn pdf_form_opacity_and_radial_shading_remain_native() {
    let source=page("/GS0 gs /Form0 Do","/ExtGState << /GS0 5 0 R >> /XObject << /Form0 6 0 R >>",vec![
        b"<< /Type /ExtGState /ca 0.5 >>".to_vec(),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 200 100] /Group << /S /Transparency /CS /DeviceRGB /I true >> /Resources << /Shading << /S0 7 0 R >> >>",b"/S0 sh"),
        b"<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [100 50 0 100 50 40] /Function 8 0 R /Extend [false false] >>".to_vec(),
        b"<< /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >>".to_vec(),
    ]);
    let p = convert(&source);
    no_media(&p);
    assert!(
        elements(&p)
            .iter()
            .any(|e| matches!(e,Element::Group(g) if (g.opacity-0.5).abs()<0.01))
    );
    let shapes: Vec<_> = elements(&p)
        .into_iter()
        .filter_map(|e| match e {
            Element::Shape(s) => Some(s),
            _ => None,
        })
        .collect();
    assert!(shapes.len() > 100);
    for s in shapes {
        assert!(
            s.bounds.x >= 59.9
                && s.bounds.right() <= 140.1
                && s.bounds.y >= 9.9
                && s.bounds.bottom() <= 90.1
        );
    }
}

#[test]
fn pdf_image_dpi_limits_pixels_without_replacing_vectors() {
    let pixels = vec![128u8; 256 * 128 * 3];
    let source = page(
        "1 0 0 rg 0 0 10 10 re f q 64 0 0 32 10 20 cm /Im0 Do Q",
        "/XObject << /Im0 5 0 R >>",
        vec![stream(
            "/Type /XObject /Subtype /Image /Width 256 /Height 128 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &pixels,
        )],
    );
    let p = compile(
        &source,
        "#image(\"asset.pdf\",width:200pt,height:100pt)",
        &lower::Options {
            image_dpi: Some(72),
            ..Default::default()
        },
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let all = elements(&p);
    let bytes = all
        .iter()
        .find_map(|e| match e {
            Element::Picture { bytes, .. } => Some(bytes),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        image::load_from_memory(bytes)
            .unwrap()
            .to_rgba8()
            .dimensions(),
        (64, 32)
    );
    assert!(all.iter().any(|e| matches!(e, Element::Shape(_))));
    pptx::write(&p).unwrap();
}

#[test]
fn typst_can_clip_and_rotate_an_embedded_pdf_without_picture_fallback() {
    let source = page("1 0 0 rg 0 0 200 100 re f", "", vec![]);
    for body in [
        "#box(width:80pt,height:60pt,clip:true)[#image(\"asset.pdf\",width:200pt,height:100pt)]",
        "#rotate(25deg, reflow:true)[#box(width:80pt,height:60pt,clip:true)[#image(\"asset.pdf\",width:200pt,height:100pt)]]",
    ] {
        let p = compile(&source, body, &Default::default());
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        no_media(&p);
        let shapes: Vec<_> = elements(&p)
            .into_iter()
            .filter_map(|e| match e {
                Element::Shape(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(shapes.len(), 1);
        assert!((shapes[0].bounds.width - 80.).abs() < 0.02);
        assert!((shapes[0].bounds.height - 60.).abs() < 0.02);
    }
}

#[test]
fn pdf_soft_masks_are_reported_without_silently_dropping_the_mask() {
    let source = page(
        "/GS0 gs 1 0 0 rg 0 0 100 100 re f",
        "/ExtGState << /GS0 5 0 R >>",
        vec![
            b"<< /Type /ExtGState /SMask << /S /Alpha /G 6 0 R >> >>".to_vec(),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 200 100] /Group << /S /Transparency /CS /DeviceRGB >>",
                b"0.5 g 0 0 100 100 re f",
            ),
        ],
    );
    let p = compile(
        &source,
        "#image(\"asset.pdf\",width:200pt,height:100pt)",
        &Default::default(),
    );
    assert!(
        p.diagnostics
            .iter()
            .any(|d| d.message.contains("PDF soft masks")),
        "{:?}",
        p.diagnostics
    );
    assert!(pptx::write(&p).is_err());
}
