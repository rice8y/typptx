//! SVG is lowered from usvg's resolved tree into native shapes, text and pictures.
mod effects;
mod paint;
mod text;
mod text_offsets;
use crate::{
    geometry::paths::{self, Contours},
    ir::*,
};
use anyhow::{Result, anyhow, bail, ensure};
use usvg::tiny_skia_path::{self as tiny, Transform};

pub fn convert(tree: &usvg::Tree, bounds: Rect, dpi: Option<u32>) -> Result<Vec<Element>> {
    let viewport = vec![vec![
        [0., 0.],
        [bounds.width, 0.],
        [bounds.width, bounds.height],
        [0., bounds.height],
    ]];
    convert_clipped(tree, bounds, dpi, &viewport)
}
pub(crate) fn convert_clipped(
    tree: &usvg::Tree,
    bounds: Rect,
    dpi: Option<u32>,
    clip: &Contours,
) -> Result<Vec<Element>> {
    let scale = Transform::from_scale(
        (bounds.width / f64::from(tree.size().width())) as f32,
        (bounds.height / f64::from(tree.size().height())) as f32,
    );
    let mut out = Vec::new();
    walk_group(tree, tree.root(), scale, bounds, clip, &mut out, dpi)?;
    Ok(out)
}

fn walk_group(
    tree: &usvg::Tree,
    g: &usvg::Group,
    scale: Transform,
    bounds: Rect,
    parent_clip: &Contours,
    out: &mut Vec<Element>,
    dpi: Option<u32>,
) -> Result<()> {
    let effects = effects::convert(g, scale, parent_clip)?;
    ensure!(
        g.blend_mode() == usvg::BlendMode::Normal,
        "SVG blend modes require compositing"
    );
    let own_clip;
    let clip = if let Some(c) = g.clip_path() {
        own_clip = paths::intersect(
            parent_clip,
            &clip_path(c, scale.pre_concat(g.abs_transform()))?,
        );
        &own_clip
    } else {
        parent_clip
    };
    let masked;
    let clip = if let Some(mask) = g.mask() {
        masked = paths::intersect(
            clip,
            &binary_mask(mask, scale.pre_concat(g.abs_transform()))?,
        );
        &masked
    } else {
        clip
    };
    let start = out.len();
    for node in g.children() {
        match node {
            usvg::Node::Group(g) => walk_group(tree, g, scale, bounds, clip, out, dpi)?,
            usvg::Node::Path(p) if p.is_visible() => {
                out.extend(path(tree, p, scale, bounds, clip, dpi)?)
            }
            usvg::Node::Text(t) => out.extend(text::convert(tree, t, scale, bounds, clip)?),
            usvg::Node::Image(i) if i.is_visible() => {
                out.extend(image(i, scale, bounds, clip, dpi)?)
            }
            _ => {}
        }
    }
    for effect in effects {
        if out.len() > start {
            let mut element = Element::group(out.split_off(start));
            if let Element::Group(group) = &mut element {
                group.effect = Some(effect);
            }
            out.push(element);
        }
    }
    if g.opacity().get() < 1. && out.len() > start {
        let mut element = Element::group(out.split_off(start));
        if let Element::Group(group) = &mut element {
            group.opacity = f64::from(g.opacity().get());
        }
        out.push(element);
    }
    Ok(())
}
fn clip_path(clip: &usvg::ClipPath, ts: Transform) -> Result<Contours> {
    fn walk(g: &usvg::Group, ts: Transform) -> Result<Contours> {
        let mut result = Vec::new();
        for n in g.children() {
            let part = match n {
                usvg::Node::Path(p) => {
                    let data = p
                        .data()
                        .clone()
                        .transform(ts.pre_concat(p.abs_transform()))
                        .ok_or_else(|| anyhow!("invalid SVG clip transform"))?;
                    paths::simplify(
                        &data,
                        p.fill()
                            .is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd),
                    )
                }
                usvg::Node::Group(g) => walk(g, ts)?,
                usvg::Node::Text(t) => walk(t.flattened(), ts)?,
                _ => bail!("invalid image in SVG clip path"),
            };
            result = paths::union(&result, &part);
        }
        if let Some(c) = g.clip_path() {
            result = paths::intersect(&result, &clip_path(c, ts.pre_concat(g.abs_transform()))?);
        }
        Ok(result)
    }
    let ts = ts.pre_concat(clip.transform());
    let mut result = walk(clip.root(), ts)?;
    if let Some(c) = clip.clip_path() {
        result = paths::intersect(&result, &clip_path(c, ts)?);
    }
    Ok(result)
}
fn rect_path(r: Rect) -> Contours {
    vec![vec![
        [r.x, r.y],
        [r.right(), r.y],
        [r.right(), r.bottom()],
        [r.x, r.bottom()],
    ]]
}
fn contains(clip: &Contours, r: Rect) -> bool {
    let rect = rect_path(r);
    let intersection = paths::intersect(clip, &rect);
    fn area(c: &Contours) -> f64 {
        c.iter()
            .map(|p| {
                p.iter()
                    .zip(p.iter().cycle().skip(1))
                    .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
                    .sum::<f64>()
                    / 2.
            })
            .sum::<f64>()
            .abs()
    }
    (area(&intersection) - r.width * r.height).abs() < 0.001
}
fn path(
    tree: &usvg::Tree,
    p: &usvg::Path,
    scale: Transform,
    image: Rect,
    clip: &Contours,
    dpi: Option<u32>,
) -> Result<Vec<Element>> {
    let ts = scale.pre_concat(p.abs_transform());
    let data = p
        .data()
        .clone()
        .transform(ts)
        .ok_or_else(|| anyhow!("invalid SVG path transform"))?;
    let bounds = data
        .compute_tight_bounds()
        .ok_or_else(|| anyhow!("empty SVG path"))?;
    let b = Rect {
        x: f64::from(bounds.left()),
        y: f64::from(bounds.top()),
        width: f64::from(bounds.width()).max(0.01),
        height: f64::from(bounds.height()).max(0.01),
    };
    let mut filled = None;
    if let Some(fill) = p.fill() {
        let data = if p
            .fill()
            .is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd)
            || !contains(clip, b)
        {
            paths::path(&paths::intersect(
                &paths::simplify(
                    &data,
                    p.fill()
                        .is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd),
                ),
                clip,
            ))
        } else {
            Some(data.clone())
        };
        if let (Some(data), usvg::Paint::Pattern(pattern)) = (&data, fill.paint()) {
            let region = paths::simplify(data, false);
            filled = Some(with_opacity(
                pattern_fill(tree, pattern, ts, image, &region, dpi)?,
                fill.opacity().get(),
            ));
        } else if let Some(mut shape) = data.as_ref().and_then(|d| paths::shape(d, [0., 0.], None))
        {
            shape.fill = Some(paint::convert(
                fill.paint(),
                fill.opacity().get(),
                ts,
                shape.bounds,
            )?);
            shape.bounds.x += image.x;
            shape.bounds.y += image.y;
            filled = Some(Element::Shape(shape));
        }
    }
    let mut stroked = None;
    if let Some(s) = p.stroke() {
        let sx = ts.sx.hypot(ts.ky);
        let sy = ts.kx.hypot(ts.sy);
        let extent = p
            .abs_stroke_bounding_box()
            .transform(scale)
            .ok_or_else(|| anyhow!("invalid SVG stroke bounds"))?;
        let eb = Rect {
            x: f64::from(extent.left()),
            y: f64::from(extent.top()),
            width: f64::from(extent.width()),
            height: f64::from(extent.height()),
        };
        let simple = (sx - sy).abs() < 1e-5
            && (ts.sx * ts.kx + ts.ky * ts.sy).abs() < 1e-5
            && s.dashoffset().abs() < 1e-6
            && s.linejoin() != usvg::LineJoin::MiterClip
            && !matches!(s.paint(), usvg::Paint::Pattern(_))
            && contains(clip, eb);
        if simple {
            let mut shape = paths::shape(&data, [image.x, image.y], None).unwrap();
            shape.stroke = Some(Stroke {
                paint: paint::convert(s.paint(), s.opacity().get(), ts, b)?,
                width: f64::from(s.width().get() * sx),
                cap: match s.linecap() {
                    usvg::LineCap::Butt => "flat",
                    usvg::LineCap::Round => "rnd",
                    usvg::LineCap::Square => "sq",
                }
                .into(),
                join: match s.linejoin() {
                    usvg::LineJoin::Miter => "miter",
                    usvg::LineJoin::Round => "round",
                    _ => "bevel",
                }
                .into(),
                dash: s
                    .dasharray()
                    .unwrap_or_default()
                    .iter()
                    .map(|d| f64::from(*d * sx))
                    .collect(),
                miter_limit: f64::from(s.miterlimit().get()),
            });
            // Keep ordinary fill and stroke as one editable shape.
            if p.paint_order() == usvg::PaintOrder::FillAndStroke
                && contains(clip, b)
                && p.fill().is_none_or(|f| f.rule() == usvg::FillRule::NonZero)
                && matches!(filled, Some(Element::Shape(_)))
                && let Some(Element::Shape(f)) = filled.take()
            {
                shape.fill = f.fill;
            }
            stroked = Some(Element::Shape(shape));
        } else {
            let mut local = p.data().clone();
            if let Some(dash) = s
                .dasharray()
                .and_then(|d| tiny::StrokeDash::new(d.to_vec(), s.dashoffset()))
            {
                local = local
                    .dash(&dash, sx.max(sy))
                    .ok_or_else(|| anyhow!("cannot resolve SVG dash"))?;
            }
            let stroke = tiny::Stroke {
                width: s.width().get(),
                miter_limit: s.miterlimit().get(),
                line_cap: match s.linecap() {
                    usvg::LineCap::Butt => tiny::LineCap::Butt,
                    usvg::LineCap::Round => tiny::LineCap::Round,
                    usvg::LineCap::Square => tiny::LineCap::Square,
                },
                line_join: match s.linejoin() {
                    usvg::LineJoin::Miter => tiny::LineJoin::Miter,
                    usvg::LineJoin::MiterClip => tiny::LineJoin::MiterClip,
                    usvg::LineJoin::Round => tiny::LineJoin::Round,
                    usvg::LineJoin::Bevel => tiny::LineJoin::Bevel,
                },
                ..Default::default()
            };
            if let Some(outline) = local
                .stroke(&stroke, sx.max(sy))
                .and_then(|p| p.transform(ts))
            {
                let outline = if contains(clip, eb) {
                    Some(outline)
                } else {
                    paths::path(&paths::intersect(&paths::simplify(&outline, false), clip))
                };
                if let (Some(data), usvg::Paint::Pattern(pattern)) = (&outline, s.paint()) {
                    stroked = Some(with_opacity(
                        pattern_fill(tree, pattern, ts, image, &paths::simplify(data, false), dpi)?,
                        s.opacity().get(),
                    ));
                } else if let Some(mut shape) = outline
                    .as_ref()
                    .and_then(|d| paths::shape(d, [0., 0.], None))
                {
                    shape.fill = Some(paint::convert(
                        s.paint(),
                        s.opacity().get(),
                        ts,
                        shape.bounds,
                    )?);
                    shape.bounds.x += image.x;
                    shape.bounds.y += image.y;
                    stroked = Some(Element::Shape(shape));
                }
            }
        }
    }
    Ok(if p.paint_order() == usvg::PaintOrder::FillAndStroke {
        vec![filled, stroked]
    } else {
        vec![stroked, filled]
    }
    .into_iter()
    .flatten()
    .collect())
}

/// Opaque black/white (or alpha) masks are geometric clipping operations.
/// Soft masks require compositing and cannot become a native object clip.
fn binary_mask(mask: &usvg::Mask, ts: Transform) -> Result<Contours> {
    fn walk(
        group: &usvg::Group,
        ts: Transform,
        kind: usvg::MaskType,
        clip: &Contours,
        result: &mut Contours,
    ) -> Result<()> {
        ensure!(
            group.filters().is_empty() && group.mask().is_none() && group.opacity().get() == 1.,
            "soft or filtered SVG masks require compositing"
        );
        let own;
        let clip = if let Some(c) = group.clip_path() {
            own = paths::intersect(clip, &clip_path(c, ts.pre_concat(group.abs_transform()))?);
            &own
        } else {
            clip
        };
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => walk(g, ts, kind, clip, result)?,
                usvg::Node::Text(t) => walk(t.flattened(), ts, kind, clip, result)?,
                usvg::Node::Path(p) if p.is_visible() => {
                    ensure!(
                        p.stroke().is_none(),
                        "stroked SVG masks need an outlined mask"
                    );
                    let Some(fill) = p.fill() else { continue };
                    if fill.opacity().get() == 0. {
                        continue;
                    }
                    ensure!(
                        fill.opacity().get() == 1.,
                        "soft SVG masks require compositing"
                    );
                    let usvg::Paint::Color(c) = fill.paint() else {
                        bail!("gradient or patterned SVG masks require compositing")
                    };
                    let white = kind == usvg::MaskType::Alpha
                        || (c.red == 255 && c.green == 255 && c.blue == 255);
                    ensure!(
                        white || (c.red == 0 && c.green == 0 && c.blue == 0),
                        "luminance SVG masks require compositing"
                    );
                    let data = p
                        .data()
                        .clone()
                        .transform(ts.pre_concat(p.abs_transform()))
                        .ok_or_else(|| anyhow!("invalid mask transform"))?;
                    let area = paths::intersect(
                        &paths::simplify(&data, fill.rule() == usvg::FillRule::EvenOdd),
                        clip,
                    );
                    *result = if white {
                        paths::union(result, &area)
                    } else {
                        paths::difference(result, &area)
                    };
                }
                usvg::Node::Image(_) => bail!("raster SVG masks require compositing"),
                _ => (),
            }
        }
        Ok(())
    }
    let r = mask.rect();
    let mut corners = [
        tiny::Point::from_xy(r.x(), r.y()),
        tiny::Point::from_xy(r.right(), r.y()),
        tiny::Point::from_xy(r.right(), r.bottom()),
        tiny::Point::from_xy(r.x(), r.bottom()),
    ];
    ts.map_points(&mut corners);
    let clip = vec![
        corners
            .into_iter()
            .map(|p| [f64::from(p.x), f64::from(p.y)])
            .collect(),
    ];
    let mut out = Vec::new();
    walk(mask.root(), ts, mask.kind(), &clip, &mut out)?;
    if let Some(next) = mask.mask() {
        out = paths::intersect(&out, &binary_mask(next, ts)?);
    }
    Ok(out)
}

fn with_opacity(elements: Vec<Element>, opacity: f32) -> Element {
    let mut out = Element::group(elements);
    if let Element::Group(g) = &mut out {
        g.opacity = f64::from(opacity);
    }
    out
}

fn pattern_fill(
    tree: &usvg::Tree,
    pattern: &usvg::Pattern,
    ts: Transform,
    image: Rect,
    clip: &Contours,
    dpi: Option<u32>,
) -> Result<Vec<Element>> {
    if clip.is_empty() {
        return Ok(vec![]);
    }
    let ts = ts.pre_concat(pattern.transform());
    let inverse = ts
        .invert()
        .ok_or_else(|| anyhow!("singular SVG pattern transform"))?;
    let mut points = clip
        .iter()
        .flatten()
        .map(|p| tiny::Point::from_xy(p[0] as f32, p[1] as f32))
        .collect::<Vec<_>>();
    inverse.map_points(&mut points);
    let extent =
        tiny::Rect::from_points(&points).ok_or_else(|| anyhow!("invalid SVG pattern bounds"))?;
    let r = pattern.rect();
    let left = ((extent.left() - r.x()) / r.width()).floor() as i32;
    let right = ((extent.right() - r.x()) / r.width()).ceil() as i32;
    let top = ((extent.top() - r.y()) / r.height()).floor() as i32;
    let bottom = ((extent.bottom() - r.y()) / r.height()).ceil() as i32;
    ensure!(
        i64::from(right - left) * i64::from(bottom - top) <= 16384,
        "SVG pattern exceeds 16384 native tiles"
    );
    let mut out = Vec::new();
    for y in top..bottom {
        for x in left..right {
            let tile =
                ts.pre_translate(r.x() + x as f32 * r.width(), r.y() + y as f32 * r.height());
            let mut corners = [
                tiny::Point::from_xy(0., 0.),
                tiny::Point::from_xy(r.width(), 0.),
                tiny::Point::from_xy(r.width(), r.height()),
                tiny::Point::from_xy(0., r.height()),
            ];
            tile.map_points(&mut corners);
            let crop = paths::intersect(
                clip,
                &vec![
                    corners
                        .into_iter()
                        .map(|p| [f64::from(p.x), f64::from(p.y)])
                        .collect(),
                ],
            );
            if !crop.is_empty() {
                walk_group(tree, pattern.root(), tile, image, &crop, &mut out, dpi)?;
            }
        }
    }
    // Adjacent tiles of a single solid paint should have no shared antialiased
    // edges. Union them into one editable compound path without reordering
    // multicolor or overlapping tile contents.
    if let Some(Element::Shape(first)) = out.first()
        && matches!(first.fill, Some(Brush::Solid { .. }))
        && out
            .iter()
            .all(|e| matches!(e,Element::Shape(s) if s.fill==first.fill && s.stroke.is_none()))
    {
        let mut area = Vec::new();
        for e in &out {
            let Element::Shape(s) = e else { unreachable!() };
            if let Some(path) = paths::shape_path(s).and_then(|p| {
                p.transform(Transform::from_translate(
                    s.bounds.x as f32,
                    s.bounds.y as f32,
                ))
            }) {
                area = paths::union(&area, &paths::simplify(&path, false));
            }
        }
        if let Some(path) =
            paths::path(&area).and_then(|p| paths::shape(&p, [0., 0.], first.fill.clone()))
        {
            return Ok(vec![Element::Shape(path)]);
        }
    }
    Ok(out)
}
fn image(
    i: &usvg::Image,
    scale: Transform,
    root: Rect,
    clip: &Contours,
    dpi: Option<u32>,
) -> Result<Vec<Element>> {
    let extent = i
        .abs_bounding_box()
        .transform(scale)
        .ok_or_else(|| anyhow!("invalid SVG image bounds"))?;
    let b = Rect {
        x: f64::from(extent.left()),
        y: f64::from(extent.top()),
        width: f64::from(extent.width()),
        height: f64::from(extent.height()),
    };

    let local = Rect {
        x: 0.,
        y: 0.,
        width: f64::from(i.size().width()),
        height: f64::from(i.size().height()),
    };
    let ts = scale.pre_concat(i.abs_transform());
    let placed = Rect {
        width: local.width * f64::from(ts.sx.hypot(ts.ky)),
        height: local.height * f64::from(ts.kx.hypot(ts.sy)),
        ..local
    };
    let nested_dpi = dpi.map(|d| {
        (f64::from(d) * f64::from(ts.sx.hypot(ts.ky).max(ts.kx.hypot(ts.sy))))
            .ceil()
            .clamp(1., f64::from(u32::MAX)) as u32
    });
    let mut elements = match i.kind() {
        usvg::ImageKind::SVG(tree) => convert(tree, local, nested_dpi)?,
        kind => {
            let (extension, bytes) = match kind {
                usvg::ImageKind::PNG(b) => ("png", b.as_ref().clone()),
                usvg::ImageKind::JPEG(b) => ("jpg", b.as_ref().clone()),
                usvg::ImageKind::GIF(b) | usvg::ImageKind::WEBP(b) => {
                    let mut buf = std::io::Cursor::new(Vec::new());
                    image::load_from_memory(b)?.write_to(&mut buf, image::ImageFormat::Png)?;
                    ("png", buf.into_inner())
                }
                _ => unreachable!(),
            };
            let format = if extension == "jpg" {
                typst::visualize::ExchangeFormat::Jpg
            } else {
                typst::visualize::ExchangeFormat::Png
            };
            let raster =
                typst::visualize::RasterImage::plain(typst::foundations::Bytes::new(bytes), format)
                    .map_err(|e| anyhow!("{e}"))?;
            let bytes = crate::assets::images::resample(&raster, placed, extension, dpi)?;
            vec![Element::Picture {
                bounds: local,
                clip: None,
                extension: extension.into(),
                svg: None,
                bytes,
            }]
        }
    };
    if !contains(clip, b) {
        let inverse = ts
            .invert()
            .ok_or_else(|| anyhow!("singular picture transform"))?;
        let local_clip: Contours = clip
            .iter()
            .map(|c| {
                c.iter()
                    .map(|p| {
                        let mut p = tiny::Point::from_xy(p[0] as f32, p[1] as f32);
                        inverse.map_point(&mut p);
                        [f64::from(p.x), f64::from(p.y)]
                    })
                    .collect()
            })
            .collect();
        let crop = paths::intersect(&local_clip, &rect_path(local));
        if crop.is_empty() {
            return Ok(vec![]);
        }
        if let usvg::ImageKind::SVG(tree) = i.kind() {
            elements = convert_clipped(tree, local, nested_dpi, &crop)?;
        } else {
            for element in &mut elements {
                if let Element::Picture { clip, .. } = element {
                    *clip = paths::clip_commands(&crop, local);
                }
            }
        }
    }

    Ok(vec![crate::geometry::transforms::apply(
        elements,
        [
            f64::from(ts.sx),
            f64::from(ts.ky),
            f64::from(ts.kx),
            f64::from(ts.sy),
            root.x + f64::from(ts.tx),
            root.y + f64::from(ts.ty),
        ],
    )])
}

#[cfg(test)]
mod tests;
