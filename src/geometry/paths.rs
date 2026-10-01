//! Vector operations delegated to kurbo and i_overlay, with a 0.02 pt tolerance.
use crate::ir::{Brush, PathCommand, Rect, VectorShape};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::{simplify::SimplifyShape, single::SingleFloatOverlay},
};
use usvg::tiny_skia_path::{Path, PathBuilder, PathSegment, Point};
pub type Contours = Vec<Vec<[f64; 2]>>;

/// Office clips across lines, but hides an entire line when a clip cuts along
/// its writing direction. Keep that case explicit instead of losing text.
pub(crate) fn check_text_clip(ink: Rect, clip: Rect, vertical: bool) -> anyhow::Result<()> {
    let visible = ink.right() > clip.x + 0.01
        && ink.x < clip.right() - 0.01
        && ink.bottom() > clip.y + 0.01
        && ink.y < clip.bottom() - 0.01;
    let cut = if vertical {
        ink.y < clip.y - 0.01 || ink.bottom() > clip.bottom() + 0.01
    } else {
        ink.x < clip.x - 0.01 || ink.right() > clip.right() + 0.01
    };
    anyhow::ensure!(
        !visible || !cut,
        "PowerPoint cannot preserve partial clipping along an editable text line"
    );
    Ok(())
}

pub fn shape_path(s: &VectorShape) -> Option<Path> {
    let mut b = PathBuilder::new();
    for c in &s.commands {
        match *c {
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

pub fn contains_rect(clip: &Contours, r: Rect) -> bool {
    let rectangle = vec![vec![
        [r.x, r.y],
        [r.right(), r.y],
        [r.right(), r.bottom()],
        [r.x, r.bottom()],
    ]];
    let outside = difference(&rectangle, clip);
    let area: f64 = outside
        .iter()
        .map(|p| {
            p.iter()
                .zip(p.iter().cycle().skip(1))
                .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
                .sum::<f64>()
                / 2.
        })
        .sum();
    area.abs() < 0.001
}

pub fn rectangular(clip: &Contours) -> Option<Rect> {
    let points: Vec<_> = clip.iter().flatten().collect();
    if points.is_empty() {
        return None;
    }
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
    let r = Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    };
    contains_rect(clip, r).then_some(r)
}

pub fn contours(path: &Path) -> Contours {
    let mut bez = kurbo::BezPath::new();
    let pt = |p: Point| (f64::from(p.x), f64::from(p.y));
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => bez.move_to(pt(p)),
            PathSegment::LineTo(p) => bez.line_to(pt(p)),
            PathSegment::QuadTo(c, p) => bez.quad_to(pt(c), pt(p)),
            PathSegment::CubicTo(a, b, p) => bez.curve_to(pt(a), pt(b), pt(p)),
            PathSegment::Close => bez.close_path(),
        }
    }
    let mut out: Contours = Vec::new();
    kurbo::flatten(bez.iter(), 0.02, |el| match el {
        kurbo::PathEl::MoveTo(p) => out.push(vec![[p.x, p.y]]),
        kurbo::PathEl::LineTo(p) => {
            if let Some(c) = out.last_mut() {
                c.push([p.x, p.y]);
            }
        }
        _ => {}
    });
    out
}

pub fn simplify(path: &Path, even_odd: bool) -> Contours {
    contours(path)
        .simplify_shape(if even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        })
        .into_iter()
        .flatten()
        .collect()
}
pub fn intersect(a: &Contours, b: &Contours) -> Contours {
    a.overlay(b, OverlayRule::Intersect, FillRule::NonZero)
        .into_iter()
        .flatten()
        .collect()
}
pub fn union(a: &Contours, b: &Contours) -> Contours {
    a.overlay(b, OverlayRule::Union, FillRule::NonZero)
        .into_iter()
        .flatten()
        .collect()
}
pub fn difference(a: &Contours, b: &Contours) -> Contours {
    a.overlay(b, OverlayRule::Difference, FillRule::NonZero)
        .into_iter()
        .flatten()
        .collect()
}
pub fn path(contours: &Contours) -> Option<Path> {
    let mut b = PathBuilder::new();
    for points in contours {
        if let Some(p) = points.first() {
            b.move_to(p[0] as f32, p[1] as f32);
            for p in &points[1..] {
                b.line_to(p[0] as f32, p[1] as f32);
            }
            b.close();
        }
    }
    b.finish()
}
pub fn shape(path: &Path, offset: [f64; 2], fill: Option<Brush>) -> Option<VectorShape> {
    let b = path.compute_tight_bounds()?;
    let p = |p: Point| [f64::from(p.x - b.left()), f64::from(p.y - b.top())];
    let mut last = Point::zero();
    let commands = path
        .segments()
        .map(|segment| match segment {
            PathSegment::MoveTo(to) => {
                last = to;
                PathCommand::Move(p(to))
            }
            PathSegment::LineTo(to) => {
                last = to;
                PathCommand::Line(p(to))
            }
            PathSegment::QuadTo(c, to) => {
                let c1 = Point::from_xy(
                    last.x + (c.x - last.x) * 2. / 3.,
                    last.y + (c.y - last.y) * 2. / 3.,
                );
                let c2 =
                    Point::from_xy(to.x + (c.x - to.x) * 2. / 3., to.y + (c.y - to.y) * 2. / 3.);
                last = to;
                PathCommand::Cubic([p(c1), p(c2), p(to)])
            }
            PathSegment::CubicTo(a, b, to) => {
                last = to;
                PathCommand::Cubic([p(a), p(b), p(to)])
            }
            PathSegment::Close => PathCommand::Close,
        })
        .collect();
    Some(VectorShape {
        bounds: Rect {
            x: offset[0] + f64::from(b.left()),
            y: offset[1] + f64::from(b.top()),
            width: f64::from(b.width()).max(0.01),
            height: f64::from(b.height()).max(0.01),
        },
        commands,
        fill,
        stroke: None,
    })
}

pub fn curve(curve: &typst::visualize::Curve, transform: typst::layout::Transform) -> Option<Path> {
    use typst::visualize::CurveItem;
    let mut b = PathBuilder::new();
    let point = |p: typst::layout::Point| {
        let p = p.transform(transform);
        (p.x.to_pt() as f32, p.y.to_pt() as f32)
    };
    for cmd in &curve.0 {
        match cmd {
            CurveItem::Move(p) => {
                let (x, y) = point(*p);
                b.move_to(x, y);
            }
            CurveItem::Line(p) => {
                let (x, y) = point(*p);
                b.line_to(x, y);
            }
            CurveItem::Cubic(a, c, d) => {
                let (ax, ay) = point(*a);
                let (cx, cy) = point(*c);
                let (dx, dy) = point(*d);
                b.cubic_to(ax, ay, cx, cy, dx, dy);
            }
            CurveItem::Close => b.close(),
        }
    }
    b.finish()
}

pub fn clip_commands(polygons: &Contours, bounds: Rect) -> Option<Vec<PathCommand>> {
    let path = path(polygons)?;
    let mut shape = shape(&path, [0., 0.], None)?;
    for cmd in &mut shape.commands {
        let adjust = |p: &mut [f64; 2]| {
            p[0] += shape.bounds.x - bounds.x;
            p[1] += shape.bounds.y - bounds.y;
        };
        match cmd {
            PathCommand::Move(p) | PathCommand::Line(p) => adjust(p),
            PathCommand::Cubic(points) => points.iter_mut().for_each(adjust),
            _ => {}
        }
    }
    Some(shape.commands)
}
