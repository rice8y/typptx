//! Place raster images, SVG images, inline objects, and explicit fallbacks.
use crate::compiler::capture::{Capture, Kind, Leaf};
use crate::ir::*;
use anyhow::{Result, anyhow, bail};
use std::collections::BTreeMap;
use typst::layout::FrameItem;
use typst::visualize::{Color, Paint};

/// Reserve the realized advance of inline pictures with a native tab. Text
/// stays in its semantic paragraph and the picture remains an editable object.
pub(super) fn prepare_inline_objects(
    capture: &mut Capture,
    dpi: Option<u32>,
) -> Vec<BTreeMap<usize, Vec<Element>>> {
    use typst::{layout::Em, syntax::Span, text::Glyph};
    let mut objects: Vec<_> = (0..capture.pages.len()).map(|_| BTreeMap::new()).collect();
    for (page, output) in objects.iter_mut().enumerate() {
        for id in 0..capture.pages[page].len() {
            let leaf = &capture.pages[page][id];
            if capture.nearest(leaf, Kind::Equation).is_some()
                || capture.nearest(leaf, Kind::Label).is_some()
                || !leaf.plain_transform()
            {
                continue;
            }
            let Some(owner) = capture.nearest(leaf, Kind::Paragraph) else {
                continue;
            };
            let FrameItem::Image(image, size, _) = &leaf.item else {
                continue;
            };
            let Ok(elements) = native_image(leaf, image, *size, dpi) else {
                continue;
            };
            let width = size.x.to_pt() * leaf.scale();
            let baseline = leaf.position.1 + size.y.to_pt() * leaf.scale();
            let template = capture.nodes[owner].pages[&page]
                .leaves
                .iter()
                .filter_map(|&i| {
                    if let FrameItem::Text(t) = &capture.pages[page][i].item {
                        Some((i, t))
                    } else {
                        None
                    }
                })
                .min_by_key(|(i, _)| i.abs_diff(id))
                .map(|(_, t)| t.clone());
            let mut text = template.unwrap_or_else(crate::math::svg::empty_text);
            text.text = "\t".into();
            text.stroke = None;
            text.fill = Paint::Solid(Color::BLACK);
            text.glyphs = vec![Glyph {
                id: 0,
                x_advance: Em::new(width / text.size.to_pt()),
                x_offset: Em::zero(),
                y_advance: Em::zero(),
                y_offset: Em::zero(),
                range: 0..1,
                span: (Span::detached(), 0),
            }];
            capture.fix_inline_layout(owner);
            let leaf = &mut capture.pages[page][id];
            leaf.item = FrameItem::Text(text);
            leaf.position.1 = baseline;
            // Placeholder advances are already in slide coordinates.
            leaf.transform = typst::layout::Transform::identity();
            capture.inline_objects.insert((page, id));
            output.insert(id, elements);
        }
    }
    objects
}

pub(super) fn drawing(
    page: &typst_layout::Page,
    bounds: Rect,
    dpi: Option<u32>,
) -> Result<Element> {
    let svg = typst_svg::svg(page, &Default::default());
    let png = crate::assets::images::render_fallback(page, dpi)?;
    Ok(Element::Drawing { bounds, svg, png })
}

pub(super) fn native_image(
    leaf: &Leaf,
    image: &typst::visualize::Image,
    size: typst::layout::Size,
    dpi: Option<u32>,
) -> Result<Vec<Element>> {
    if !leaf.plain_transform() {
        let mut local = leaf.clone();
        local.position = (0., 0.);
        local.transform = typst::layout::Transform::identity();
        local.clipped = false;
        local.clips.clear();
        // Use the placed axis lengths for resampling, independent of rotation.
        let t = leaf.transform;
        let image_scale =
            t.sx.get()
                .hypot(t.ky.get())
                .max(t.kx.get().hypot(t.sy.get()));
        let local_dpi = dpi.map(|d| {
            (f64::from(d) * image_scale)
                .ceil()
                .clamp(1., f64::from(u32::MAX)) as u32
        });
        let mut elements = native_image(&local, image, size, local_dpi)?;
        if !leaf.clips.is_empty() {
            let t = leaf.transform;
            let m = usvg::tiny_skia_path::Transform::from_row(
                t.sx.get() as f32,
                t.ky.get() as f32,
                t.kx.get() as f32,
                t.sy.get() as f32,
                leaf.position.0 as f32,
                leaf.position.1 as f32,
            )
            .invert()
            .ok_or_else(|| anyhow!("singular picture transform"))?;
            let mut clip = vec![vec![
                [0., 0.],
                [size.x.to_pt(), 0.],
                [size.x.to_pt(), size.y.to_pt()],
                [0., size.y.to_pt()],
            ]];
            for source in &leaf.clips {
                let local: Vec<Vec<_>> = source
                    .iter()
                    .map(|c| {
                        c.iter()
                            .map(|p| {
                                let mut p =
                                    usvg::tiny_skia_path::Point::from_xy(p[0] as f32, p[1] as f32);
                                m.map_point(&mut p);
                                [f64::from(p.x), f64::from(p.y)]
                            })
                            .collect()
                    })
                    .collect();
                clip = crate::geometry::paths::intersect(&clip, &local);
            }
            if clip.is_empty() {
                return Ok(vec![]);
            }
            if let typst::visualize::ImageKind::Svg(svg) = image.kind() {
                elements = crate::graphics::svg::convert_clipped(
                    svg.tree(),
                    Rect {
                        x: 0.,
                        y: 0.,
                        width: size.x.to_pt(),
                        height: size.y.to_pt(),
                    },
                    dpi,
                    &clip,
                )?;
            } else {
                for element in &mut elements {
                    if let Element::Picture {
                        bounds, clip: mask, ..
                    } = element
                    {
                        *mask = crate::geometry::paths::clip_commands(&clip, *bounds);
                    }
                }
            }
        }
        let t = leaf.transform;
        return Ok(vec![crate::geometry::transforms::apply(
            elements,
            [
                t.sx.get(),
                t.ky.get(),
                t.kx.get(),
                t.sy.get(),
                leaf.position.0,
                leaf.position.1,
            ],
        )]);
    }
    let bounds = Rect {
        x: leaf.position.0,
        y: leaf.position.1,
        width: size.x.to_pt() * leaf.scale(),
        height: size.y.to_pt() * leaf.scale(),
    };
    match image.kind() {
        typst::visualize::ImageKind::Raster(raster) => {
            let data = raster.data();
            let extension = if data.starts_with(b"\x89PNG") {
                "png"
            } else if data.starts_with(&[0xff, 0xd8]) {
                "jpg"
            } else {
                "png"
            };
            Ok(vec![Element::Picture {
                bounds,
                clip: None,
                extension: extension.into(),
                bytes: crate::assets::images::resample(raster, bounds, extension, dpi)?,
            }])
        }
        typst::visualize::ImageKind::Svg(svg) => {
            crate::graphics::svg::convert(svg.tree(), bounds, dpi)
        }
        _ => bail!(
            "embedded vector assets require conversion to native shapes; SVG/PDF picture fallback is disabled"
        ),
    }
}
