//! Pixel density affects raster assets, never native text, math, or shapes.
use crate::ir::Rect;
use anyhow::{Result, ensure};
use image::imageops::FilterType;
use image::{DynamicImage, ImageEncoder, codecs::jpeg::JpegEncoder, codecs::png::PngEncoder};
use typst::visualize::RasterImage;

/// Keep an imported SVG or PDF page as a single vector picture. Outlining SVG
/// text makes its appearance independent of the fonts installed in Office.
pub fn vector(
    image: &typst::visualize::Image,
    bounds: Rect,
    dpi: Option<u32>,
) -> Result<crate::ir::Element> {
    use typst::{
        foundations::{Content, Smart},
        layout::{Abs, Frame, FrameItem, Point, Sides, Size},
        syntax::Span,
        visualize::ImageKind,
    };
    let svg = match image.kind() {
        ImageKind::Svg(svg) => {
            super::svg::compatible(&svg.tree().to_string(&usvg::WriteOptions::default()))?
        }
        ImageKind::Pdf(_) => super::svg::compatible(&String::from_utf8(
            typst_svg::WebImage::new(image).data.to_vec(),
        )?)?,
        ImageKind::Raster(_) => unreachable!("raster images retain their original encoding"),
    };
    let size = Size::new(Abs::pt(bounds.width), Abs::pt(bounds.height));
    let mut frame = Frame::hard(size);
    frame.push(
        Point::zero(),
        FrameItem::Image(image.clone(), size, Span::detached()),
    );
    let page = typst_layout::Page {
        frame,
        bleed: Sides::splat(Abs::zero()),
        fill: Smart::Custom(None),
        numbering: None,
        supplement: Content::empty(),
        number: 1,
    };
    Ok(crate::ir::Element::Picture {
        bounds,
        clip: None,
        extension: "png".into(),
        alt: image.alt().map(str::to_owned),
        bytes: render_fallback(&page, dpi)?,
        svg: Some(svg),
    })
}

pub fn resample(
    raster: &RasterImage,
    bounds: Rect,
    extension: &str,
    dpi: Option<u32>,
) -> Result<Vec<u8>> {
    let original_encoding = match extension {
        "png" => raster.data().starts_with(b"\x89PNG"),
        "jpg" => raster.data().starts_with(&[0xff, 0xd8]),
        _ => false,
    };
    let width = f64::from(raster.width());
    let height = f64::from(raster.height());
    let scale = dpi.map_or(1., |dpi| {
        let target_width = (bounds.width * f64::from(dpi) / 72.).ceil().max(1.);
        let target_height = (bounds.height * f64::from(dpi) / 72.).ceil().max(1.);
        (target_width / width).min(target_height / height).min(1.)
    });
    let target = (
        (width * scale).round().max(1.) as u32,
        (height * scale).round().max(1.) as u32,
    );
    let unchanged = target == (raster.width(), raster.height());
    if unchanged && original_encoding {
        return Ok(raster.data().to_vec());
    }
    // Typst already decoded this image and applied EXIF orientation.
    let resized = if unchanged {
        raster.dynamic().as_ref().clone()
    } else {
        resize(raster.dynamic(), target)
    };
    let mut bytes = Vec::new();
    match extension {
        "jpg" => {
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 95);
            if let Some(icc) = raster.icc() {
                encoder.set_icc_profile(icc.to_vec())?;
            }
            resized.write_with_encoder(encoder)?;
        }
        "png" => {
            let mut encoder = PngEncoder::new(&mut bytes);
            if let Some(icc) = raster.icc() {
                encoder.set_icc_profile(icc.to_vec())?;
            }
            resized.write_with_encoder(encoder)?;
        }
        _ => unreachable!("native raster formats are checked before resampling"),
    }
    Ok(bytes)
}

fn resize(image: &DynamicImage, (width, height): (u32, u32)) -> DynamicImage {
    if !image.color().has_alpha() {
        return image.resize_exact(width, height, FilterType::Lanczos3);
    }
    // Filter premultiplied colours so invisible pixels cannot leave coloured
    // fringes along transparent edges. Keep 16-bit PNG precision when present.
    let mut premultiplied = image.to_rgba32f();
    for pixel in premultiplied.pixels_mut() {
        for channel in 0..3 {
            pixel[channel] *= pixel[3];
        }
    }
    let mut resized = image::imageops::resize(&premultiplied, width, height, FilterType::Lanczos3);
    for pixel in resized.pixels_mut() {
        for channel in 0..3 {
            pixel[channel] = if pixel[3] > 0.000001 {
                pixel[channel] / pixel[3]
            } else {
                0.
            };
        }
    }
    let resized = DynamicImage::ImageRgba32F(resized);
    match image.color() {
        image::ColorType::La8 => resized.to_luma_alpha8().into(),
        image::ColorType::La16 => resized.to_luma_alpha16().into(),
        image::ColorType::Rgba16 => resized.to_rgba16().into(),
        _ => resized.to_rgba8().into(),
    }
}

pub fn render_fallback(page: &typst_layout::Page, dpi: Option<u32>) -> Result<Vec<u8>> {
    let dpi = dpi.unwrap_or(144);
    let pixels_per_point = f64::from(dpi) / 72.;
    let size = page.frame.size();
    let width = (size.x.to_pt() * pixels_per_point).round().max(1.);
    let height = (size.y.to_pt() * pixels_per_point).round().max(1.);
    // Reject excessive requests before Typst allocates a full-page RGBA buffer.
    ensure!(
        width.is_finite() && height.is_finite() && width * height <= 100_000_000.,
        "fallback PNG at {dpi} DPI exceeds 100 million pixels; lower --image-dpi"
    );
    let options = typst_render::RenderOptions {
        pixel_per_pt: pixels_per_point.into(),
        ..Default::default()
    };
    Ok(typst_render::render(page, &options).encode_png()?)
}
