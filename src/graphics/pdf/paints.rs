use super::*;
use pdf::pattern::{Pattern, ShadingPattern};
use pdf::shading::ShadingType;

impl NativePdf {
    pub(super) fn paint_path(
        &mut self,
        data: &tiny::Path,
        paint: &pdf::Paint<'_>,
        is_stroke: bool,
    ) -> Result<()> {
        match paint {
            pdf::Paint::Color(c) => {
                if let Some(shape) = paths::shape(
                    data,
                    [0., 0.],
                    Some(Brush::Solid {
                        color: c.to_rgba().to_rgba8(),
                    }),
                ) {
                    self.elements.push(Element::Shape(shape));
                }
            }
            pdf::Paint::Pattern(pattern) => match pattern.as_ref() {
                Pattern::Shading(s) => self.shading(data, s)?,
                Pattern::Tiling(p) => {
                    ensure!(
                        p.matrix.determinant().abs() > 1e-15,
                        "singular PDF pattern transform"
                    );
                    let region = paths::simplify(data, false);
                    let inverse = p.matrix.inverse();
                    let bounds = (inverse * path_from_tiny(data)).bounding_box();
                    let sx = f64::from(p.x_step.abs());
                    let sy = f64::from(p.y_step.abs());
                    ensure!(sx > 0. && sy > 0., "zero PDF pattern step");
                    let x0 = ((bounds.x0 - p.bbox.x1) / sx).floor() as i32;
                    let x1 = ((bounds.x1 - p.bbox.x0) / sx).ceil() as i32;
                    let y0 = ((bounds.y0 - p.bbox.y1) / sy).floor() as i32;
                    let y1 = ((bounds.y1 - p.bbox.y0) / sy).ceil() as i32;
                    ensure!(
                        (i64::from(x1) - i64::from(x0) + 1)
                            .saturating_mul(i64::from(y1) - i64::from(y0) + 1)
                            <= 16384,
                        "PDF pattern exceeds 16384 native tiles"
                    );
                    self.clips.push(region);
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            let transform = p.matrix
                                * Affine::translate((f64::from(x) * sx, f64::from(y) * sy));
                            p.interpret(self, transform, is_stroke)
                                .ok_or_else(|| anyhow!("cannot decode PDF pattern"))?;
                        }
                    }
                    self.flush_text();
                    self.clips.pop();
                }
            },
        }
        Ok(())
    }

    fn shading(&mut self, data: &tiny::Path, pattern: &ShadingPattern) -> Result<()> {
        let shading = &pattern.shading;
        if matches!(shading.shading_type.as_ref(), ShadingType::Dummy) {
            return Ok(());
        }
        let mut area = paths::simplify(data, false);
        if let Some(clip) = &shading.clip_path {
            let clip = tiny_path(clip).map_or_else(Vec::new, |p| paths::simplify(&p, false));
            area = paths::intersect(&area, &clip);
        }
        if area.is_empty() {
            return Ok(());
        }
        if let Some(background) = &shading.background {
            let color = shading
                .color_space
                .to_rgba(background, pattern.opacity, false)
                .to_rgba8();
            if let Some(shape) = paths::path(&area)
                .and_then(|p| paths::shape(&p, [0., 0.], Some(Brush::Solid { color })))
            {
                self.elements.push(Element::Shape(shape));
            }
        }
        let ShadingType::RadialAxial {
            coords,
            domain,
            function,
            extend,
            axial,
        } = shading.shading_type.as_ref()
        else {
            bail!("PDF function and mesh shadings require a native surface implementation");
        };
        let matrix = pattern.matrix;
        ensure!(
            matrix.determinant().abs() > 1e-15,
            "singular PDF shading transform"
        );
        let inverse = matrix.inverse();
        let sample = |t: f64| -> Result<[u8; 4]> {
            let v = domain[0] + (domain[1] - domain[0]) * t.clamp(0., 1.) as f32;
            let values = function
                .eval(&std::iter::once(v).collect())
                .ok_or_else(|| anyhow!("cannot evaluate PDF shading function"))?;
            let color = shading.color_space.to_rgba(&values, pattern.opacity, false);
            Ok(pattern
                .transfer_function
                .as_ref()
                .map_or(color, |f| f.apply(&color))
                .to_rgba8())
        };
        let coords = coords.map(f64::from);
        if *axial {
            let p0 = kurbo::Point::new(coords[0], coords[1]);
            let v = kurbo::Vec2::new(coords[2] - coords[0], coords[3] - coords[1]);
            let len = v.hypot2();
            ensure!(len > 1e-15, "zero PDF gradient vector");
            let values: Vec<_> = area
                .iter()
                .flatten()
                .map(|p| {
                    let q = inverse * kurbo::Point::new(p[0], p[1]) - p0;
                    (q.dot(v) / len, q.cross(v) / len)
                })
                .collect();
            let lo = values.iter().map(|v| v.0).fold(f64::INFINITY, f64::min);
            let hi = values.iter().map(|v| v.0).fold(f64::NEG_INFINITY, f64::max);
            let start = if extend[0] { lo } else { lo.max(0.) };
            let end = if extend[1] { hi } else { hi.min(1.) };
            if start >= end {
                return Ok(());
            }
            if !extend[0] || !extend[1] {
                let a = values.iter().map(|v| v.1).fold(f64::INFINITY, f64::min) - 1.;
                let b = values.iter().map(|v| v.1).fold(f64::NEG_INFINITY, f64::max) + 1.;
                let point = |t: f64, n: f64| {
                    let p = matrix * (p0 + v * t + kurbo::Vec2::new(v.y, -v.x) * n);
                    [p.x, p.y]
                };
                area = paths::intersect(
                    &area,
                    &vec![vec![
                        point(start, a),
                        point(end, a),
                        point(end, b),
                        point(start, b),
                    ]],
                );
            }
            let Some(mut shape) = paths::path(&area).and_then(|p| paths::shape(&p, [0., 0.], None))
            else {
                return Ok(());
            };
            let b = shape.bounds;
            let at = |x: f64, y: f64| ((inverse * kurbo::Point::new(x, y)) - p0).dot(v) / len;
            let values = [
                at(b.x, b.y),
                at(b.right(), b.y),
                at(b.right(), b.bottom()),
                at(b.x, b.bottom()),
            ];
            let lo = values.into_iter().fold(f64::INFINITY, f64::min);
            let hi = values.into_iter().fold(f64::NEG_INFINITY, f64::max);
            let span = hi - lo;
            ensure!(span > 1e-15, "degenerate PDF gradient extent");
            let mut stops = vec![(0., sample(lo)?), (1., sample(hi)?)];
            for i in 0..=128 {
                let t = f64::from(i) / 128.;
                if t > lo && t < hi {
                    stops.push(((t - lo) / span, sample(t)?));
                }
            }
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            let gx = at(b.right(), b.y) - at(b.x, b.y);
            let gy = at(b.x, b.bottom()) - at(b.x, b.y);
            shape.fill = Some(Brush::Linear {
                stops,
                angle: gy.atan2(gx).to_degrees(),
            });
            self.elements.push(Element::Shape(shape));
        } else {
            let [x0, y0, r0, x1, y1, r1] = coords;
            ensure!(
                r1 > r0 && (x1 - x0).hypot(y1 - y0) < r1 - r0 + 1e-6,
                "non-nested PDF radial gradients require a native surface implementation"
            );
            for (keep, r, x, y) in [(extend[1], r1, x1, y1), (extend[0], r0, x0, y0)] {
                if keep || r == 0. {
                    continue;
                }
                let circle = matrix * kurbo::Circle::new((x, y), r).to_path(0.02);
                let circle =
                    tiny_path(&circle).map_or_else(Vec::new, |p| paths::simplify(&p, false));
                area = if r == r1 {
                    paths::intersect(&area, &circle)
                } else {
                    paths::difference(&area, &circle)
                };
            }
            let Some(mut shape) = paths::path(&area).and_then(|p| paths::shape(&p, [0., 0.], None))
            else {
                return Ok(());
            };
            let b = shape.bounds;
            let [a, c, d, e, f, g] = matrix.as_coeffs();
            let stops = (0..=128)
                .map(|i| {
                    let t = f64::from(i) / 128.;
                    Ok((t, sample(t)?))
                })
                .collect::<Result<_>>()?;
            shape.fill = Some(Brush::Radial {
                stops,
                center: [x1, y1],
                focal: [x0, y0],
                radius: r1,
                inner_radius: r0,
                transform: Box::new([
                    a / b.width,
                    c / b.height,
                    d / b.width,
                    e / b.height,
                    (f - b.x) / b.width,
                    (g - b.y) / b.height,
                ]),
            });
            self.elements.push(Element::Shape(shape));
        }
        Ok(())
    }
}

fn path_from_tiny(path: &tiny::Path) -> BezPath {
    let mut out = BezPath::new();
    for s in path.segments() {
        match s {
            tiny::PathSegment::MoveTo(p) => out.move_to((f64::from(p.x), f64::from(p.y))),
            tiny::PathSegment::LineTo(p) => out.line_to((f64::from(p.x), f64::from(p.y))),
            tiny::PathSegment::QuadTo(c, p) => out.quad_to(
                (f64::from(c.x), f64::from(c.y)),
                (f64::from(p.x), f64::from(p.y)),
            ),
            tiny::PathSegment::CubicTo(a, b, p) => out.curve_to(
                (f64::from(a.x), f64::from(a.y)),
                (f64::from(b.x), f64::from(b.y)),
                (f64::from(p.x), f64::from(p.y)),
            ),
            tiny::PathSegment::Close => out.close_path(),
        }
    }
    out
}
