//! Affine placement using native group transforms (including shear via SVD).
use crate::ir::{Element, ObjectGroup, Rect};

/// Coefficients [a,b,c,d,e,f] map (x,y) to (a*x+c*y+e,b*x+d*y+f).
pub fn apply(elements: Vec<Element>, m: [f64; 6]) -> Element {
    let [a, b, c, d, e, f] = m;
    let dot = a * c + b * d;
    if dot.abs() < 1e-9 {
        return orthogonal(elements, m);
    }
    // A = U S V^T. Both U and V are rotations/reflections, which PowerPoint
    // groups support. This keeps pictures and text editable under a shear.
    let theta = 0.5 * (2. * dot).atan2(a * a + b * b - c * c - d * d);
    let (s, t) = theta.sin_cos();
    let u1 = [a * t + c * s, b * t + d * s];
    let u2 = [-a * s + c * t, -b * s + d * t];
    let first = orthogonal(elements, [t, -s, s, t, 0., 0.]);
    orthogonal(vec![first], [u1[0], u1[1], u2[0], u2[1], e, f])
}
fn orthogonal(elements: Vec<Element>, m: [f64; 6]) -> Element {
    let content = elements
        .iter()
        .map(Element::bounds)
        .reduce(Rect::union)
        .unwrap_or_default();
    let [a, b, c, d, e, f] = m;
    let sx = a.hypot(b);
    let sy = c.hypot(d);
    let cx = content.x + content.width / 2.;
    let cy = content.y + content.height / 2.;
    let width = (content.width * sx).max(0.01);
    let height = (content.height * sy).max(0.01);
    Element::Group(ObjectGroup {
        bounds: Rect {
            x: a * cx + c * cy + e - width / 2.,
            y: b * cx + d * cy + f - height / 2.,
            width,
            height,
        },
        content_bounds: content,
        rotation: b.atan2(a).to_degrees(),
        opacity: 1.,
        effect: None,
        flip_x: false,
        flip_y: a * d - b * c < 0.,
        elements,
    })
}
