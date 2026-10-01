use crate::ir::{Brush, Rect};
use anyhow::{Result, anyhow, bail, ensure};
use usvg::tiny_skia_path::{Point, Transform};

pub(super) fn convert(
    paint: &usvg::Paint,
    opacity: f32,
    transform: Transform,
    bounds: Rect,
) -> Result<Brush> {
    match paint {
        usvg::Paint::Color(c) => Ok(Brush::Solid {
            color: [c.red, c.green, c.blue, (opacity * 255.).round() as u8],
        }),
        usvg::Paint::LinearGradient(g) => {
            let inv = transform
                .pre_concat(g.transform())
                .invert()
                .ok_or_else(|| anyhow!("singular gradient transform"))?;
            let dx = g.x2() - g.x1();
            let dy = g.y2() - g.y1();
            let len = dx * dx + dy * dy;
            ensure!(len > 1e-12, "zero gradient vector");
            let at = |x: f64, y: f64| {
                let mut p = Point::from_xy(x as f32, y as f32);
                inv.map_point(&mut p);
                f64::from(((p.x - g.x1()) * dx + (p.y - g.y1()) * dy) / len)
            };
            let values = [
                at(bounds.x, bounds.y),
                at(bounds.right(), bounds.y),
                at(bounds.right(), bounds.bottom()),
                at(bounds.x, bounds.bottom()),
            ];
            let lo = values.into_iter().fold(f64::INFINITY, f64::min);
            let hi = values.into_iter().fold(f64::NEG_INFINITY, f64::max);
            let gx = f64::from(inv.sx * dx + inv.ky * dy) * bounds.width;
            let gy = f64::from(inv.kx * dx + inv.sy * dy) * bounds.height;
            Ok(Brush::Linear {
                stops: stops(g, opacity, lo, hi),
                angle: gy.atan2(gx).to_degrees(),
            })
        }
        usvg::Paint::RadialGradient(g) => {
            let ts = transform.pre_concat(g.transform());
            let dx = f64::from(g.cx() - g.fx());
            let dy = f64::from(g.cy() - g.fy());
            let dr = f64::from(g.r().get() - g.fr().get());
            ensure!(
                dr > 0. && dx.hypot(dy) < dr + 1e-6,
                "non-nested SVG gradient circles require a different gradient surface"
            );
            let mut extent = 1_f64;
            if g.spread_method() != usvg::SpreadMethod::Pad {
                let inv = ts
                    .invert()
                    .ok_or_else(|| anyhow!("singular radial gradient transform"))?;
                for (x, y) in [
                    (bounds.x, bounds.y),
                    (bounds.right(), bounds.y),
                    (bounds.right(), bounds.bottom()),
                    (bounds.x, bounds.bottom()),
                ] {
                    let mut p = Point::from_xy(x as f32, y as f32);
                    inv.map_point(&mut p);
                    let (x, y) = (f64::from(p.x - g.fx()), f64::from(p.y - g.fy()));
                    let fr = f64::from(g.fr().get());
                    let a = dx * dx + dy * dy - dr * dr;
                    let b = -2. * (x * dx + y * dy + fr * dr);
                    let c = x * x + y * y - fr * fr;
                    ensure!(a < -1e-12, "degenerate repeating radial gradient");
                    extent = extent.max((-b - (b * b - 4. * a * c).max(0.).sqrt()) / (2. * a));
                }
                extent = extent.ceil();
                ensure!(
                    extent <= 64.,
                    "radial gradient exceeds 64 native repetitions"
                );
            }
            let stops = stops(g, opacity, 0., extent);
            ensure!(
                stops.iter().all(|s| s.1[3] == stops[0].1[3]),
                "PowerPoint introduces seams in radial gradients with varying opacity"
            );
            Ok(Brush::Radial {
                stops,
                center: [
                    f64::from(g.fx()) + dx * extent,
                    f64::from(g.fy()) + dy * extent,
                ],
                focal: [f64::from(g.fx()), f64::from(g.fy())],
                radius: f64::from(g.fr().get()) + dr * extent,
                inner_radius: f64::from(g.fr().get()),
                transform: Box::new([
                    f64::from(ts.sx) / bounds.width,
                    f64::from(ts.ky) / bounds.height,
                    f64::from(ts.kx) / bounds.width,
                    f64::from(ts.sy) / bounds.height,
                    (f64::from(ts.tx) - bounds.x) / bounds.width,
                    (f64::from(ts.ty) - bounds.y) / bounds.height,
                ]),
            })
        }
        usvg::Paint::Pattern(_) => bail!("SVG pattern needs tiled native objects"),
    }
}
fn stops(g: &usvg::BaseGradient, opacity: f32, lo: f64, hi: f64) -> Vec<(f64, [u8; 4])> {
    let mut positions: Vec<f64> = vec![0., 1.];
    let span = hi - lo;
    if span.abs() < 1e-12 {
        return vec![(0., sample(g, opacity, lo)), (1., sample(g, opacity, lo))];
    }
    let (a, b) = match g.spread_method() {
        usvg::SpreadMethod::Pad => (0, 0),
        _ => (lo.floor() as i32, hi.ceil() as i32),
    };
    for cycle in a.max(-1024)..=b.min(1024) {
        for s in g.stops() {
            let u = f64::from(s.offset().get());
            let u = if g.spread_method() == usvg::SpreadMethod::Reflect && cycle % 2 != 0 {
                1. - u
            } else {
                u
            };
            let t = (f64::from(cycle) + u - lo) / span;
            if (0.0..=1.0).contains(&t) {
                positions.push(t);
                // Preserve the left limit at a hard stop or repeat boundary.
                // One unit in DrawingML's 100000-step stop coordinate space.
                if t > 0.00001 {
                    positions.push(t - 0.00001);
                }
            }
        }
    }
    positions.sort_by(f64::total_cmp);
    positions.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    positions
        .into_iter()
        .map(|t| (t, sample(g, opacity, lo + span * t)))
        .collect()
}
fn sample(g: &usvg::BaseGradient, opacity: f32, mut t: f64) -> [u8; 4] {
    t = match g.spread_method() {
        usvg::SpreadMethod::Pad => t.clamp(0., 1.),
        usvg::SpreadMethod::Repeat => t.rem_euclid(1.),
        usvg::SpreadMethod::Reflect => 1. - (t.rem_euclid(2.) - 1.).abs(),
    };
    let stops = g.stops();
    let color = |s: &usvg::Stop| {
        let c = s.color();
        [
            f64::from(c.red),
            f64::from(c.green),
            f64::from(c.blue),
            f64::from(s.opacity().get() * opacity) * 255.,
        ]
    };
    let right = stops
        .iter()
        .position(|s| f64::from(s.offset().get()) >= t)
        .unwrap_or(stops.len() - 1);
    let left = right.saturating_sub(1);
    let a = &stops[left];
    let b = &stops[right];
    let u = if left == right || a.offset() == b.offset() {
        0.
    } else {
        (t - f64::from(a.offset().get())) / f64::from(b.offset().get() - a.offset().get())
    };
    let a = color(a);
    let b = color(b);
    std::array::from_fn(|i| (a[i] + (b[i] - a[i]) * u).round().clamp(0., 255.) as u8)
}
