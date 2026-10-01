//! Imported PDF pages stay single pictures, including text and compositing.
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

fn picture(p: &Presentation) -> (&Rect, &[u8], &str) {
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let all = elements(p);
    assert_eq!(
        all.iter()
            .filter(|e| matches!(e, Element::Picture { .. }))
            .count(),
        1
    );
    assert!(!all.iter().any(|e| matches!(
        e,
        Element::Shape(_) | Element::Text(_) | Element::Drawing { .. }
    )));
    assert!(p.fonts.is_empty());
    all.into_iter()
        .find_map(|e| match e {
            Element::Picture {
                bounds,
                bytes,
                svg: Some(svg),
                ..
            } => Some((bounds, bytes.as_slice(), svg.as_str())),
            _ => None,
        })
        .unwrap()
}

#[test]
fn pdf_text_and_paths_stay_one_vector_picture_without_embedded_office_fonts() {
    let p = convert(&font_pdf("Native ABC Ω"));
    let (bounds, bytes, svg) = picture(&p);
    assert_eq!((bounds.width, bounds.height), (200., 100.));
    assert_eq!(image::load_from_memory(bytes).unwrap().width(), 400);
    assert!(svg.contains("<path"));
    let mut z = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    use std::io::Read;
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        doc.descendants()
            .filter(|n| n.tag_name().name() == "pic")
            .count(),
        1
    );
    assert!(
        !doc.descendants()
            .any(|n| matches!(n.tag_name().name(), "txBody" | "sp"))
    );
    assert!(doc.descendants().any(|n| n.tag_name().name() == "svgBlip"));
    assert_eq!(
        z.file_names()
            .filter(|n| n.starts_with("ppt/media/"))
            .count(),
        2
    );
}

#[test]
fn selected_pdf_page_retains_crop_rotation_and_color() {
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
    let (bounds, bytes, _) = picture(&p);
    assert_eq!((bounds.width, bounds.height), (50., 100.));
    let png = image::load_from_memory(bytes).unwrap().to_rgba8();
    assert_eq!(png.dimensions(), (100, 200));
    assert_eq!(png.get_pixel(50, 100).0, [0, 0, 255, 255]);
    pptx::write(&p).unwrap();
}

#[test]
fn imported_pdf_blending_and_masks_do_not_require_editable_conversion() {
    let blending = page(
        "0 0 1 rg 0 0 200 100 re f /GS0 gs 1 0 0 rg 10 10 50 50 re f",
        "/ExtGState << /GS0 5 0 R >>",
        vec![b"<< /Type /ExtGState /BM /Multiply >>".to_vec()],
    );
    let mask = page(
        "/GS0 gs 1 0 0 rg 0 0 200 100 re f",
        "/ExtGState << /GS0 5 0 R >>",
        vec![
            b"<< /Type /ExtGState /SMask << /S /Alpha /G 6 0 R >> >>".to_vec(),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 200 100] /Group << /S /Transparency /CS /DeviceRGB >>",
                b"0 g 0 0 100 100 re f",
            ),
        ],
    );
    for (source, color) in [(blending, [0, 0, 0, 255]), (mask, [255, 0, 0, 255])] {
        let p = convert(&source);
        let (_, bytes, _) = picture(&p);
        let png = image::load_from_memory(bytes).unwrap().to_rgba8();
        assert_eq!(png.get_pixel(50, 150).0, color);
        pptx::write(&p).unwrap();
    }
}

#[test]
fn pdf_preview_dpi_changes_pixels_but_preserves_vector_data_and_size() {
    let source = page("1 0 0 rg 0 0 200 100 re f", "", vec![]);
    let mut previous = None;
    for (dpi, pixels) in [
        (None, (400, 200)),
        (Some(72), (200, 100)),
        (Some(144), (400, 200)),
    ] {
        let p = compile(
            &source,
            "#image(\"asset.pdf\",width:200pt,height:100pt)",
            &lower::Options {
                image_dpi: dpi,
                ..Default::default()
            },
        );
        let (bounds, bytes, svg) = picture(&p);
        assert_eq!((bounds.width, bounds.height), (200., 100.));
        assert_eq!(
            image::load_from_memory(bytes)
                .unwrap()
                .to_rgba8()
                .dimensions(),
            pixels
        );
        if let Some(old) = &previous {
            assert_eq!(old, svg);
        }
        previous = Some(svg.to_owned());
    }
}

#[test]
fn pdf_clipping_and_rotation_keep_one_picture_with_a_crop() {
    let source = page("1 0 0 rg 0 0 200 100 re f", "", vec![]);
    for body in [
        "#box(width:80pt,height:60pt,clip:true)[#image(\"asset.pdf\",width:200pt,height:100pt)]",
        "#rotate(25deg, reflow:true)[#box(width:80pt,height:60pt,clip:true)[#image(\"asset.pdf\",width:200pt,height:100pt)]]",
    ] {
        let p = compile(&source, body, &Default::default());
        picture(&p);
        assert!(
            elements(&p)
                .iter()
                .any(|e| matches!(e, Element::Picture { clip: Some(_), .. }))
        );
        pptx::write(&p).unwrap();
    }
}

#[test]
fn pdf_inside_typst_tiling_stays_pictures() {
    let source = page("1 0 0 rg 0 0 200 100 re f", "", vec![]);
    let p = compile(
        &source,
        "#rect(width:80pt,height:40pt,stroke:none,fill:tiling(size:(40pt,20pt))[#image(\"asset.pdf\",width:40pt,height:20pt)])",
        &Default::default(),
    );
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    let all = elements(&p);
    assert_eq!(
        all.iter()
            .filter(|e| matches!(e, Element::Picture { .. }))
            .count(),
        4
    );
    assert!(
        !all.iter()
            .any(|e| matches!(e, Element::Shape(_) | Element::Drawing { .. }))
    );
    pptx::write(&p).unwrap();
}

#[test]
fn pdf_picture_retains_its_alternative_text() {
    let source = page("1 0 0 rg 0 0 200 100 re f", "", vec![]);
    let p = compile(
        &source,
        "#image(\"asset.pdf\",width:200pt,height:100pt,alt:\"Red rectangle PDF\")",
        &Default::default(),
    );
    assert!(elements(&p).iter().any(
        |e| matches!(e, Element::Picture { alt: Some(alt), .. } if alt == "Red rectangle PDF")
    ));
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(Cursor::new(pptx::write(&p).unwrap())).unwrap();
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("descr=\"Red rectangle PDF\""));
}
