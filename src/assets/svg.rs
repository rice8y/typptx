//! Office-compatible SVG within a single picture, without changing PPTX objects.
use anyhow::Result;
use base64::Engine;
use std::fmt::Write;

/// Office does not display SVG data URLs inside another SVG, and averages tiny
/// pixelated raster images. Expand nested SVGs and small pixel grids inside the
/// asset. All fonts must already be outlined by the source renderer.
pub(crate) fn compatible(source: &str) -> Result<String> {
    let tree = usvg::Tree::from_str(source, &usvg::Options::default())?;
    let svg = flatten(&tree, "asset_")?;
    // usvg also exports resource definitions from embedded image trees at the
    // outer level. Drop those unused copies after inlining their scoped trees.
    Ok(usvg::Tree::from_str(&svg, &usvg::Options::default())?
        .to_string(&usvg::WriteOptions::default()))
}

pub(crate) fn page(page: &typst_layout::Page) -> Result<String> {
    use typst::{
        foundations::Bytes,
        layout::{Frame, FrameItem},
        visualize::{Image, ImageKind, SvgImage},
    };
    fn images(frame: &Frame) -> Result<Frame> {
        let mut output = Frame::new(frame.size(), frame.kind());
        if frame.has_baseline() {
            output.set_baseline(frame.baseline());
        }
        for (position, item) in frame.items() {
            let item = match item {
                FrameItem::Group(group) => {
                    let mut group = group.clone();
                    group.frame = images(&group.frame)?;
                    FrameItem::Group(group)
                }
                FrameItem::Image(image, size, span)
                    if matches!(image.kind(), ImageKind::Svg(_)) =>
                {
                    let ImageKind::Svg(svg) = image.kind() else {
                        unreachable!()
                    };
                    let source = svg.tree().to_string(&usvg::WriteOptions::default());
                    let outlined = SvgImage::new(Bytes::new(source.into_bytes()))
                        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                    FrameItem::Image(
                        Image::new(outlined, image.alt().map(Into::into), image.scaling()),
                        *size,
                        *span,
                    )
                }
                _ => item.clone(),
            };
            output.push(*position, item);
        }
        Ok(output)
    }
    let mut page = page.clone();
    page.frame = images(&page.frame)?;
    compatible(&typst_svg::svg(&page, &Default::default()))
}

fn flatten(tree: &usvg::Tree, prefix: &str) -> Result<String> {
    let mut svg = tree.to_string(&usvg::WriteOptions {
        id_prefix: Some(prefix.into()),
        ..Default::default()
    });
    let document = roxmltree::Document::parse(&svg)?;
    let mut changes = Vec::new();
    for (index, node) in document
        .descendants()
        .filter(|n| n.tag_name().name() == "image")
        .enumerate()
    {
        let Some(href) = node.attribute(("http://www.w3.org/1999/xlink", "href")) else {
            continue;
        };
        let Some((mime, data)) = href.split_once(";base64,") else {
            continue;
        };
        let data = base64::engine::general_purpose::STANDARD.decode(data.trim())?;
        let replacement = if mime == "data:image/svg+xml" {
            let nested = usvg::Tree::from_data(&data, &usvg::Options::default())?;
            let nested = flatten(&nested, &format!("{prefix}{index}_"))?;
            let parsed = roxmltree::Document::parse(&nested)?;
            let root = parsed.root_element();
            root.children()
                .filter(|n| n.is_element())
                .map(|n| &nested[n.range()])
                .collect::<String>()
        } else {
            let pixelated = node.attribute("image-rendering") == Some("optimizeSpeed")
                || node
                    .attribute("style")
                    .is_some_and(|s| s.contains("pixelated") || s.contains("crisp-edges"));
            if !pixelated {
                continue;
            }
            let (width, height) = image::ImageReader::new(std::io::Cursor::new(&data))
                .with_guessed_format()?
                .into_dimensions()?;
            if u64::from(width) * u64::from(height) > 4096 {
                continue;
            }
            let image = image::load_from_memory(&data)?.to_rgba8();
            let mut paths = String::new();
            for y in 0..image.height() {
                let mut x = 0;
                while x < image.width() {
                    let pixel = image.get_pixel(x, y).0;
                    let start = x;
                    x += 1;
                    while x < image.width() && image.get_pixel(x, y).0 == pixel {
                        x += 1;
                    }
                    if pixel[3] == 0 {
                        continue;
                    }
                    write!(
                        paths,
                        "<path fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{}\" d=\"M{} {}h{}v1h-{}Z\"/>",
                        pixel[0],
                        pixel[1],
                        pixel[2],
                        f64::from(pixel[3]) / 255.,
                        start,
                        y,
                        x - start,
                        x - start
                    )?;
                }
            }
            paths
        };
        let mut group = String::from("<g");
        // usvg places image transforms and clips on surrounding groups. Retain
        // the remaining node identity/visibility when replacing its payload.
        for name in ["id", "visibility"] {
            if let Some(value) = node.attribute(name) {
                write!(
                    group,
                    " {name}=\"{}\"",
                    value.replace('&', "&amp;").replace('"', "&quot;")
                )?;
            }
        }
        group.push('>');
        group.push_str(&replacement);
        group.push_str("</g>");
        changes.push((node.range(), group));
    }
    for (range, replacement) in changes.into_iter().rev() {
        svg.replace_range(range, &replacement);
    }
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(svg: &str) -> Vec<u8> {
        use typst::{
            foundations::{Bytes, Content, Smart},
            layout::{Abs, Frame, FrameItem, Point, Size},
            syntax::Span,
            visualize::{Image, SvgImage},
        };
        let image = SvgImage::new(Bytes::new(svg.as_bytes().to_vec())).unwrap();
        let size = Size::new(Abs::pt(100.), Abs::pt(60.));
        let mut frame = Frame::hard(size);
        frame.push(
            Point::zero(),
            FrameItem::Image(Image::plain(image), size, Span::detached()),
        );
        let page = typst_layout::Page {
            frame,
            fill: Smart::Custom(None),
            bleed: Default::default(),
            numbering: None,
            supplement: Content::empty(),
            number: 1,
        };
        typst_render::render(&page, &Default::default())
            .data()
            .to_vec()
    }
    #[test]
    fn nested_svg_clips_and_ids_keep_pixels_without_embedded_svg_urls() {
        let child = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><defs><clipPath id="clip"><rect width="5" height="10"/></clipPath></defs><rect width="10" height="10" fill="red" clip-path="url(#clip)"/></svg>"##;
        let encoded = base64::engine::general_purpose::STANDARD.encode(child);
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="60"><image xlink:href="data:image/svg+xml;base64,{encoded}" x="10" y="10" width="30" height="30"/><image xlink:href="data:image/svg+xml;base64,{encoded}" x="60" y="10" width="20" height="20"/></svg>"#
        );
        let output = compatible(&source).unwrap();
        assert!(!output.contains("data:image/svg"));
        assert_eq!(render(&source), render(&output));
        let xml = roxmltree::Document::parse(&output).unwrap();
        let ids: Vec<_> = xml
            .descendants()
            .filter_map(|n| n.attribute("id"))
            .collect();
        assert_eq!(
            ids.len(),
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            "duplicate IDs: {ids:?}\n{output}"
        );
    }
    #[test]
    fn tiny_pixelated_images_keep_colors_alpha_and_transforms() {
        let pixels = image::RgbaImage::from_raw(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 0, 0, 0, 255, 128, 255, 255, 255, 255,
            ],
        )
        .unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.into_inner());
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="60"><g transform="translate(10 10) scale(20)"><image width="2" height="2" style="image-rendering:pixelated" xlink:href="data:image/png;base64,{encoded}"/></g></svg>"#
        );
        let output = compatible(&source).unwrap();
        assert!(!output.contains("<image"));
        assert_eq!(render(&source), render(&output));
    }
}
