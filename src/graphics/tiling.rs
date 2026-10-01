//! Use Typst's resolved paint coordinates, then lower the pattern to native
//! paths and text. Pure graphics use SVG only as an in-memory interchange;
//! patterns containing text or imported images retain their original frame.
use crate::{
    compiler::capture::Leaf,
    geometry::paths,
    ir::{Element, Rect},
};
use anyhow::{Result, ensure};
use typst::{
    foundations::{Content, Smart},
    layout::{Abs, Frame, FrameItem, GroupItem, Point, Sides, Size},
    syntax::Span,
    visualize::{Geometry, Paint, Shape},
};

fn has_text_or_images(frame: &Frame) -> bool {
    frame.items().any(|(_, item)| match item {
        FrameItem::Text(_) => true,
        FrameItem::Group(g) => has_text_or_images(&g.frame),
        FrameItem::Image(..) => true,
        FrameItem::Shape(s, _) => s
            .fill
            .iter()
            .chain(s.stroke.iter().map(|s| &s.paint))
            .any(|p| matches!(p, Paint::Tiling(t) if has_text_or_images(t.frame()))),
        _ => false,
    })
}

pub(crate) fn convert(leaf: &Leaf, options: &crate::lower::Options) -> Result<Vec<Element>> {
    let FrameItem::Shape(shape, _) = &leaf.item else {
        anyhow::bail!("expected shape")
    };
    if shape
        .fill
        .iter()
        .chain(shape.stroke.iter().map(|s| &s.paint))
        .any(|p| matches!(p, Paint::Tiling(t) if has_text_or_images(t.frame())))
    {
        return native(leaf, options);
    }
    let ink = leaf
        .ink_bounds()
        .ok_or_else(|| anyhow::anyhow!("empty tiling bounds"))?;
    // Include off-page geometry as well; the caller's clip is applied below.
    let origin = [
        ink.x.min(leaf.frame.x).min(0.) - 1.,
        ink.y.min(leaf.frame.y).min(0.) - 1.,
    ];
    let bounds = Rect {
        x: origin[0],
        y: origin[1],
        width: ink.right().max(leaf.frame.right()).max(1.) - origin[0] + 1.,
        height: ink.bottom().max(leaf.frame.bottom()).max(1.) - origin[1] + 1.,
    };
    let size = Size::new(Abs::pt(bounds.width), Abs::pt(bounds.height));
    let mut root = Frame::hard(size);
    let mut parent = Frame::hard(Size::new(
        Abs::pt(leaf.frame_geometry.size.0),
        Abs::pt(leaf.frame_geometry.size.1),
    ));
    let inverse = leaf
        .transform
        .invert()
        .ok_or_else(|| anyhow::anyhow!("singular tiling transform"))?;
    parent.push(
        Point::new(Abs::pt(leaf.position.0), Abs::pt(leaf.position.1)).transform(inverse),
        leaf.item.clone(),
    );
    let mut group = GroupItem::new(parent);
    group.transform = leaf.transform;
    root.push(
        Point::new(Abs::pt(-origin[0]), Abs::pt(-origin[1])),
        FrameItem::Group(group),
    );
    let page = typst_layout::Page {
        frame: root,
        bleed: Sides::splat(Abs::zero()),
        fill: Smart::Custom(None),
        numbering: None,
        supplement: Content::empty(),
        number: 1,
    };
    let svg = typst_svg::svg(&page, &Default::default());
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())?;
    ensure!(
        !tree.has_text_nodes(),
        "tiling text needs semantic font information"
    );
    let mut clip = vec![vec![
        [0., 0.],
        [bounds.width, 0.],
        [bounds.width, bounds.height],
        [0., bounds.height],
    ]];
    for c in &leaf.clips {
        let local = c
            .iter()
            .map(|p| {
                p.iter()
                    .map(|p| [p[0] - origin[0], p[1] - origin[1]])
                    .collect()
            })
            .collect();
        clip = paths::intersect(&clip, &local);
    }
    crate::graphics::svg::convert_clipped(&tree, bounds, options.image_dpi, &clip)
}

pub(crate) fn rectangle(
    paint: Paint,
    bounds: Rect,
    options: &crate::lower::Options,
) -> Result<Vec<Element>> {
    let mut frame = Frame::hard(Size::new(
        Abs::pt(bounds.right().max(1.)),
        Abs::pt(bounds.bottom().max(1.)),
    ));
    frame.push(
        Point::new(Abs::pt(bounds.x), Abs::pt(bounds.y)),
        FrameItem::Shape(
            Shape {
                geometry: Geometry::Rect(Size::new(Abs::pt(bounds.width), Abs::pt(bounds.height))),
                fill: Some(paint),
                stroke: None,
                fill_rule: Default::default(),
            },
            Span::detached(),
        ),
    );
    let doc = typst_layout::PagedDocument::new(
        vec![typst_layout::Page {
            frame,
            bleed: Sides::splat(Abs::zero()),
            fill: Smart::Custom(None),
            numbering: None,
            supplement: Content::empty(),
            number: 1,
        }]
        .into(),
        Default::default(),
    );
    convert(
        &crate::compiler::capture::Capture::new(&doc).pages[0][0],
        options,
    )
}

/// Keep each repeated tile's semantic tags in its own capture. Reusing a tag
/// location across one combined frame would merge unrelated tile paragraphs.
fn native(leaf: &Leaf, options: &crate::lower::Options) -> Result<Vec<Element>> {
    use typst::layout::Transform;
    use typst::visualize::{Color, Curve, CurveItem, RelativeTo};
    use usvg::tiny_skia_path as tiny;
    let FrameItem::Shape(shape, span) = &leaf.item else {
        unreachable!()
    };
    let mut out = Vec::new();
    for stroke in [false, true] {
        let paint = if stroke {
            shape.stroke.as_ref().map(|s| &s.paint)
        } else {
            shape.fill.as_ref()
        };
        let Some(paint) = paint else { continue };
        let mut part = shape.clone();
        if stroke {
            part.fill = None;
        } else {
            part.stroke = None;
        }
        let mut solid = leaf.clone();
        let Paint::Tiling(pattern) = paint else {
            solid.item = FrameItem::Shape(part, *span);
            out.extend(crate::graphics::convert_with_options(&solid, options)?);
            continue;
        };
        if stroke {
            part.stroke.as_mut().unwrap().paint = Paint::Solid(Color::BLACK);
        } else {
            part.fill = Some(Paint::Solid(Color::BLACK));
        }
        solid.item = FrameItem::Shape(part, *span);
        // This forces stroke expansion and clipping through the same geometry
        // libraries as ordinary paths, retaining its joins, dashes and fill rule.
        solid.clipped = true;
        let mut region = Vec::new();
        for element in crate::graphics::convert_with_options(&solid, options)? {
            let Element::Shape(s) = element else {
                unreachable!()
            };
            if let Some(path) = paths::shape_path(&s).and_then(|p| {
                p.transform(tiny::Transform::from_translate(
                    s.bounds.x as f32,
                    s.bounds.y as f32,
                ))
            }) {
                region = paths::union(&region, &paths::simplify(&path, false));
            }
        }
        if region.is_empty() {
            continue;
        }
        let t = if pattern.unwrap_relative(false) == RelativeTo::Parent {
            leaf.paint_geometry.transform
        } else {
            Transform {
                tx: Abs::pt(leaf.position.0),
                ty: Abs::pt(leaf.position.1),
                ..leaf.transform
            }
        };
        let inverse = t
            .invert()
            .ok_or_else(|| anyhow::anyhow!("singular tiling transform"))?;
        let local: paths::Contours = region
            .iter()
            .map(|c| {
                c.iter()
                    .map(|p| {
                        let p = Point::new(Abs::pt(p[0]), Abs::pt(p[1])).transform(inverse);
                        [p.x.to_pt(), p.y.to_pt()]
                    })
                    .collect()
            })
            .collect();
        let size = pattern.size() + pattern.spacing();
        let (w, h) = (size.x.to_pt(), size.y.to_pt());
        ensure!(w > 0. && h > 0., "tiling has an empty repeat interval");
        let offset = pattern.offset();
        let (ox, oy) = (offset.x.to_pt(), offset.y.to_pt());
        let extent = local.iter().flatten().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |mut b, p| {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
                b
            },
        );
        let left = ((extent[0] - ox) / w).floor() as i64;
        let right = ((extent[2] - ox) / w).ceil() as i64;
        let top = ((extent[1] - oy) / h).floor() as i64;
        let bottom = ((extent[3] - oy) / h).ceil() as i64;
        ensure!(
            right
                .saturating_sub(left)
                .saturating_mul(bottom.saturating_sub(top))
                <= 16384,
            "Typst pattern exceeds 16384 native tiles"
        );
        for y in top..bottom {
            for x in left..right {
                let (x, y) = (ox + x as f64 * w, oy + y as f64 * h);
                let clip = paths::intersect(
                    &local,
                    &vec![vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]],
                );
                if clip.is_empty() {
                    continue;
                }
                let mut curve = Curve::new();
                for contour in &clip {
                    for (i, p) in contour.iter().enumerate() {
                        let p = Point::new(Abs::pt(p[0] - x), Abs::pt(p[1] - y));
                        curve.0.push(if i == 0 {
                            CurveItem::Move(p)
                        } else {
                            CurveItem::Line(p)
                        });
                    }
                    curve.0.push(CurveItem::Close);
                }
                let mut tile = GroupItem::new(pattern.frame().clone());
                tile.clip = Some(curve);
                tile.transform = t.pre_concat(Transform::translate(Abs::pt(x), Abs::pt(y)));
                let mut root = Frame::hard(Size::new(
                    Abs::pt(leaf.frame.right().max(1.)),
                    Abs::pt(leaf.frame.bottom().max(1.)),
                ));
                root.push(Point::zero(), FrameItem::Group(tile));
                let doc = typst_layout::PagedDocument::new(
                    vec![typst_layout::Page {
                        frame: root,
                        bleed: Sides::splat(Abs::zero()),
                        fill: Smart::Custom(None),
                        numbering: None,
                        supplement: Content::empty(),
                        number: 1,
                    }]
                    .into(),
                    Default::default(),
                );
                out.extend(crate::lower::convert_fragment(&doc, options)?);
            }
        }
    }
    Ok(out)
}
