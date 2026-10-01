use image::{DynamicImage, ImageBuffer, ImageEncoder, Rgb, Rgba, codecs::jpeg::JpegEncoder};
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    process::Command,
};
use typptx::{
    ir::{Element, Presentation},
    lower, pptx,
    world::CompilerWorld,
};

fn document(dir: &Path, body: &str) -> typst_layout::PagedDocument {
    let source = dir.join("test.typ");
    fs::write(&source, format!(
        "#set page(width:480pt,height:600pt,margin:20pt)\n#set text(font:\"Libertinus Serif\")\n{body}"
    )).unwrap();
    CompilerWorld::new(&source, None, &[], &[])
        .unwrap()
        .compile()
        .unwrap()
        .0
}

fn convert(doc: &typst_layout::PagedDocument, dpi: Option<u32>) -> Presentation {
    let p = lower::convert_with_options(
        doc,
        &lower::Options {
            image_dpi: dpi,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p
}

fn pictures(p: &Presentation) -> Vec<(&typptx::ir::Rect, &str, &[u8])> {
    p.slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
        .filter_map(|e| match e {
            Element::Picture {
                bounds,
                extension,
                bytes,
                ..
            } => Some((bounds, extension.as_str(), bytes.as_slice())),
            _ => None,
        })
        .collect()
}

#[test]
fn svg_picture_preview_uses_placement_dpi_and_keeps_the_vector_asset() {
    let dir = tempfile::tempdir().unwrap();
    sources(dir.path());
    // Inline data keeps the fixture self-contained for Typst's SVG loader.
    let data = fs::read(dir.path().join("source.png")).unwrap();
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='144' height='72'><image width='144' height='72' href='data:image/png;base64,{encoded}'/></svg>"
    );
    let doc = document(
        dir.path(),
        &format!(
            "#image(bytes({}),format:\"svg\",width:144pt)",
            serde_json::to_string(&svg).unwrap()
        ),
    );
    let p = convert(&doc, Some(100));
    assert!(
        p.slides[0]
            .elements
            .iter()
            .flat_map(Element::walk)
            .any(|e| matches!(e, Element::Picture { svg: Some(_), .. }))
    );
    let pictures = pictures(&p);
    assert_eq!(pictures.len(), 1);
    let decoded = image::load_from_memory(pictures[0].2).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (200, 100));
    pptx::write(&p).unwrap();
}

#[test]
fn source_webp_and_gif_images_are_embedded_as_editable_png_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = image::RgbaImage::from_pixel(32, 16, image::Rgba([50, 100, 150, 255]));
    for format in [image::ImageFormat::WebP, image::ImageFormat::Gif] {
        let filename = format!("source.{}", format.extensions_str()[0]);
        pixels
            .save_with_format(dir.path().join(&filename), format)
            .unwrap();
        let doc = document(dir.path(), &format!("#image(\"{filename}\",width:72pt)"));
        let p = convert(&doc, None);
        let pics = pictures(&p);
        assert_eq!(pics[0].1, "png");
        assert_eq!(image::load_from_memory(pics[0].2).unwrap().width(), 32);
    }
}

fn sources(dir: &Path) {
    let rgb = ImageBuffer::from_fn(600, 300, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, 180_u8])
    });
    rgb.save(dir.join("source.png")).unwrap();
    rgb.save(dir.join("source.jpg")).unwrap();
}

#[test]
fn original_bytes_survive_default_and_larger_dpi_requests() {
    let dir = tempfile::tempdir().unwrap();
    sources(dir.path());
    let doc = document(
        dir.path(),
        "#image(\"source.png\",width:144pt)\n\n#image(\"source.jpg\",width:144pt)",
    );
    for dpi in [None, Some(300), Some(600), Some(u32::MAX)] {
        let p = convert(&doc, dpi);
        let images = pictures(&p);
        assert_eq!(images.len(), 2);
        for (_, extension, bytes) in images {
            assert_eq!(
                bytes,
                fs::read(dir.path().join(format!("source.{extension}"))).unwrap()
            );
        }
    }
}

#[test]
fn cli_dpi_resamples_per_placement_and_keeps_native_objects_and_geometry() {
    let dir = tempfile::tempdir().unwrap();
    sources(dir.path());
    let doc = document(
        dir.path(),
        "- Editable list\n- Second item\n\n#rect(width:20pt,height:10pt,fill:red)\n\n#image(\"source.png\",width:144pt)\n\n#image(\"source.png\",width:72pt)\n\n#image(\"source.jpg\",width:144pt)",
    );
    let original = convert(&doc, None);
    let output = dir.path().join("test.pptx");
    let ir = dir.path().join("test.json");
    let status = Command::new(env!("CARGO_BIN_EXE_typptx"))
        .arg(dir.path().join("test.typ"))
        .arg("-o")
        .arg(&output)
        .args(["--image-dpi", "150", "--strict", "--dump-ir"])
        .arg(&ir)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let p: Presentation = serde_json::from_slice(&fs::read(ir).unwrap()).unwrap();
    assert_eq!(
        p.slides[0].elements.len(),
        original.slides[0].elements.len()
    );
    assert!(
        p.slides[0]
            .elements
            .iter()
            .any(|e| matches!(e, Element::Shape(_)))
    );
    assert!(
        p.slides[0]
            .elements
            .iter()
            .any(|e| matches!(e, Element::Text(t) if t.role == "list"))
    );
    for ((actual, _, _), (expected, _, _)) in pictures(&p).iter().zip(pictures(&original)) {
        for (a, b) in [
            (actual.x, expected.x),
            (actual.y, expected.y),
            (actual.width, expected.width),
            (actual.height, expected.height),
        ] {
            assert!((a - b).abs() < 1e-8, "image placement changed: {actual:?}");
        }
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(fs::read(output).unwrap())).unwrap();
    let names: Vec<_> = zip
        .file_names()
        .filter(|n| n.starts_with("ppt/media/"))
        .map(str::to_owned)
        .collect();
    assert_eq!(names.len(), 3);
    let mut dimensions = Vec::new();
    for name in names {
        let mut bytes = Vec::new();
        zip.by_name(&name).unwrap().read_to_end(&mut bytes).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            if name.ends_with(".jpg") || name.ends_with(".jpeg") {
                image::ImageFormat::Jpeg
            } else {
                image::ImageFormat::Png
            }
        );
        dimensions.push((decoded.width(), decoded.height()));
    }
    dimensions.sort();
    assert_eq!(dimensions, [(150, 75), (300, 150), (300, 150)]);
}

#[test]
fn transparent_png_downsampling_preserves_alpha_and_avoids_colour_fringes() {
    let dir = tempfile::tempdir().unwrap();
    let rgba = ImageBuffer::from_fn(400, 200, |x, _| {
        if x < 200 {
            Rgba([65535_u16, 0, 0, 0])
        } else {
            Rgba([0_u16, 0, 65535, 65535])
        }
    });
    rgba.save(dir.path().join("alpha.png")).unwrap();
    let doc = document(dir.path(), "#image(\"alpha.png\",width:144pt)");
    let p = convert(&doc, Some(50));
    let images = pictures(&p);
    let decoded = image::load_from_memory(images[0].2).unwrap();
    assert_eq!(decoded.color(), image::ColorType::Rgba16);
    let decoded = decoded.to_rgba16();
    assert_eq!(decoded.dimensions(), (100, 50));
    assert_eq!(decoded.get_pixel(0, 25)[3], 0);
    assert_eq!(decoded.get_pixel(99, 25)[3], 65535);
    let edge: Vec<_> = decoded
        .pixels()
        .filter(|p| p[3] > 100 && p[3] < 65400)
        .collect();
    assert!(!edge.is_empty());
    assert!(edge.iter().all(|p| p[0] == 0 && p[2] > 65000));
    // A grayscale profile must remain attached to grayscale pixels.
    let gray = ImageBuffer::from_fn(400, 200, |x, _| {
        image::LumaA([32768_u16, if x < 200 { 0 } else { 65535 }])
    });
    gray.save(dir.path().join("gray.png")).unwrap();
    let doc = document(dir.path(), "#image(\"gray.png\",width:144pt)");
    let p = convert(&doc, Some(50));
    let decoded = image::load_from_memory(pictures(&p)[0].2).unwrap();
    assert_eq!(decoded.color(), image::ColorType::La16);
    assert_eq!(
        decoded.to_luma_alpha16().get_pixel(99, 25).0,
        [32768, 65535]
    );
}

#[test]
fn resized_jpeg_uses_typst_orientation_and_keeps_icc_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let rgb: DynamicImage = ImageBuffer::from_fn(400, 200, |x, _| {
        if x < 200 {
            Rgb([255_u8, 0, 0])
        } else {
            Rgb([0_u8, 0, 255])
        }
    })
    .into();
    let mut bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 95);
    // Little-endian TIFF with a single orientation tag: rotate 90 degrees.
    encoder
        .set_exif_metadata(vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ])
        .unwrap();
    let profile = b"ICC metadata round-trip fixture";
    encoder.set_icc_profile(profile.to_vec()).unwrap();
    rgb.write_with_encoder(encoder).unwrap();
    fs::write(dir.path().join("rotated.jpg"), bytes).unwrap();
    let doc = document(dir.path(), "#image(\"rotated.jpg\",width:72pt)");
    let p = convert(&doc, Some(100));
    let images = pictures(&p);
    let decoded = image::load_from_memory(images[0].2).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (100, 200));
    assert!(decoded.get_pixel(50, 20)[0] > 240);
    assert!(decoded.get_pixel(50, 180)[2] > 240);
    use image::ImageDecoder;
    let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(images[0].2)).unwrap();
    assert_eq!(decoder.icc_profile().unwrap().unwrap(), profile);
    assert_eq!(
        decoder.orientation().unwrap(),
        image::metadata::Orientation::NoTransforms
    );
}

#[test]
fn fallback_png_uses_requested_dpi_while_svg_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let doc = document(
        dir.path(),
        "#set page(width:144pt,height:72pt,margin:10pt)\n#skew(ax:15deg)[Fallback]",
    );
    let mut svg = None;
    for dpi in [None, Some(72), Some(300)] {
        let p = lower::convert_with_options(
            &doc,
            &lower::Options {
                allow_image_fallback: true,
                image_dpi: dpi,
                ..Default::default()
            },
        )
        .unwrap();
        let mut count = 0;
        for e in &p.slides[0].elements {
            if let Element::Drawing {
                png,
                svg: current,
                bounds,
            } = e
            {
                count += 1;
                let decoded = image::load_from_memory(png).unwrap();
                // The fallback covers the failed text, not the whole page.
                assert!(bounds.width < 100. && bounds.height < 30.);
                let scale = f64::from(dpi.unwrap_or(144)) / 72.;
                let expected = (
                    (bounds.width * scale).round().max(1.) as u32,
                    (bounds.height * scale).round().max(1.) as u32,
                );
                assert_eq!((decoded.width(), decoded.height()), expected);
                if let Some(previous) = &svg {
                    assert_eq!(previous, current);
                } else {
                    svg = Some(current.clone());
                }
            }
        }
        assert_eq!(count, 1);
        pptx::write(&p).unwrap();
    }
    let error = lower::convert_with_options(
        &doc,
        &lower::Options {
            allow_image_fallback: true,
            image_dpi: Some(u32::MAX),
            ..Default::default()
        },
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("lower --image-dpi"));
    assert!(
        lower::convert_with_options(
            &doc,
            &lower::Options {
                image_dpi: Some(0),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn invalid_cli_dpi_leaves_existing_presentation_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("test.typ");
    let output = input.with_extension("pptx");
    fs::write(&input, "Source").unwrap();
    fs::write(&output, "Existing presentation").unwrap();
    for dpi in ["0", "-1", "NaN", "inf", "1.5", "4294967296"] {
        let status = Command::new(env!("CARGO_BIN_EXE_typptx"))
            .arg(&input)
            .arg(format!("--image-dpi={dpi}"))
            .output()
            .unwrap();
        assert!(!status.status.success(), "{dpi}");
        assert!(String::from_utf8_lossy(&status.stderr).contains("--image-dpi"));
        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "Existing presentation"
        );
    }
}
