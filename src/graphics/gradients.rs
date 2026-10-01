//! Curved gradients become grouped native paths. This avoids Office's radial
//! radius/tiling bugs and preserves alpha without rasterizing the object.
use crate::{
    geometry::paths::{self, Contours},
    ir::*,
};
use std::f64::consts::{PI, TAU};
use usvg::tiny_skia_path as tiny;

pub fn nonlinear(paint: &Brush) -> bool {
    matches!(paint, Brush::Radial { .. } | Brush::Conic { .. })
}
fn sample(stops: &[(f64, [u8; 4])], t: f64) -> [u8; 4] {
    let right = stops.partition_point(|(s, _)| *s < t).min(stops.len() - 1);
    let left = right.saturating_sub(1);
    let (a, c) = stops[left];
    let (b, d) = stops[right];
    let u = if b <= a {
        0.
    } else {
        ((t - a) / (b - a)).clamp(0., 1.)
    };
    std::array::from_fn(|i| {
        (f64::from(c[i]) + (f64::from(d[i]) - f64::from(c[i])) * u).round() as u8
    })
}
fn path(s: &VectorShape) -> Option<tiny::Path> {
    let mut b = tiny::PathBuilder::new();
    for c in &s.commands {
        match c {
            PathCommand::Move(p) => b.move_to(p[0] as f32, p[1] as f32),
            PathCommand::Line(p) => b.line_to(p[0] as f32, p[1] as f32),
            PathCommand::Cubic(p) => b.cubic_to(
                p[0][0] as f32,
                p[0][1] as f32,
                p[1][0] as f32,
                p[1][1] as f32,
                p[2][0] as f32,
                p[2][1] as f32,
            ),
            PathCommand::Close => b.close(),
        }
    }
    b.finish()
}
fn solid_path(region: &Contours, bounds: Rect, color: [u8; 4]) -> Option<Element> {
    if color[3] == 0 {
        return None;
    }
    paths::path(region)
        .and_then(|p| paths::shape(&p, [bounds.x, bounds.y], Some(Brush::Solid { color })))
        .map(Element::Shape)
}
fn fill(s: &VectorShape, paint: &Brush) -> Vec<Element> {
    let Some(path) = path(s) else {
        return vec![];
    };
    let area = paths::simplify(&path, false);
    let stops = match paint {
        Brush::Radial { stops, .. } | Brush::Conic { stops, .. } => stops,
        _ => unreachable!(),
    };
    let flat_alpha = stops
        .first()
        .map(|s| s.1[3])
        .filter(|alpha| stops.iter().all(|s| s.1[3] == *alpha));
    let mut out = Vec::new();
    let mut emit = |region: Contours, mut color: [u8; 4]| {
        if flat_alpha.is_some() {
            color[3] = 255;
        }
        if let Some(e) = solid_path(&region, s.bounds, color) {
            out.push(e);
        }
    };
    match paint {
        Brush::Radial {
            stops,
            center,
            radius,
            focal,
            inner_radius,
            transform: m,
        } => {
            // Sample in gradient coordinates before applying the original affine
            // transform; nonuniform scale and offset focus remain exact geometry.
            let points = ((s.bounds.width.max(s.bounds.height) / 0.04).sqrt() * PI)
                .ceil()
                .clamp(64., 2048.) as usize;
            let contour = |t: f64| -> Contours {
                vec![
                    (0..points)
                        .map(|i| {
                            let a = TAU * i as f64 / points as f64;
                            let u = [a.cos(), a.sin()];
                            let r = inner_radius + (radius - inner_radius) * t;
                            let p = [
                                focal[0] + (center[0] - focal[0]) * t + r * u[0],
                                focal[1] + (center[1] - focal[1]) * t + r * u[1],
                            ];
                            [
                                (m[0] * p[0] + m[2] * p[1] + m[4]) * s.bounds.width,
                                (m[1] * p[0] + m[3] * p[1] + m[5]) * s.bounds.height,
                            ]
                        })
                        .collect(),
                ]
            };
            let mut outer = contour(1.);
            emit(
                if flat_alpha.is_some() {
                    area.clone()
                } else {
                    paths::difference(&area, &outer)
                },
                sample(stops, 1.),
            );
            let mut positions: Vec<_> = (0..=256)
                .map(|i| f64::from(i) / 256.)
                .chain(stops.iter().map(|s| s.0))
                .collect();
            positions.sort_by(f64::total_cmp);
            positions.dedup();
            for pair in positions.windows(2).rev() {
                let inner = contour(pair[0]);
                // Opaque nested fills share no antialiased internal edges.
                let ring = if flat_alpha.is_some() {
                    outer.clone()
                } else {
                    paths::difference(&outer, &inner)
                };
                emit(
                    paths::intersect(&area, &ring),
                    sample(stops, (pair[0] + pair[1]) * 0.5),
                );
                outer = inner;
            }
            emit(paths::intersect(&area, &outer), sample(stops, 0.));
        }
        Brush::Conic {
            stops,
            center,
            angle,
        } => {
            let cx = center[0] * s.bounds.width;
            let cy = center[1] * s.bounds.height;
            let radius = (cx
                .abs()
                .max((s.bounds.width - cx).abs())
                .hypot(cy.abs().max((s.bounds.height - cy).abs()))
                * 2.)
                .max(1.);
            for i in 0..720 {
                let a = TAU * f64::from(i) / 720. + angle - PI;
                // Cumulative sectors cover the preceding color, so Office's
                // antialiased polygon edges cannot expose the background.
                let b = if flat_alpha.is_some() {
                    TAU + angle - PI
                } else {
                    TAU * f64::from(i + 1) / 720. + angle - PI
                };
                let steps = ((b - a) / (PI / 8.)).ceil().max(1.) as usize;
                let mut polygon = vec![[cx, cy]];
                polygon.extend((0..=steps).map(|j| {
                    let t = a + (b - a) * j as f64 / steps as f64;
                    [cx + radius * t.cos(), cy + radius * t.sin()]
                }));
                let wedge = vec![polygon];
                emit(
                    paths::intersect(&area, &wedge),
                    sample(stops, (f64::from(i) + 0.5) / 720.),
                );
            }
        }
        _ => unreachable!(),
    }
    if let Some(alpha) = flat_alpha.filter(|a| *a < 255) {
        let mut group = Element::group(out);
        if let Element::Group(g) = &mut group {
            g.opacity = f64::from(alpha) / 255.;
        }
        vec![group]
    } else {
        out
    }
}
fn rectangle(bounds: Rect, paint: Brush) -> VectorShape {
    VectorShape {
        bounds,
        commands: vec![
            PathCommand::Move([0., 0.]),
            PathCommand::Line([bounds.width, 0.]),
            PathCommand::Line([bounds.width, bounds.height]),
            PathCommand::Line([0., bounds.height]),
            PathCommand::Close,
        ],
        fill: Some(paint),
        stroke: None,
    }
}

pub fn expand(element: Element) -> Element {
    match element {
        Element::Group(mut g) => {
            g.elements = g.elements.into_iter().map(expand).collect();
            Element::Group(g)
        }
        Element::Shape(mut s)
            if s.fill.as_ref().is_some_and(nonlinear)
                || s.stroke.as_ref().is_some_and(|s| nonlinear(&s.paint)) =>
        {
            let mut out = Vec::new();
            if s.fill.as_ref().is_some_and(nonlinear) {
                out.extend(fill(&s, s.fill.as_ref().unwrap()));
                s.fill = None;
            }
            if s.stroke.as_ref().is_some_and(|s| nonlinear(&s.paint)) {
                let stroke = s.stroke.take().unwrap();
                if let Some(mut p) = path(&s) {
                    if let Some(d) =
                        tiny::StrokeDash::new(stroke.dash.iter().map(|v| *v as f32).collect(), 0.)
                        && let Some(dashed) = p.dash(&d, 1.)
                    {
                        p = dashed;
                    }
                    let ts = tiny::Stroke {
                        width: stroke.width as f32,
                        miter_limit: stroke.miter_limit as f32,
                        line_cap: match stroke.cap.as_str() {
                            "rnd" => tiny::LineCap::Round,
                            "sq" => tiny::LineCap::Square,
                            _ => tiny::LineCap::Butt,
                        },
                        line_join: match stroke.join.as_str() {
                            "round" => tiny::LineJoin::Round,
                            "bevel" => tiny::LineJoin::Bevel,
                            _ => tiny::LineJoin::Miter,
                        },
                        ..Default::default()
                    };
                    if let Some(p) = p.stroke(&ts, 1.) {
                        let mut outline = s.clone();
                        outline.commands = paths::clip_commands(
                            &paths::simplify(&p, false),
                            Rect {
                                x: 0.,
                                y: 0.,
                                ..s.bounds
                            },
                        )
                        .unwrap_or_default();
                        outline.fill = Some(stroke.paint);
                        outline.stroke = None;
                        out.extend(fill(&outline, outline.fill.as_ref().unwrap()));
                    }
                }
            }
            if s.fill.is_some() || s.stroke.is_some() {
                out.push(Element::Shape(s));
            }
            Element::group(out)
        }
        Element::Table(mut t) => {
            let mut backgrounds = Vec::new();
            for cell in &mut t.cells {
                if cell.fill.as_ref().is_some_and(nonlinear) {
                    let b = Rect {
                        x: t.bounds.x + t.column_widths[..cell.column].iter().sum::<f64>(),
                        y: t.bounds.y + t.row_heights[..cell.row].iter().sum::<f64>(),
                        width: t.column_widths[cell.column..cell.column + cell.column_span]
                            .iter()
                            .sum(),
                        height: t.row_heights[cell.row..cell.row + cell.row_span]
                            .iter()
                            .sum(),
                    };
                    backgrounds.push(expand(Element::Shape(rectangle(
                        b,
                        cell.fill.take().unwrap(),
                    ))));
                }
            }
            if backgrounds.is_empty() {
                Element::Table(t)
            } else {
                backgrounds.push(Element::Table(t));
                Element::group(backgrounds)
            }
        }
        other => other,
    }
}

/// Keep the same paint coordinates after cropping a path's bounding rectangle.
pub fn remap(mut paint: Brush, from: Rect, to: Rect) -> Brush {
    match &mut paint {
        Brush::Radial { transform: m, .. } => {
            let sx = from.width / to.width;
            let sy = from.height / to.height;
            m[0] *= sx;
            m[2] *= sx;
            m[1] *= sy;
            m[3] *= sy;
            m[4] = m[4] * sx + (from.x - to.x) / to.width;
            m[5] = m[5] * sy + (from.y - to.y) / to.height;
        }
        Brush::Conic { center, .. } => {
            center[0] = (from.x + center[0] * from.width - to.x) / to.width;
            center[1] = (from.y + center[1] * from.height - to.y) / to.height;
        }
        Brush::Linear { stops, angle } => {
            let (y, x) = angle.to_radians().sin_cos();
            let span = x.abs() + y.abs();
            let at = |px: f64, py: f64| {
                (x * (px - from.x) / from.width + y * (py - from.y) / from.height
                    - x.min(0.)
                    - y.min(0.))
                    / span
            };
            let corners = [
                at(to.x, to.y),
                at(to.right(), to.y),
                at(to.x, to.bottom()),
                at(to.right(), to.bottom()),
            ];
            let lo = corners.into_iter().fold(f64::INFINITY, f64::min);
            let hi = corners.into_iter().fold(f64::NEG_INFINITY, f64::max);
            *stops = (0..=256)
                .map(|i| {
                    let t = f64::from(i) / 256.;
                    (t, sample(stops, lo + (hi - lo) * t))
                })
                .collect();
            *angle = (y * to.height / from.height)
                .atan2(x * to.width / from.width)
                .to_degrees();
        }
        _ => {}
    }
    paint
}
