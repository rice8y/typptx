use super::*;
use image::{DynamicImage, ImageBuffer};

impl NativePdf {
    pub(super) fn image(&mut self, image: pdf::Image<'_, '_>, transform: Affine) -> Result<()> {
        self.check_paint()?;
        let mut decoded = None;
        match image {
            pdf::Image::Raster(r) => r.with_rgba(
                |data, alpha| {
                    decoded = Some(rgba(data, alpha).and_then(|(image, scale)| {
                        self.place_image(
                            image,
                            transform * Affine::scale_non_uniform(scale.0.into(), scale.1.into()),
                        )
                    }));
                },
                None,
            ),
            pdf::Image::Stencil(s) => s.with_stencil(
                |mask, paint| {
                    decoded = Some((|| {
                        let pdf::Paint::Color(color) = paint else {
                            bail!("patterned PDF stencil images require compositing");
                        };
                        let color = color.to_rgba().to_rgba8();
                        let pixels = mask
                            .data
                            .iter()
                            .flat_map(|a| {
                                [
                                    color[0],
                                    color[1],
                                    color[2],
                                    (u16::from(*a) * u16::from(color[3]) / 255) as u8,
                                ]
                            })
                            .collect();
                        let image = ImageBuffer::from_raw(mask.width, mask.height, pixels)
                            .ok_or_else(|| anyhow!("invalid PDF stencil dimensions"))?;
                        self.place_image(
                            DynamicImage::ImageRgba8(image),
                            transform
                                * Affine::scale_non_uniform(
                                    mask.scale_factors.0.into(),
                                    mask.scale_factors.1.into(),
                                ),
                        )
                    })());
                },
                None,
            ),
        }
        decoded.ok_or_else(|| anyhow!("cannot decode PDF image"))?
    }

    fn place_image(&mut self, image: DynamicImage, transform: Affine) -> Result<()> {
        tiny_transform(transform)?;
        let [a, b, c, d, _, _] = transform.as_coeffs();
        let (sx, sy) = (a.hypot(b), c.hypot(d));
        // Store picture extents in points. Pixel-sized child shapes can be
        // downsampled by Office before the enclosing group enlarges them.
        let local = Rect {
            x: 0.,
            y: 0.,
            width: f64::from(image.width()) * sx,
            height: f64::from(image.height()) * sy,
        };
        let transform = transform * Affine::scale_non_uniform(1. / sx, 1. / sy);
        let inv = transform.inverse();
        let crop = self
            .clip()
            .iter()
            .map(|c| {
                c.iter()
                    .map(|p| {
                        let p = inv * kurbo::Point::new(p[0], p[1]);
                        [p.x, p.y]
                    })
                    .collect()
            })
            .collect();
        let crop = paths::intersect(&rectangle(local), &crop);
        if crop.is_empty() {
            return Ok(());
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        let raster = typst::visualize::RasterImage::plain(
            typst::foundations::Bytes::new(bytes.into_inner()),
            typst::visualize::ExchangeFormat::Png,
        )
        .map_err(|e| anyhow!("{e}"))?;
        let bytes = crate::assets::images::resample(&raster, local, "png", self.dpi)?;
        let clip = if paths::contains_rect(&crop, local) {
            None
        } else {
            paths::clip_commands(&crop, local)
        };
        self.elements.push(crate::geometry::transforms::apply(
            vec![Element::Picture {
                bounds: local,
                clip,
                extension: "png".into(),
                bytes,
            }],
            transform.as_coeffs(),
        ));
        Ok(())
    }
}

fn rgba(data: pdf::ImageData, alpha: Option<pdf::LumaData>) -> Result<(DynamicImage, (f32, f32))> {
    let scale = data.scale_factors();
    let (width, height) = (data.width(), data.height());
    let rgb = match data {
        pdf::ImageData::Rgb(rgb) => rgb.data,
        pdf::ImageData::Luma(gray) => gray.data.iter().flat_map(|v| [*v, *v, *v]).collect(),
    };
    if let Some(alpha) = alpha {
        let mut mask = image::GrayImage::from_raw(alpha.width, alpha.height, alpha.data)
            .ok_or_else(|| anyhow!("invalid PDF alpha dimensions"))?;
        if mask.dimensions() != (width, height) {
            mask = image::imageops::resize(
                &mask,
                width,
                height,
                if alpha.interpolate {
                    image::imageops::FilterType::Triangle
                } else {
                    image::imageops::FilterType::Nearest
                },
            );
        }
        let pixels = rgb
            .chunks_exact(3)
            .zip(mask.as_raw())
            .flat_map(|(rgb, a)| [rgb[0], rgb[1], rgb[2], *a])
            .collect();
        let image = ImageBuffer::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow!("invalid PDF image dimensions"))?;
        Ok((DynamicImage::ImageRgba8(image), scale))
    } else {
        let image = ImageBuffer::from_raw(width, height, rgb)
            .ok_or_else(|| anyhow!("invalid PDF image dimensions"))?;
        Ok((DynamicImage::ImageRgb8(image), scale))
    }
}
