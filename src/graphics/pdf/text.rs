use super::*;
use std::collections::BTreeMap;
use typst::{
    foundations::Bytes,
    layout::Abs,
    text::{Font, FontFlags, FontInstance, FontStyle},
};

#[derive(Default)]
pub(super) struct Fonts(BTreeMap<u128, Option<FontInstance>>);

impl Fonts {
    fn get(&mut self, glyph: &pdf::font::OutlineGlyph) -> Option<(char, FontInstance)> {
        let character = match glyph.as_unicode()? {
            pdf::hayro_cmap::BfString::Char(c) => c,
            pdf::hayro_cmap::BfString::String(s) => {
                let mut cs = s.chars();
                let c = cs.next()?;
                if cs.next().is_some() {
                    return None;
                }
                c
            }
        };
        // PDF glyph order alone does not supply bidi runs or shaping clusters.
        if unicode_bidi::BidiInfo::new(&character.to_string(), None).has_rtl()
            || character.is_control()
        {
            return None;
        }
        let font = self
            .0
            .entry(glyph.font_cache_key())
            .or_insert_with(|| {
                let source = glyph.font_data()?;
                let data = source.data.as_ref().as_ref();
                let original = Font::new(Bytes::new(data.to_vec()), 0)?;
                if !original.info().axes.is_empty() {
                    return None;
                }
                // Distinct PDF subsets can have the same family name but unrelated
                // glyph numbering. Give each font program its own Office family.
                let mut hash = 0xcbf29ce484222325u64;
                for byte in data {
                    hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
                }
                let prefix: String = original.info().family.chars().take(12).collect();
                let family = format!("{prefix} P{hash:016x}");
                use write_fonts::{
                    FontBuilder,
                    from_obj::ToOwnedTable,
                    read::{FontRef, TableProvider},
                };
                let parsed = FontRef::new(data).ok()?;
                let mut names: write_fonts::tables::name::Name =
                    parsed.name().ok()?.to_owned_table();
                for name in &mut names.name_record {
                    match name.name_id.to_u16() {
                        1 | 3 | 4 | 16 | 21 => name.string = family.clone().into(),
                        6 => {
                            name.string = family
                                .chars()
                                .filter(char::is_ascii_alphanumeric)
                                .collect::<String>()
                                .into()
                        }
                        _ => {}
                    }
                }
                let mut builder = FontBuilder::new();
                builder.add_table(&names).ok()?;
                builder.copy_missing_tables(parsed);
                let font = Font::new(Bytes::new(builder.build()), 0)?;
                Some(font.clone().instantiate(
                    font.info().variant,
                    Abs::pt(12.),
                    &Default::default(),
                ))
            })
            .as_ref()?;
        let id = font.ttf().glyph_index(character)?;
        if u32::from(id.0) != glyph.glyph_id().to_u32() {
            return None;
        }
        let nominal =
            f64::from(font.ttf().glyph_hor_advance(id)?) / f64::from(font.ttf().units_per_em());
        let specified = f64::from(glyph.advance_width()?) / 1000.;
        // Hayro stretches outlines to PDF Widths. Office's spacing cannot
        // reproduce a per-glyph horizontal stretch within one text line.
        if !character.is_whitespace() && (nominal - specified).abs() > 0.001 {
            return None;
        }
        Some((character, font.clone()))
    }
}

struct Atom {
    character: char,
    font: FontInstance,
    size: f64,
    x: f64,
    nominal: f64,
    advance: f64,
    color: [u8; 4],
}

pub(super) struct Line {
    transform: Affine,
    atoms: Vec<Atom>,
}

impl NativePdf {
    pub(super) fn glyph<'a>(
        &mut self,
        glyph: &pdf::font::Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        paint: &pdf::Paint<'a>,
        mode: &pdf::GlyphDrawMode,
    ) -> Result<()> {
        self.check_paint()?;
        if self.clip().is_empty() {
            return Ok(());
        }
        match glyph {
            pdf::font::Glyph::Type3(g) => {
                self.flush_text();
                g.interpret(self, transform, glyph_transform, paint);
            }
            pdf::font::Glyph::Outline(g) => {
                let full = transform * glyph_transform;
                let m = full * Affine::scale_non_uniform(1000., -1000.);
                let [a, b, c, d, x, y] = m.as_coeffs();
                let size = c.hypot(d);
                let ink = rect((full * g.outline()).bounding_box());
                if matches!(mode, pdf::GlyphDrawMode::Fill)
                    && let pdf::Paint::Color(color) = paint
                    && size > 1e-8
                    && (a * c + b * d).abs() < 1e-6 * size * size
                    && paths::contains_rect(self.clip(), ink)
                    && let Some((character, font)) = self.fonts.get(g)
                {
                    let basis = Affine::new([a / size, b / size, c / size, d / size, x, y]);
                    let next = self.text.as_ref().and_then(|line| {
                        let previous = line.transform.as_coeffs();
                        let next = basis.as_coeffs();
                        if !(0..4).all(|i| (previous[i] - next[i]).abs() < 1e-6) {
                            return None;
                        }
                        let position = line.transform.inverse() * kurbo::Point::new(x, y);
                        let last = line.atoms.last()?;
                        let gap = position.x - last.x - last.advance;
                        (position.y.abs() < 0.05
                            && position.x >= last.x - 0.01
                            && gap.abs() <= size * 2.)
                            .then_some(position.x)
                    });
                    if next.is_none() {
                        self.flush_text();
                        self.text = Some(Line {
                            transform: basis,
                            atoms: Vec::new(),
                        });
                    }
                    let nominal = font
                        .x_advance(g.glyph_id().to_u32().try_into().unwrap())
                        .map_or(0., |v| v.at(Abs::pt(size)).to_pt());
                    self.text.as_mut().unwrap().atoms.push(Atom {
                        character,
                        font,
                        size,
                        x: next.unwrap_or(0.),
                        nominal,
                        advance: f64::from(g.advance_width().unwrap_or(0.)) * size / 1000.,
                        color: color.to_rgba().to_rgba8(),
                    });
                } else {
                    // Font programs or Unicode mappings can be absent in a PDF.
                    // Such glyphs remain native editable outlines, never images.
                    self.flush_text();
                    let mode = match mode {
                        pdf::GlyphDrawMode::Fill => pdf::PathDrawMode::Fill(pdf::FillRule::NonZero),
                        pdf::GlyphDrawMode::Stroke(stroke) => {
                            pdf::PathDrawMode::Stroke(stroke.clone())
                        }
                        pdf::GlyphDrawMode::Invisible => return Ok(()),
                    };
                    // Stroke width is defined before the glyph's own matrix.
                    self.path(&(glyph_transform * g.outline()), transform, paint, &mode)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn flush_text(&mut self) {
        let Some(line) = self.text.take() else {
            return;
        };
        if line.atoms.is_empty() {
            return;
        }
        let mut ascent = 0_f64;
        let mut descent = 0_f64;
        let mut width = 0_f64;
        let mut runs: Vec<Run> = Vec::new();
        for (i, atom) in line.atoms.iter().enumerate() {
            let info = atom.font.info();
            ascent = ascent.max(atom.font.metrics().ascender.at(Abs::pt(atom.size)).to_pt());
            descent = descent.max(-atom.font.metrics().descender.at(Abs::pt(atom.size)).to_pt());
            width = width.max(atom.x + atom.advance);
            let tracking = line
                .atoms
                .get(i + 1)
                .map_or(atom.advance - atom.nominal, |next| {
                    next.x - atom.x - atom.nominal
                });
            let style = TextStyle {
                rtl: false,
                font: crate::assets::fonts::family(&atom.font),
                pitch_family: if info.flags.contains(FontFlags::MONOSPACE) {
                    0x31
                } else {
                    0x12
                },
                size: atom.size,
                baseline: 0.,
                kerning: false,
                letter_spacing: tracking,
                bold: !crate::assets::fonts::baked(&atom.font)
                    && info.variant.weight.to_number() >= 600,
                italic: !crate::assets::fonts::baked(&atom.font)
                    && info.variant.style != FontStyle::Normal,
                underline: false,
                strike: false,
                color: atom.color,
                fill: None,
                outline: None,
                language: "en-US".into(),
            };
            if let Some(last) = runs.last_mut().filter(|r| r.style == style) {
                last.text.push(atom.character);
            } else {
                runs.push(Run {
                    text: atom.character.into(),
                    style,
                    hyperlink: None,
                    math: None,
                    math_inline: false,
                    advances: Vec::new(),
                    source_line: 0,
                    source_width: None,
                });
            }
        }
        let block = Element::Text(TextBlock {
            source_id: format!("pdf:{}", self.elements.len()),
            role: "pdf_text".into(),
            vertical: None,
            clip: None,
            font_scale: None,
            wrap: false,
            bounds: Rect {
                x: 0.,
                y: -ascent,
                width: (width + 0.5).max(0.01),
                height: ascent + descent + 0.5,
            },
            paragraphs: vec![Paragraph {
                rtl: false,
                margin_right: 0.,
                runs,
                lines: Vec::new(),
                level: 0,
                bullet: None,
                margin_left: 0.,
                indent: 0.,
                alignment: "l".into(),
                line_spacing: ascent + descent,
                space_before: 0.,
                space_after: 0.,
                tab_stops: Vec::new(),
                break_latin: false,
            }],
        });
        self.elements.push(crate::geometry::transforms::apply(
            vec![block],
            line.transform.as_coeffs(),
        ));
    }
}

pub(super) fn collect_fonts(image: &PdfImage) -> Vec<FontInstance> {
    struct Collector(Fonts);
    impl<'a> Device<'a> for Collector {
        fn set_soft_mask(&mut self, _: Option<pdf::SoftMask<'a>>) {}
        fn set_blend_mode(&mut self, _: pdf::BlendMode) {}
        fn draw_path(
            &mut self,
            _: &BezPath,
            _: Affine,
            paint: &pdf::Paint<'a>,
            mode: &pdf::PathDrawMode,
        ) {
            if let pdf::Paint::Pattern(p) = paint
                && let pdf::pattern::Pattern::Tiling(p) = p.as_ref()
            {
                p.interpret(self, p.matrix, matches!(mode, pdf::PathDrawMode::Stroke(_)));
            }
        }
        fn push_clip_path(&mut self, _: &pdf::ClipPath) {}
        fn pop_clip_path(&mut self) {}
        fn push_transparency_group(
            &mut self,
            _: f32,
            _: Option<pdf::SoftMask<'a>>,
            _: pdf::BlendMode,
        ) {
        }
        fn pop_transparency_group(&mut self) {}
        fn draw_image(&mut self, _: pdf::Image<'a, '_>, _: Affine) {}
        fn draw_glyph(
            &mut self,
            g: &pdf::font::Glyph<'a>,
            t: Affine,
            gt: Affine,
            p: &pdf::Paint<'a>,
            _: &pdf::GlyphDrawMode,
        ) {
            match g {
                pdf::font::Glyph::Outline(g) => {
                    self.0.get(g);
                }
                pdf::font::Glyph::Type3(g) => g.interpret(self, t, gt, p),
            }
        }
    }
    let page = image.page();
    let cache = pdf::InterpreterCache::new();
    let settings = pdf::InterpreterSettings {
        render_annotations: false,
        ..Default::default()
    };
    let mut context = pdf::Context::new(
        page.initial_transform(true).to_kurbo(),
        kurbo::Rect::new(0., 0., image.width().into(), image.height().into()),
        &cache,
        page.xref(),
        settings,
    );
    let mut collector = Collector(Fonts::default());
    pdf::interpret_page(page, &mut context, &mut collector);
    collector.0.0.into_values().flatten().collect()
}
