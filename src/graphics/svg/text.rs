use super::*;
use typst::{
    foundations::Bytes,
    layout::Abs,
    text::{Font, FontFlags},
};

pub(super) fn convert(
    tree: &usvg::Tree,
    text: &usvg::Text,
    scale: Transform,
    bounds: Rect,
    clip: &Contours,
) -> Result<Vec<Element>> {
    let vertical = text.writing_mode() == usvg::WritingMode::TopToBottom;
    ensure!(
        text.rotate().iter().all(|x| x.abs() < 1e-6),
        "individually rotated SVG characters need native text-on-path layout"
    );
    let glyphs: Vec<_> = text
        .layouted()
        .iter()
        .flat_map(|s| &s.positioned_glyphs)
        .collect();
    let mut cursor = 0;
    let mut char_cursor = 0;
    let mut out = Vec::new();
    let ts = scale.pre_concat(text.abs_transform());
    ensure!(
        (ts.sx * ts.kx + ts.ky * ts.sy).abs() < 1e-5,
        "PowerPoint cannot shear editable SVG text"
    );
    for (idx, chunk) in text.chunks().iter().enumerate() {
        let char_start = char_cursor;
        let char_count = chunk.text().chars().count();
        char_cursor += char_count;
        let has_rtl = unicode_bidi::BidiInfo::new(chunk.text(), None).has_rtl();
        let adjusted = (char_start + usize::from(!has_rtl)..char_cursor).any(|i| {
            text.dx().get(i).is_some_and(|x| x.abs() > 1e-6)
                || text.dy().get(i).is_some_and(|y| y.abs() > 1e-6)
        });
        ensure!(
            matches!(chunk.text_flow(), usvg::TextFlow::Linear),
            "SVG text on a path needs native WordArt mapping"
        );
        let start = cursor;
        let mut chars = 0;
        while cursor < glyphs.len() && chars < chunk.text().chars().count() {
            chars += glyphs[cursor].text.chars().count();
            cursor += 1;
        }
        if has_rtl {
            while cursor < glyphs.len() && glyphs[cursor].text.is_empty() {
                cursor += 1;
            }
        }
        let selected = &glyphs[start..cursor];
        if selected.is_empty() {
            continue;
        }
        let mut baseline = f64::from(selected[0].transform().ty);
        // A chunk is one line of source text; preserve source runs and bidi
        // order rather than splitting its glyphs into individual text boxes.
        let mut x = f64::INFINITY;
        let mut right = f64::NEG_INFINITY;
        let mut ascent = 0_f64;
        let mut descent = 0_f64;
        let mut fonts = Vec::new();
        let upright = vertical
            && selected
                .iter()
                .any(|g| g.transform().ky.abs() < g.transform().sx.abs());
        let mut top = f64::INFINITY;
        let mut bottom = f64::NEG_INFINITY;
        let mut axis = 0.;
        let mut em = 0_f64;
        let mut ink: Option<Rect> = None;
        for glyph in selected {
            let font = tree
                .fontdb()
                .with_face_data(glyph.font, |data, index| {
                    Font::new(Bytes::new(data.to_vec()), index)
                })
                .flatten()
                .ok_or_else(|| anyhow!("missing SVG text font"))?;
            let font = font.clone().instantiate(
                font.info().variant,
                Abs::pt(f64::from(glyph.font_size())),
                &Default::default(),
            );
            let t = glyph.transform();
            if let Some(b) = font.ttf().glyph_bounding_box(glyph.id) {
                let b = tiny::Rect::from_ltrb(
                    f32::from(b.x_min),
                    f32::from(b.y_min),
                    f32::from(b.x_max),
                    f32::from(b.y_max),
                )
                .and_then(|b| b.transform(glyph.outline_transform()));
                if let Some(b) = b {
                    let b = Rect {
                        x: f64::from(b.x()),
                        y: f64::from(b.y()),
                        width: f64::from(b.width()),
                        height: f64::from(b.height()),
                    };
                    ink = Some(ink.map_or(b, |previous| previous.union(b)));
                }
            }
            x = x.min(f64::from(t.tx));
            let advance = font
                .x_advance(glyph.id.0)
                .map_or(0., |a| a.at(Abs::pt(f64::from(glyph.font_size()))).to_pt());
            right = right.max(f64::from(t.tx) + advance);
            if vertical {
                let size = f64::from(glyph.font_size());
                let middle = (f64::from(font.ttf().ascender()) + f64::from(font.ttf().descender()))
                    * size
                    / f64::from(font.ttf().units_per_em())
                    / 2.;
                let is_upright = t.ky.abs() < t.sx.abs();
                let start = f64::from(t.ty)
                    - if is_upright {
                        middle + advance / 2.
                    } else {
                        0.
                    };
                top = top.min(start);
                bottom = bottom.max(start + advance);
                if fonts.is_empty() {
                    axis = f64::from(t.tx) + if is_upright { advance / 2. } else { middle };
                }
                em = em.max(size);
            }
            ascent = ascent.max(
                font.metrics()
                    .ascender
                    .at(Abs::pt(f64::from(glyph.font_size())))
                    .to_pt(),
            );
            descent = descent.max(
                -font
                    .metrics()
                    .descender
                    .at(Abs::pt(f64::from(glyph.font_size())))
                    .to_pt(),
            );
            fonts.push(font);
        }
        let clusters = adjusted
            .then(|| super::text_offsets::resolve(text, chunk, selected, &fonts, char_start))
            .transpose()?;
        if let Some(clusters) = &clusters {
            baseline = f64::from(selected[clusters[0].glyph].transform().ty);
        }
        let local = if vertical {
            let width = if upright { em } else { ascent + descent };
            Rect {
                x: axis - width / 2.,
                y: top,
                width,
                height: (bottom - top + 0.5).max(0.01),
            }
        } else {
            Rect {
                x,
                y: baseline - ascent,
                width: (right - x + 0.5).max(0.01),
                height: ascent + descent + 0.5,
            }
        };
        let mut runs = Vec::new();
        for span in chunk.spans().iter().filter(|s| s.is_visible()) {
            let content = chunk
                .text()
                .get(span.start()..span.end())
                .ok_or_else(|| anyhow!("invalid SVG text span"))?;
            let mut offset = 0;
            let index = clusters
                .as_ref()
                .and_then(|cs| {
                    cs.iter()
                        .find(|c| c.source.start == span.start())
                        .map(|c| c.glyph)
                })
                .unwrap_or_else(|| {
                    selected
                        .iter()
                        .position(|g| {
                            let in_span = offset >= span.start();
                            offset += g.text.len();
                            in_span
                        })
                        .unwrap_or(0)
                });
            let font = &fonts[index];
            let family = crate::assets::fonts::family(font);
            let fill = span
                .fill()
                .map(|f| paint::convert(f.paint(), f.opacity().get(), Transform::identity(), local))
                .transpose()?;
            let color = match &fill {
                Some(Brush::Solid { color }) => *color,
                None => [0, 0, 0, 0],
                _ => [0, 0, 0, 255],
            };
            let outline = span
                .stroke()
                .map(|s| -> Result<Stroke> {
                    Ok(Stroke {
                        paint: paint::convert(
                            s.paint(),
                            s.opacity().get(),
                            Transform::identity(),
                            local,
                        )?,
                        width: f64::from(s.width().get()),
                        cap: match s.linecap() {
                            usvg::LineCap::Butt => "flat",
                            usvg::LineCap::Round => "rnd",
                            usvg::LineCap::Square => "sq",
                        }
                        .into(),
                        join: match s.linejoin() {
                            usvg::LineJoin::Round => "round",
                            usvg::LineJoin::Bevel => "bevel",
                            _ => "miter",
                        }
                        .into(),
                        dash: s
                            .dasharray()
                            .unwrap_or_default()
                            .iter()
                            .map(|d| f64::from(*d))
                            .collect(),
                        miter_limit: f64::from(s.miterlimit().get()),
                    })
                })
                .transpose()?;
            ensure!(
                !fill
                    .as_ref()
                    .is_some_and(crate::graphics::gradients::nonlinear)
                    && !outline
                        .as_ref()
                        .is_some_and(|s| crate::graphics::gradients::nonlinear(&s.paint)),
                "PowerPoint cannot preserve this gradient on editable SVG text"
            );
            let run = Run {
                text: content.into(),
                hyperlink: None,
                math: None,
                math_inline: false,
                advances: Vec::new(),
                source_line: 0,
                source_width: None,
                style: TextStyle {
                    rtl: unicode_bidi::BidiInfo::new(content, None)
                        .paragraphs
                        .first()
                        .is_some_and(|p| p.level.is_rtl()),
                    font: family,
                    pitch_family: if font.info().flags.contains(FontFlags::MONOSPACE) {
                        0x31
                    } else {
                        0x12
                    },
                    size: f64::from(span.font_size().get()),
                    baseline: if vertical {
                        0.
                    } else {
                        baseline - f64::from(selected[index].transform().ty)
                    },
                    letter_spacing: f64::from(span.letter_spacing()),
                    kerning: span.apply_kerning(),
                    bold: !crate::assets::fonts::baked(font) && span.font().weight() >= 600,
                    italic: !crate::assets::fonts::baked(font)
                        && span.font().style() != usvg::FontStyle::Normal,
                    underline: span.decoration().underline().is_some(),
                    strike: span.decoration().line_through().is_some(),
                    color,
                    fill,
                    outline,
                    language: "en-US".into(),
                },
            };
            if let Some(clusters) = &clusters {
                for cluster in clusters
                    .iter()
                    .filter(|c| c.source.start >= span.start() && c.source.end <= span.end())
                {
                    let index = cluster.glyph;
                    let glyph = selected[index];
                    let mut r = run.clone();
                    r.text = chunk.text()[cluster.source.clone()].to_owned();
                    r.style.rtl = cluster.rtl;
                    r.style.font = crate::assets::fonts::family(&fonts[index]);
                    r.style.baseline = baseline - f64::from(glyph.transform().ty);
                    r.style.letter_spacing += cluster.spacing;
                    if let Some(previous) =
                        runs.last_mut().filter(|p: &&mut Run| p.style == r.style)
                    {
                        previous.text.push_str(&r.text);
                    } else {
                        runs.push(r);
                    }
                }
            } else {
                runs.push(run);
            }
        }
        if runs.is_empty() {
            continue;
        }
        let rtl = clusters.as_ref().is_none_or(|cs| cs.iter().all(|c| c.rtl))
            && unicode_bidi::BidiInfo::new(chunk.text(), None)
                .paragraphs
                .first()
                .is_some_and(|p| p.level.is_rtl());
        let mut element = Element::Text(TextBlock {
            vertical: vertical.then(|| if upright { "eaVert" } else { "vert" }.into()),
            clip: None,
            source_id: format!("svg:{}:{idx}", text.id()),
            role: "svg_text".into(),
            bounds: local,
            font_scale: None,
            wrap: false,
            paragraphs: vec![Paragraph {
                rtl,
                margin_right: 0.,
                runs,
                lines: vec![],
                level: 0,
                bullet: None,
                margin_left: 0.,
                indent: 0.,
                alignment: if rtl { "r" } else { "l" }.into(),
                line_spacing: if vertical && upright {
                    em
                } else {
                    ascent + descent
                },
                space_before: 0.,
                space_after: 0.,
                tab_stops: vec![],
                break_latin: false,
            }],
        });
        let extent = text
            .abs_bounding_box()
            .transform(scale)
            .ok_or_else(|| anyhow!("invalid SVG text bounds"))?;
        let extent = Rect {
            x: f64::from(extent.left()),
            y: f64::from(extent.top()),
            width: f64::from(extent.width()),
            height: f64::from(extent.height()),
        };
        if !contains(clip, extent) {
            let inv = ts
                .invert()
                .ok_or_else(|| anyhow!("singular SVG text transform"))?;
            let local_clip = clip
                .iter()
                .map(|c| {
                    c.iter()
                        .map(|p| {
                            let mut p = tiny::Point::from_xy(p[0] as f32, p[1] as f32);
                            inv.map_point(&mut p);
                            [f64::from(p.x), f64::from(p.y)]
                        })
                        .collect()
                })
                .collect();
            let clip = paths::rectangular(&local_clip).ok_or_else(|| {
                anyhow!("editable SVG text requires a rectangular clip aligned with the text")
            })?;
            paths::check_text_clip(ink.unwrap_or(local), clip, vertical)?;
            if let Element::Text(t) = &mut element {
                t.clip = Some(clip);
            }
        }
        out.push(crate::geometry::transforms::apply(
            vec![element],
            [
                f64::from(ts.sx),
                f64::from(ts.ky),
                f64::from(ts.kx),
                f64::from(ts.sy),
                bounds.x + f64::from(ts.tx),
                bounds.y + f64::from(ts.ty),
            ],
        ));
    }
    Ok(out)
}
