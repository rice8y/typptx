//! Convert the compiler's vector primitives directly to editable DrawingML.
pub(crate) mod gradients;
pub(crate) mod pdf;
pub(crate) mod svg;
pub(crate) mod tiling;

use crate::{compiler::capture::Leaf, ir::*};
use anyhow::{Result, bail, ensure};
use typst::{
    layout::Point,
    visualize::{Color, CurveItem, Geometry, Gradient, LineCap, LineJoin, Paint},
};

pub fn brush(paint: &Paint) -> Result<Brush> {
    Ok(match paint {
        Paint::Solid(c) => Brush::Solid {
            color: rgba(c.clone()),
        },
        Paint::Gradient(g) => {
            // Sample perceptual interpolation; Office uses sRGB interpolation.
            let stops: Vec<_> = (0..=16)
                .map(|i| {
                    let t = f64::from(i) / 16.;
                    (
                        t,
                        rgba(g.sample(typst::visualize::RatioOrAngle::Ratio(
                            typst::layout::Ratio::new(t),
                        ))),
                    )
                })
                .collect();
            ensure!(
                matches!(g, Gradient::Linear(_)) || stops.iter().all(|s| s.1[3] == stops[0].1[3]),
                "PowerPoint introduces seams in radial/conic gradients with varying opacity; use a constant opacity or explicit image fallback"
            );
            match g {
                Gradient::Linear(_) => Brush::Linear {
                    stops,
                    angle: g.angle().unwrap().to_deg(),
                },
                Gradient::Radial(_) => {
                    let center = g.center().unwrap();
                    let focal = g.focal_center().unwrap();
                    Brush::Radial {
                        stops,
                        center: [center.x.get(), center.y.get()],
                        radius: g.radius().unwrap().get(),
                        focal: [focal.x.get(), focal.y.get()],
                        inner_radius: g.focal_radius().unwrap().get(),
                        transform: Box::new([1., 0., 0., 1., 0., 0.]),
                    }
                }
                Gradient::Conic(_) => {
                    let center = g.center().unwrap();
                    Brush::Conic {
                        stops,
                        center: [center.x.get(), center.y.get()],
                        angle: g.angle().unwrap().to_rad(),
                    }
                }
            }
        }
        _ => bail!("tiling paints require a native pattern implementation"),
    })
}

pub fn shape(leaf: &Leaf) -> Result<VectorShape> {
    use typst::layout::FrameItem;
    let FrameItem::Shape(shape, _) = &leaf.item else {
        bail!("expected shape")
    };
    ensure!(!leaf.clipped, "clipped paths are not supported yet");
    let local: Vec<CurveItem> = match &shape.geometry {
        Geometry::Curve(curve) => curve.0.clone(),
        Geometry::Rect(size) => typst::visualize::Curve::rect(*size).0,
        Geometry::Line(end) => vec![CurveItem::Move(Point::zero()), CurveItem::Line(*end)],
    };
    let point = |p: Point| {
        let t = &leaf.transform;
        [
            leaf.position.0 + t.sx.get() * p.x.to_pt() + t.kx.get() * p.y.to_pt(),
            leaf.position.1 + t.ky.get() * p.x.to_pt() + t.sy.get() * p.y.to_pt(),
        ]
    };
    let mut points = Vec::new();
    let mut commands: Vec<_> = local
        .iter()
        .map(|cmd| match cmd {
            CurveItem::Move(p) => {
                let p = point(*p);
                points.push(p);
                PathCommand::Move(p)
            }
            CurveItem::Line(p) => {
                let p = point(*p);
                points.push(p);
                PathCommand::Line(p)
            }
            CurveItem::Cubic(a, b, c) => {
                let pts = [point(*a), point(*b), point(*c)];
                points.extend(pts);
                PathCommand::Cubic(pts)
            }
            CurveItem::Close => PathCommand::Close,
        })
        .collect();
    ensure!(!points.is_empty(), "empty vector path");
    let x = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let y = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let right = points
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = points
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    for command in &mut commands {
        let adjust = |p: &mut [f64; 2]| {
            p[0] -= x;
            p[1] -= y;
        };
        match command {
            PathCommand::Move(p) | PathCommand::Line(p) => adjust(p),
            PathCommand::Cubic(pts) => pts.iter_mut().for_each(adjust),
            _ => {}
        }
    }
    let stroke = shape
        .stroke
        .as_ref()
        .map(|s| -> Result<Stroke> {
            let scale = (leaf.transform.sx.get() * leaf.transform.sy.get()
                - leaf.transform.kx.get() * leaf.transform.ky.get())
            .abs()
            .sqrt();
            stroke(s, scale)
        })
        .transpose()?;
    Ok(VectorShape {
        bounds: Rect {
            x,
            y,
            width: (right - x).max(0.01),
            height: (bottom - y).max(0.01),
        },
        commands,
        fill: shape.fill.as_ref().map(brush).transpose()?,
        stroke,
    })
}

pub(crate) fn stroke(s: &typst::visualize::FixedStroke, scale: f64) -> Result<Stroke> {
    Ok(Stroke {
        paint: brush(&s.paint)?,
        width: s.thickness.to_pt() * scale,
        cap: match s.cap {
            LineCap::Butt => "flat",
            LineCap::Round => "rnd",
            LineCap::Square => "sq",
        }
        .into(),
        join: match s.join {
            LineJoin::Miter => "miter",
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
        }
        .into(),
        dash: s.dash.as_ref().map_or(Vec::new(), |d| {
            d.array.iter().map(|a| a.to_pt() * scale).collect()
        }),
        miter_limit: s.miter_limit.get(),
    })
}

/// Resolve nonuniform strokes and clipped geometry through established path
/// stroking/boolean libraries. The result is still editable vector geometry.
pub fn convert(leaf: &Leaf) -> Result<Vec<Element>> {
    convert_with_dpi(leaf, None)
}

pub(crate) fn convert_with_dpi(leaf: &Leaf, dpi: Option<u32>) -> Result<Vec<Element>> {
    convert_with_options(
        leaf,
        &crate::lower::Options {
            image_dpi: dpi,
            ..Default::default()
        },
    )
}

pub(crate) fn convert_with_options(
    leaf: &Leaf,
    options: &crate::lower::Options,
) -> Result<Vec<Element>> {
    use typst::layout::{FrameItem, Transform};
    use usvg::tiny_skia_path as tiny;
    let FrameItem::Shape(s, _) = &leaf.item else {
        bail!("expected shape")
    };
    if s.fill
        .as_ref()
        .is_some_and(|p| matches!(p, Paint::Tiling(_)))
        || s.stroke
            .as_ref()
            .is_some_and(|s| matches!(s.paint, Paint::Tiling(_)))
    {
        return crate::graphics::tiling::convert(leaf, options);
    }
    let gradient = s
        .fill
        .as_ref()
        .is_some_and(|p| matches!(p, Paint::Gradient(_)))
        || s.stroke
            .as_ref()
            .is_some_and(|s| matches!(s.paint, Paint::Gradient(_)));
    let t = leaf.transform;
    if gradient
        && ((t.sx.get() - 1.).abs() > 1e-6
            || (t.sy.get() - 1.).abs() > 1e-6
            || t.kx.get().abs() > 1e-6
            || t.ky.get().abs() > 1e-6)
    {
        let affine = tiny::Transform::from_row(
            t.sx.get() as f32,
            t.ky.get() as f32,
            t.kx.get() as f32,
            t.sy.get() as f32,
            leaf.position.0 as f32,
            leaf.position.1 as f32,
        );
        let inverse = affine
            .invert()
            .ok_or_else(|| anyhow::anyhow!("singular gradient transform"))?;
        let mut local = leaf.clone();
        local.position = (0., 0.);
        local.transform = Transform::identity();
        for clip in &mut local.clips {
            for p in clip.iter_mut().flatten() {
                let mut q = tiny::Point::from_xy(p[0] as f32, p[1] as f32);
                inverse.map_point(&mut q);
                *p = [f64::from(q.x), f64::from(q.y)];
            }
        }
        return Ok(vec![crate::geometry::transforms::apply(
            convert_with_options(&local, options)?,
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

    let sx = t.sx.get().hypot(t.ky.get());
    let sy = t.kx.get().hypot(t.sy.get());
    let complex = leaf.clipped
        || s.fill_rule == typst::visualize::FillRule::EvenOdd
        || s.stroke.as_ref().is_some_and(|s| {
            (sx - sy).abs() > 1e-6
                || (t.sx.get() * t.kx.get() + t.ky.get() * t.sy.get()).abs() > 1e-6
                || s.dash
                    .as_ref()
                    .is_some_and(|d| d.phase.to_pt().abs() > 1e-6)
        });
    if !complex {
        return Ok(vec![Element::Shape(shape(leaf)?)]);
    }
    let curve = match &s.geometry {
        Geometry::Curve(c) => c.clone(),
        Geometry::Rect(size) => typst::visualize::Curve::rect(*size),
        Geometry::Line(end) => {
            typst::visualize::Curve(vec![CurveItem::Move(Point::zero()), CurveItem::Line(*end)])
        }
    };
    let local = crate::geometry::paths::curve(&curve, Transform::identity())
        .ok_or_else(|| anyhow::anyhow!("empty path"))?;
    let ts = tiny::Transform::from_row(
        t.sx.get() as f32,
        t.ky.get() as f32,
        t.kx.get() as f32,
        t.sy.get() as f32,
        leaf.position.0 as f32,
        leaf.position.1 as f32,
    );
    let mut out = Vec::new();
    let mut add = |path: tiny::Path, paint: &Paint, even_odd: bool| -> Result<()> {
        let path = path
            .transform(ts)
            .ok_or_else(|| anyhow::anyhow!("singular path transform"))?;
        let original = path
            .compute_tight_bounds()
            .ok_or_else(|| anyhow::anyhow!("empty gradient geometry"))?;
        let original = Rect {
            x: f64::from(original.left()),
            y: f64::from(original.top()),
            width: f64::from(original.width()).max(0.01),
            height: f64::from(original.height()).max(0.01),
        };
        let path = if leaf.clips.is_empty() && !even_odd {
            Some(path)
        } else {
            let mut region = crate::geometry::paths::simplify(&path, even_odd);
            for clip in &leaf.clips {
                region = crate::geometry::paths::intersect(&region, clip);
            }
            crate::geometry::paths::path(&region)
        };
        if let Some(path) = path
            && let Some(shape) = crate::geometry::paths::shape(&path, [0., 0.], Some(brush(paint)?))
        {
            let mut shape = shape;
            shape.fill = shape
                .fill
                .map(|p| crate::graphics::gradients::remap(p, original, shape.bounds));
            out.push(Element::Shape(shape));
        }
        Ok(())
    };
    if let Some(paint) = &s.fill {
        add(
            local.clone(),
            paint,
            s.fill_rule == typst::visualize::FillRule::EvenOdd,
        )?;
    }
    if let Some(stroke) = &s.stroke {
        let mut path = local;
        if let Some(d) = &stroke.dash
            && let Some(dash) = tiny::StrokeDash::new(
                d.array.iter().map(|v| v.to_pt() as f32).collect(),
                d.phase.to_pt() as f32,
            )
        {
            path = path
                .dash(&dash, sx.max(sy) as f32)
                .ok_or_else(|| anyhow::anyhow!("invalid stroke dash"))?;
        }
        let tiny = tiny::Stroke {
            width: stroke.thickness.to_pt() as f32,
            miter_limit: stroke.miter_limit.get() as f32,
            line_cap: match stroke.cap {
                LineCap::Butt => tiny::LineCap::Butt,
                LineCap::Round => tiny::LineCap::Round,
                LineCap::Square => tiny::LineCap::Square,
            },
            line_join: match stroke.join {
                LineJoin::Miter => tiny::LineJoin::Miter,
                LineJoin::Round => tiny::LineJoin::Round,
                LineJoin::Bevel => tiny::LineJoin::Bevel,
            },
            ..Default::default()
        };
        if let Some(path) = path.stroke(&tiny, sx.max(sy) as f32) {
            add(path, &stroke.paint, false)?;
        }
    }
    Ok(out)
}

pub fn rgba(color: Color) -> [u8; 4] {
    let c = color.to_rgb();
    [c.red, c.green, c.blue, c.alpha].map(|x| (x * 255.0).round() as u8)
}
