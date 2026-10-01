//! Lower supported SVG filter chains to effects on native object groups.
use super::*;
use usvg::filter::{ColorInterpolation, Input, Kind};

pub(super) fn convert(g: &usvg::Group, scale: Transform, clip: &Contours) -> Result<Vec<Effect>> {
    if g.filters().is_empty() {
        return Ok(vec![]);
    }
    ensure!(
        g.clip_path().is_none() && g.mask().is_none(),
        "clipping or masking a filtered SVG group requires an effect clip"
    );
    let ts = scale.pre_concat(g.abs_transform());
    let sx = f64::from(ts.sx).hypot(f64::from(ts.ky));
    let sy = f64::from(ts.kx).hypot(f64::from(ts.sy));
    ensure!(
        (sx - sy).abs() < 1e-5 && (ts.sx * ts.kx + ts.ky * ts.sy).abs() < 1e-5,
        "nonuniformly transformed SVG filters need elliptical native effects"
    );
    let b = g.stroke_bounding_box();
    let mut extent = Rect {
        x: f64::from(b.x()),
        y: f64::from(b.y()),
        width: f64::from(b.width()),
        height: f64::from(b.height()),
    };
    // Descendant filters can extend beyond their source geometry.
    for child in g.children() {
        if let usvg::Node::Group(child) = child
            && let Some(b) = child.layer_bounding_box().transform(child.transform())
        {
            extent = extent.union(Rect {
                x: f64::from(b.x()),
                y: f64::from(b.y()),
                width: f64::from(b.width()),
                height: f64::from(b.height()),
            });
        }
    }
    let mut effects = Vec::new();
    for filter in g.filters() {
        if let Some(shadow) = expanded_shadow(filter, extent)? {
            extent = extent.union(shadow.bounds);
            check_region(extent, filter.rect())?;
            check_clip(extent, ts, clip)?;
            effects.push(Effect::Shadow {
                radius: 2. * shadow.sigma * sx,
                offset: [
                    f64::from(ts.sx) * shadow.offset[0] + f64::from(ts.kx) * shadow.offset[1],
                    f64::from(ts.ky) * shadow.offset[0] + f64::from(ts.sy) * shadow.offset[1],
                ],
                color: shadow.color,
            });
            continue;
        }
        let mut previous = None;
        for primitive in filter.primitives() {
            let (input, dx, dy, sigma_x, sigma_y, color) = match primitive.kind() {
                Kind::GaussianBlur(b) => {
                    ensure!(
                        primitive.color_interpolation() == ColorInterpolation::SRGB,
                        "SVG blur in linearRGB needs a color-space conversion; use sRGB for native blur"
                    );
                    (
                        b.input(),
                        0.,
                        0.,
                        b.std_dev_x().get(),
                        b.std_dev_y().get(),
                        None,
                    )
                }
                Kind::DropShadow(s) => {
                    let c = s.color();
                    (
                        s.input(),
                        s.dx(),
                        s.dy(),
                        s.std_dev_x().get(),
                        s.std_dev_y().get(),
                        Some([
                            c.red,
                            c.green,
                            c.blue,
                            (s.opacity().get() * 255.).round() as u8,
                        ]),
                    )
                }
                _ => bail!("this SVG filter primitive has no native blur/shadow mapping"),
            };
            ensure!(
                match (&previous, input) {
                    (None, Input::SourceGraphic) => true,
                    (Some(result), Input::Reference(name)) => result == name,
                    _ => false,
                },
                "branched SVG filters need an effect graph"
            );
            ensure!(
                (sigma_x - sigma_y).abs() < 1e-5,
                "anisotropic SVG blur needs an elliptical native effect"
            );
            let sigma = f64::from(sigma_x);
            let blurred = Rect {
                x: extent.x + f64::from(dx) - 3. * sigma,
                y: extent.y + f64::from(dy) - 3. * sigma,
                width: extent.width + 6. * sigma,
                height: extent.height + 6. * sigma,
            };
            extent = if color.is_some() {
                extent.union(blurred)
            } else {
                blurred
            };
            for r in [primitive.rect(), filter.rect()] {
                check_region(extent, r)?;
            }
            check_clip(extent, ts, clip)?;
            let radius = 2. * sigma * sx;
            effects.push(if let Some(color) = color {
                Effect::Shadow {
                    radius,
                    offset: [
                        f64::from(ts.sx * dx + ts.kx * dy),
                        f64::from(ts.ky * dx + ts.sy * dy),
                    ],
                    color,
                }
            } else {
                Effect::Blur { radius }
            });
            previous = Some(primitive.result().to_owned());
        }
    }
    Ok(effects)
}

fn check_region(bounds: Rect, region: tiny::NonZeroRect) -> Result<()> {
    ensure!(
        bounds.x >= f64::from(region.x()) - 0.01
            && bounds.y >= f64::from(region.y()) - 0.01
            && bounds.right() <= f64::from(region.right()) + 0.01
            && bounds.bottom() <= f64::from(region.bottom()) + 0.01,
        "SVG filter region cuts the effect; native group effects cannot retain this clipping"
    );
    Ok(())
}

fn check_clip(extent: Rect, ts: Transform, clip: &Contours) -> Result<()> {
    let b = tiny::Rect::from_xywh(
        extent.x as f32,
        extent.y as f32,
        extent.width as f32,
        extent.height as f32,
    )
    .and_then(|b| b.transform(ts))
    .ok_or_else(|| anyhow!("invalid SVG effect bounds"))?;
    ensure!(
        contains(
            clip,
            Rect {
                x: f64::from(b.x()),
                y: f64::from(b.y()),
                width: f64::from(b.width()),
                height: f64::from(b.height()),
            }
        ),
        "SVG viewport or ancestor clip cuts the native effect"
    );
    Ok(())
}

struct Shadow {
    bounds: Rect,
    offset: [f64; 2],
    sigma: f64,
    color: [u8; 4],
}

// SVG 1.1 exporters often spell feDropShadow as a SourceAlpha blur/offset,
// optional flood-in compositing, then merge with SourceGraphic. Follow named
// dependencies, including reused result names, rather than assuming adjacency.
fn expanded_shadow(filter: &usvg::filter::Filter, source: Rect) -> Result<Option<Shadow>> {
    use usvg::filter::CompositeOperator as Op;
    let nodes = filter.primitives();
    let Some(last) = nodes.last() else {
        return Ok(None);
    };
    let input = match last.kind() {
        Kind::Merge(m) if m.inputs().len() == 2 && m.inputs()[1] == Input::SourceGraphic => {
            &m.inputs()[0]
        }
        Kind::Composite(c) if c.operator() == Op::Over && *c.input1() == Input::SourceGraphic => {
            c.input2()
        }
        _ => return Ok(None),
    };
    ensure!(
        nodes.len() <= 256,
        "SVG shadow filter dependency chain is too deep"
    );
    let end = nodes.len() - 1;
    let mut color = [0, 0, 0, 255];
    let (alpha, before, region) = if let Some(i) = reference(nodes, end, input) {
        if let Kind::Composite(c) = nodes[i].kind() {
            ensure!(
                c.operator() == Op::In,
                "SVG shadow needs flood-in compositing"
            );
            let flood = reference(nodes, i, c.input1())
                .ok_or_else(|| anyhow!("SVG shadow needs a flood color"))?;
            let Kind::Flood(f) = nodes[flood].kind() else {
                bail!("SVG shadow needs a flood color")
            };
            let rgb = f.color();
            color = [
                rgb.red,
                rgb.green,
                rgb.blue,
                (f.opacity().get() * 255.).round() as u8,
            ];
            (c.input2(), i, Some((nodes[i].rect(), nodes[flood].rect())))
        } else {
            (input, end, None)
        }
    } else {
        (input, end, None)
    };
    let mut shadow = alpha_shadow(nodes, before, alpha, source)?;
    shadow.color = color;
    if let Some((composite, flood)) = region {
        check_region(shadow.bounds, composite)?;
        check_region(shadow.bounds, flood)?;
    }
    check_region(source.union(shadow.bounds), last.rect())?;
    Ok(Some(shadow))
}

fn reference(nodes: &[usvg::filter::Primitive], before: usize, input: &Input) -> Option<usize> {
    let Input::Reference(name) = input else {
        return None;
    };
    nodes[..before].iter().rposition(|p| p.result() == name)
}

fn alpha_shadow(
    nodes: &[usvg::filter::Primitive],
    before: usize,
    input: &Input,
    source: Rect,
) -> Result<Shadow> {
    if *input == Input::SourceAlpha {
        return Ok(Shadow {
            bounds: source,
            offset: [0., 0.],
            sigma: 0.,
            color: [0, 0, 0, 255],
        });
    }
    let i = reference(nodes, before, input)
        .ok_or_else(|| anyhow!("SVG shadow must derive from SourceAlpha"))?;
    let shadow = match nodes[i].kind() {
        Kind::GaussianBlur(b) => {
            ensure!(
                (b.std_dev_x().get() - b.std_dev_y().get()).abs() < 1e-5,
                "anisotropic SVG blur needs an elliptical native effect"
            );
            let mut s = alpha_shadow(nodes, i, b.input(), source)?;
            ensure!(
                s.sigma == 0.,
                "multiple alpha blurs need a separate native effect chain"
            );
            s.sigma = f64::from(b.std_dev_x().get());
            s.bounds.x -= 3. * s.sigma;
            s.bounds.y -= 3. * s.sigma;
            s.bounds.width += 6. * s.sigma;
            s.bounds.height += 6. * s.sigma;
            s
        }
        Kind::Offset(o) => {
            let mut s = alpha_shadow(nodes, i, o.input(), source)?;
            s.offset[0] += f64::from(o.dx());
            s.offset[1] += f64::from(o.dy());
            s.bounds.x += f64::from(o.dx());
            s.bounds.y += f64::from(o.dy());
            s
        }
        _ => bail!("this SVG alpha filter has no native shadow mapping"),
    };
    check_region(shadow.bounds, nodes[i].rect())?;
    Ok(shadow)
}
