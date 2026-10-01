//! Interpret embedded PDF pages into native paths, text, and individual images.
mod images;
mod paints;
mod text;

use crate::{
    geometry::paths::{self, Contours},
    ir::*,
};
use anyhow::{Result, anyhow, bail, ensure};
use hayro_interpret::{self as pdf, Device, TransformExt};
use kurbo::{Affine, BezPath, Shape};
use typst::visualize::PdfImage;
use usvg::tiny_skia_path as tiny;

pub(crate) fn convert(
    image: &PdfImage,
    bounds: Rect,
    dpi: Option<u32>,
    clip: Option<&Contours>,
) -> Result<Vec<Element>> {
    let page = image.page();
    let transform = Affine::translate((bounds.x, bounds.y))
        * Affine::scale_non_uniform(
            bounds.width / f64::from(image.width()),
            bounds.height / f64::from(image.height()),
        )
        * page.initial_transform(true).to_kurbo();
    ensure!(
        transform.as_coeffs().iter().all(|v| v.is_finite()),
        "invalid PDF page dimensions"
    );
    let viewport = rectangle(bounds);
    let clip = clip.map_or(viewport.clone(), |c| paths::intersect(&viewport, c));
    let mut device = NativePdf {
        elements: Vec::new(),
        clips: vec![clip],
        groups: Vec::new(),
        text: None,
        fonts: Default::default(),
        dpi,
        soft_mask: false,
        blend: pdf::BlendMode::Normal,
        error: None,
    };
    let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = warnings.clone();
    let settings = pdf::InterpreterSettings {
        warning_sink: std::sync::Arc::new(move |warning| sink.lock().unwrap().push(warning)),
        render_annotations: false,
        ..Default::default()
    };
    let cache = pdf::InterpreterCache::new();
    let mut context = pdf::Context::new(
        transform,
        kurbo::Rect::new(bounds.x, bounds.y, bounds.right(), bounds.bottom()),
        &cache,
        page.xref(),
        settings,
    );
    pdf::interpret_page(page, &mut context, &mut device);
    device.flush_text();
    if let Some(error) = device.error {
        return Err(error);
    }
    ensure!(
        warnings.lock().unwrap().is_empty(),
        "PDF interpreter reported an unsupported font or an image decoding failure"
    );
    Ok(device.elements)
}

pub(crate) fn fonts(image: &PdfImage) -> Vec<typst::text::FontInstance> {
    text::collect_fonts(image)
}

struct NativePdf {
    elements: Vec<Element>,
    clips: Vec<Contours>,
    groups: Vec<(usize, f32)>,
    text: Option<text::Line>,
    fonts: text::Fonts,
    dpi: Option<u32>,
    soft_mask: bool,
    blend: pdf::BlendMode,
    error: Option<anyhow::Error>,
}

impl NativePdf {
    fn clip(&self) -> &Contours {
        self.clips.last().unwrap()
    }

    fn check_paint(&self) -> Result<()> {
        ensure!(!self.soft_mask, "PDF soft masks require compositing");
        ensure!(
            self.blend == pdf::BlendMode::Normal,
            "PDF blend modes require compositing"
        );
        Ok(())
    }

    fn record(&mut self, result: Result<()>) {
        if self.error.is_none() {
            self.error = result.err();
        }
    }

    fn path(
        &mut self,
        path: &BezPath,
        transform: Affine,
        paint: &pdf::Paint<'_>,
        mode: &pdf::PathDrawMode,
    ) -> Result<()> {
        self.check_paint()?;
        if self.clip().is_empty() {
            return Ok(());
        }
        let Some(local) = tiny_path(path) else {
            return Ok(());
        };
        let matrix = tiny_transform(transform)?;
        let data = local
            .clone()
            .transform(matrix)
            .ok_or_else(|| anyhow!("invalid PDF path transform"))?;
        if let pdf::PathDrawMode::Stroke(stroke) = mode {
            let sx = matrix.sx.hypot(matrix.ky);
            let sy = matrix.kx.hypot(matrix.sy);
            let b = (transform * path).bounding_box().inflate(
                f64::from(stroke.line_width * sx.max(sy) * stroke.miter_limit.max(1.)),
                f64::from(stroke.line_width * sx.max(sy) * stroke.miter_limit.max(1.)),
            );
            if stroke.line_width > 0.
                && (sx - sy).abs() < 1e-5
                && (matrix.sx * matrix.kx + matrix.ky * matrix.sy).abs() < 1e-5
                && stroke.dash_offset.abs() < 1e-6
                && paths::contains_rect(self.clip(), rect(b))
                && let pdf::Paint::Color(color) = paint
                && let Some(mut shape) = paths::shape(&data, [0., 0.], None)
            {
                shape.stroke = Some(Stroke {
                    paint: Brush::Solid {
                        color: color.to_rgba().to_rgba8(),
                    },
                    width: f64::from(stroke.line_width * sx),
                    cap: match stroke.line_cap {
                        kurbo::Cap::Butt => "flat",
                        kurbo::Cap::Round => "rnd",
                        kurbo::Cap::Square => "sq",
                    }
                    .into(),
                    join: match stroke.line_join {
                        kurbo::Join::Miter => "miter",
                        kurbo::Join::Round => "round",
                        kurbo::Join::Bevel => "bevel",
                    }
                    .into(),
                    dash: stroke
                        .dash_array
                        .iter()
                        .map(|v| f64::from(*v * sx))
                        .collect(),
                    miter_limit: f64::from(stroke.miter_limit),
                });
                self.elements.push(Element::Shape(shape));
                return Ok(());
            }
            let mut local = local;
            if !stroke.dash_array.is_empty() {
                let dash = tiny::StrokeDash::new(stroke.dash_array.to_vec(), stroke.dash_offset)
                    .ok_or_else(|| anyhow!("invalid PDF stroke dash"))?;
                let Some(dashed) = local.dash(&dash, sx.max(sy)) else {
                    return Ok(());
                };
                local = dashed;
            }
            let outlined = local
                .stroke(
                    &tiny::Stroke {
                        // A zero-width PDF stroke is a device hairline.
                        width: if stroke.line_width == 0. {
                            0.1 / sx.max(sy).max(1e-6)
                        } else {
                            stroke.line_width
                        },
                        line_cap: match stroke.line_cap {
                            kurbo::Cap::Butt => tiny::LineCap::Butt,
                            kurbo::Cap::Round => tiny::LineCap::Round,
                            kurbo::Cap::Square => tiny::LineCap::Square,
                        },
                        line_join: match stroke.line_join {
                            kurbo::Join::Miter => tiny::LineJoin::Miter,
                            kurbo::Join::Round => tiny::LineJoin::Round,
                            kurbo::Join::Bevel => tiny::LineJoin::Bevel,
                        },
                        miter_limit: stroke.miter_limit,
                        ..Default::default()
                    },
                    sx.max(sy),
                )
                .and_then(|p| p.transform(matrix));
            if let Some(outlined) = outlined {
                self.fill(&outlined, false, paint, true)?;
            }
        } else if let pdf::PathDrawMode::Fill(rule) = mode {
            self.fill(&data, *rule == pdf::FillRule::EvenOdd, paint, false)?;
        }
        Ok(())
    }

    fn fill(
        &mut self,
        data: &tiny::Path,
        even_odd: bool,
        paint: &pdf::Paint<'_>,
        is_stroke: bool,
    ) -> Result<()> {
        let b = data.compute_tight_bounds().map(|r| Rect {
            x: r.x().into(),
            y: r.y().into(),
            width: r.width().into(),
            height: r.height().into(),
        });
        let clipped;
        let data = if !even_odd && b.is_some_and(|b| paths::contains_rect(self.clip(), b)) {
            data
        } else {
            clipped = paths::path(&paths::intersect(
                &paths::simplify(data, even_odd),
                self.clip(),
            ));
            let Some(ref p) = clipped else {
                return Ok(());
            };
            p
        };
        self.paint_path(data, paint, is_stroke)
    }
}

impl<'a> Device<'a> for NativePdf {
    fn set_soft_mask(&mut self, mask: Option<pdf::SoftMask<'a>>) {
        if self.soft_mask != mask.is_some() {
            self.flush_text();
        }
        self.soft_mask = mask.is_some();
    }
    fn set_blend_mode(&mut self, mode: pdf::BlendMode) {
        if self.blend != mode {
            self.flush_text();
        }
        self.blend = mode;
    }
    fn draw_path(
        &mut self,
        path: &BezPath,
        transform: Affine,
        paint: &pdf::Paint<'a>,
        mode: &pdf::PathDrawMode,
    ) {
        self.flush_text();
        if self.error.is_some() {
            return;
        }
        let result = self.path(path, transform, paint, mode);
        self.record(result);
    }
    fn push_clip_path(&mut self, clip: &pdf::ClipPath) {
        self.flush_text();
        let next = tiny_path(&clip.path).map_or_else(Vec::new, |p| {
            paths::simplify(&p, clip.fill == pdf::FillRule::EvenOdd)
        });
        self.clips.push(paths::intersect(self.clip(), &next));
    }
    fn pop_clip_path(&mut self) {
        self.flush_text();
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }
    fn push_transparency_group(
        &mut self,
        opacity: f32,
        mask: Option<pdf::SoftMask<'a>>,
        blend: pdf::BlendMode,
    ) {
        self.flush_text();
        self.groups.push((self.elements.len(), opacity));
        let result = if mask.is_some() || blend != pdf::BlendMode::Normal {
            Err(anyhow!(
                "PDF masked or blended transparency groups require compositing"
            ))
        } else {
            Ok(())
        };
        self.record(result);
    }
    fn pop_transparency_group(&mut self) {
        self.flush_text();
        if let Some((start, opacity)) = self.groups.pop()
            && self.elements.len() > start
            && opacity < 1.
        {
            let mut group = Element::group(self.elements.split_off(start));
            if let Element::Group(g) = &mut group {
                g.opacity = f64::from(opacity);
            }
            self.elements.push(group);
        }
    }
    fn draw_glyph(
        &mut self,
        glyph: &pdf::font::Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        paint: &pdf::Paint<'a>,
        mode: &pdf::GlyphDrawMode,
    ) {
        if self.error.is_some() || matches!(mode, pdf::GlyphDrawMode::Invisible) {
            return;
        }
        let result = self.glyph(glyph, transform, glyph_transform, paint, mode);
        self.record(result);
    }
    fn draw_image(&mut self, image: pdf::Image<'a, '_>, transform: Affine) {
        self.flush_text();
        if self.error.is_some() {
            return;
        }
        let result = self.image(image, transform);
        self.record(result);
    }
}

fn rectangle(r: Rect) -> Contours {
    vec![vec![
        [r.x, r.y],
        [r.right(), r.y],
        [r.right(), r.bottom()],
        [r.x, r.bottom()],
    ]]
}
fn rect(r: kurbo::Rect) -> Rect {
    Rect {
        x: r.x0,
        y: r.y0,
        width: r.width(),
        height: r.height(),
    }
}
fn tiny_transform(t: Affine) -> Result<tiny::Transform> {
    let [a, b, c, d, e, f] = t.as_coeffs();
    ensure!(
        [a, b, c, d, e, f].iter().all(|v| v.is_finite()) && t.determinant().abs() > 1e-15,
        "singular PDF transform"
    );
    Ok(tiny::Transform::from_row(
        a as f32, b as f32, c as f32, d as f32, e as f32, f as f32,
    ))
}
fn tiny_path(path: &BezPath) -> Option<tiny::Path> {
    let mut builder = tiny::PathBuilder::new();
    for el in path.elements() {
        match *el {
            kurbo::PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
            kurbo::PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
            kurbo::PathEl::QuadTo(c, p) => {
                builder.quad_to(c.x as f32, c.y as f32, p.x as f32, p.y as f32)
            }
            kurbo::PathEl::CurveTo(a, b, p) => builder.cubic_to(
                a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32,
            ),
            kurbo::PathEl::ClosePath => builder.close(),
        }
    }
    builder.finish()
}
