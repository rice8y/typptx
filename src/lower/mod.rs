//! Lower compiled Typst pages to the portable presentation model.
mod animation;
mod bibliography;
mod links;
mod lists;
mod pictures;
mod structure;
mod table;
mod text;

use crate::compiler::capture::{Capture, Kind, Leaf};
use crate::compiler::diagnostics::{self, Origin};
pub use crate::graphics::rgba;
use crate::ir::*;
use crate::lower::pictures::{native_image, prepare_inline_objects};
use crate::lower::structure::{parent_structure, structure};
use crate::lower::text::{fragment_owner, native_fragment};
use anyhow::{Result, anyhow, ensure};
use std::collections::BTreeMap;
use typst::layout::FrameItem;
use typst::visualize::Paint;
use typst_layout::PagedDocument;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum MathFormat {
    /// Editable Office Math equations.
    #[default]
    Office,
    /// Equations rendered by Typst and embedded as SVG pictures.
    Svg,
}

#[derive(Default)]
pub struct Options {
    pub animations: AnimationFormat,
    pub allow_image_fallback: bool,
    pub math_format: MathFormat,
    /// Maximum resolution of original raster images at their placed size.
    /// Also sets the PNG preview and fallback rendering DPI. None preserves
    /// original raster bytes and uses 144 DPI for rendered PNGs. Must be positive.
    pub image_dpi: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum AnimationFormat {
    /// Combine package overlays into native click animations.
    #[default]
    Native,
    /// Keep each compiled overlay as a separate static slide.
    Slides,
}

pub fn convert(document: &PagedDocument) -> Result<Presentation> {
    convert_with_options(document, &Options::default())
}

/// Lower a compiled document. Call `CompilerWorld::locate_diagnostics` on the
/// result before serializing to include source file positions in diagnostics.
pub fn convert_with_options(document: &PagedDocument, options: &Options) -> Result<Presentation> {
    convert_document(document, options, true)
}

pub(crate) fn convert_fragment(
    document: &PagedDocument,
    options: &Options,
) -> Result<Vec<Element>> {
    let p = convert_document(document, options, false)?;
    if !p.diagnostics.is_empty() {
        let origin = p.diagnostics.iter().find_map(|d| {
            Some(Origin {
                span: d.span?,
                element: d.element.clone().unwrap_or_else(|| "object".into()),
            })
        });
        return Err(diagnostics::at(
            anyhow!(
                p.diagnostics
                    .iter()
                    .map(|d| d.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            origin,
        ));
    }
    Ok(p.slides.into_iter().flat_map(|s| s.elements).collect())
}

fn convert_document(
    document: &PagedDocument,
    options: &Options,
    embed_fonts: bool,
) -> Result<Presentation> {
    ensure!(
        options.image_dpi != Some(0),
        "image DPI must be greater than zero"
    );
    let mut capture = Capture::new(document);
    let font_capture = capture.clone();
    let mut inline_objects = prepare_inline_objects(&mut capture, options.image_dpi);
    let mut math_images = if options.math_format == MathFormat::Svg {
        crate::math::svg::prepare(&mut capture, document, options.image_dpi)?
    } else {
        (0..document.pages().len())
            .map(|_| BTreeMap::new())
            .collect()
    };
    let notes = crate::compiler::notes::extract(document)?;
    let mut output = Presentation {
        schema_version: 20,
        slides: Vec::new(),
        diagnostics: Vec::new(),
        fonts: Vec::new(),
    };
    for (page_idx, page) in document.pages().iter().enumerate() {
        let leaves = &capture.pages[page_idx];
        let mut used = vec![false; leaves.len()];
        let mut blocked = vec![None; leaves.len()];
        let mut native: BTreeMap<usize, Vec<Element>> = BTreeMap::new();
        // Outer structural blocks own their contents. Do not convert a failed
        // list/table into apparently editable fragments of its inner text.
        let priorities = [
            Kind::Table,
            Kind::Bibliography,
            Kind::List,
            Kind::Enum,
            Kind::Heading,
            Kind::Paragraph,
            Kind::Equation,
        ];
        for kind in priorities {
            for (idx, node) in capture.nodes.iter().enumerate() {
                if node.kind != kind {
                    continue;
                }
                let Some(np) = node.pages.get(&page_idx) else {
                    continue;
                };
                if np.leaves.is_empty()
                    || np.leaves.iter().any(|&i| used[i] || blocked[i].is_some())
                {
                    continue;
                }
                if parent_structure(&capture, idx).is_some() {
                    continue;
                }
                // Artifacts (headers/footers) can be nested inside a paragraph
                // whose layout spans pages. They belong to separate objects.
                let ids: Vec<_> = np
                    .leaves
                    .iter()
                    .copied()
                    .filter(|&id| {
                        !leaves[id].ancestors.iter().any(|&n| {
                            n > idx && capture.nodes[n].content.elem().name() == "artifact"
                        })
                    })
                    .collect();
                if ids.is_empty() {
                    continue;
                }
                match structure(&capture, idx, page_idx, &ids, options) {
                    Ok(element) => {
                        for &i in &ids {
                            used[i] = true;
                        }
                        // A standalone SVG equation needs no empty text box.
                        // List markers and table cells still need their native container.
                        let only_math = ids.iter().all(|&id| {
                            capture.is_svg_math(&leaves[id])
                                || capture.inline_objects.contains(&(page_idx, id))
                        });
                        let mut elements = if only_math
                            && matches!(&element, Element::Text(t) if t.paragraphs.iter().all(|p| p.bullet.is_none()))
                        {
                            Vec::new()
                        } else {
                            vec![element]
                        };
                        let has_inline = ids
                            .iter()
                            .any(|id| inline_objects[page_idx].contains_key(id));
                        for &id in &ids {
                            if let Some(image) = inline_objects[page_idx].remove(&id) {
                                elements.extend(image);
                            }
                            if let Some(image) = math_images[page_idx].remove(&id) {
                                elements.push(image);
                            }
                        }
                        if has_inline && elements.len() > 1 {
                            elements = vec![Element::group(elements)];
                        }
                        native.insert(ids[0], links::attach(&capture, page_idx, &ids, elements));
                    }
                    Err(error) => {
                        if matches!(kind, Kind::Paragraph | Kind::Heading) {
                            continue;
                        }
                        for &i in &ids {
                            blocked[i] = Some(idx);
                        }
                        output.diagnostics.push(diagnostics::from_error(
                            page_idx + 1,
                            node.id(),
                            if options.allow_image_fallback {
                                "drawing_fallback"
                            } else {
                                "unsupported_structure"
                            },
                            node.content.elem().name(),
                            error,
                            Origin::node(&capture, idx, page_idx),
                        ));
                    }
                }
            }
        }
        let size = page.frame.size();
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: size.x.to_pt(),
            height: size.y.to_pt(),
        };
        let mut slide = Slide {
            width: bounds.width,
            height: bounds.height,
            background: None,
            elements: Vec::new(),
            notes: notes[page_idx].clone(),
            links: Vec::new(),
            animation: None,
        };
        let background = match page.fill_or_white() {
            Some(Paint::Solid(color)) => {
                slide.background = Some(rgba(color));
                Ok(Vec::new())
            }
            Some(paint @ Paint::Tiling(_)) => {
                crate::graphics::tiling::rectangle(paint, bounds, options)
            }
            Some(ref paint) => crate::graphics::brush(paint).map(|fill| {
                vec![Element::Shape(VectorShape {
                    bounds,
                    commands: vec![
                        PathCommand::Move([0., 0.]),
                        PathCommand::Line([bounds.width, 0.]),
                        PathCommand::Line([bounds.width, bounds.height]),
                        PathCommand::Line([0., bounds.height]),
                        PathCommand::Close,
                    ],
                    fill: Some(fill),
                    stroke: None,
                })]
            }),
            None => Ok(Vec::new()),
        };
        match background {
            Ok(elements) => slide.elements.extend(elements),
            Err(error) => {
                output.diagnostics.push(diagnostics::from_error(
                    page_idx + 1,
                    "background".into(),
                    if options.allow_image_fallback {
                        "drawing_fallback"
                    } else {
                        "unsupported_element"
                    },
                    "page-background",
                    error,
                    None,
                ));
                if options.allow_image_fallback {
                    let mut background = page.clone();
                    background.frame = typst::layout::Frame::hard(page.frame.size());
                    slide
                        .elements
                        .push(pictures::drawing(&background, bounds, options.image_dpi)?);
                }
            }
        }
        // Maintain paint order: drawings before, between, and after native
        // objects are separate layers instead of one full-slide background.
        let mut i = 0;
        while i < leaves.len() {
            if let Some(elements) = native.remove(&i) {
                slide.elements.extend(elements);
            }
            if used[i] || (blocked[i].is_some() && !options.allow_image_fallback) {
                i += 1;
                continue;
            }
            if blocked[i].is_none() {
                let start = i;
                let result = match &leaves[i].item {
                    FrameItem::Text(_) => {
                        let owner = fragment_owner(&capture, &leaves[i]);
                        i += 1;
                        while i < leaves.len()
                            && !used[i]
                            && blocked[i].is_none()
                            && !native.contains_key(&i)
                            && matches!(leaves[i].item, FrameItem::Text(_))
                            && fragment_owner(&capture, &leaves[i]) == owner
                            && leaves[i].frame == leaves[start].frame
                            && (leaves[i].position.1 - leaves[start].position.1).abs() < 0.1
                        {
                            i += 1;
                        }
                        native_fragment(&capture, page_idx, &(start..i).collect::<Vec<_>>()).map(
                            |t| {
                                if (start..i).all(|id| {
                                    capture.inline_objects.contains(&(page_idx, id))
                                        || capture.is_svg_math(&leaves[id])
                                }) {
                                    vec![]
                                } else {
                                    vec![Element::Text(t)]
                                }
                            },
                        )
                    }
                    FrameItem::Shape(..) => {
                        i += 1;
                        crate::graphics::convert_with_options(&leaves[start], options)
                    }
                    FrameItem::Image(image, size, _) => {
                        i += 1;
                        native_image(&leaves[start], image, *size, options.image_dpi)
                    }
                    FrameItem::Link(..) => {
                        if let Some(link) = links::empty_region(&capture, page_idx, i) {
                            slide.links.push(link);
                        }
                        i += 1;
                        continue;
                    }
                    _ => {
                        i += 1;
                        Err(anyhow!("unsupported display-list item"))
                    }
                };
                match result {
                    Ok(elements) => {
                        slide.elements.extend(links::attach(
                            &capture,
                            page_idx,
                            &(start..i).collect::<Vec<_>>(),
                            elements,
                        ));
                        for id in start..i {
                            if let Some(image) = inline_objects[page_idx].remove(&id) {
                                slide.elements.extend(image);
                            }
                            if let Some(image) = math_images[page_idx].remove(&id) {
                                slide.elements.push(image);
                            }
                        }
                    }
                    Err(error) => {
                        output.diagnostics.push(diagnostics::from_error(
                            page_idx + 1,
                            format!("display-list:{start}"),
                            if options.allow_image_fallback {
                                "drawing_fallback"
                            } else {
                                "unsupported_element"
                            },
                            "object",
                            error,
                            Origin::leaf(&capture, page_idx, start),
                        ));
                        if options.allow_image_fallback {
                            let fallback = pictures::fallback(
                                page,
                                &font_capture.pages[page_idx],
                                start..i,
                                options.image_dpi,
                            )?;
                            slide.elements.extend(links::attach(
                                &font_capture,
                                page_idx,
                                &(start..i).collect::<Vec<_>>(),
                                vec![fallback],
                            ));
                        }
                    }
                }
                continue;
            }
            let start = i;
            while i < leaves.len()
                && blocked[i] == blocked[start]
                && !used[i]
                && !native.contains_key(&i)
            {
                i += 1;
            }
            if !leaves[start..i].iter().any(Leaf::is_drawable) {
                continue;
            }
            let fallback = pictures::fallback(
                page,
                &font_capture.pages[page_idx],
                start..i,
                options.image_dpi,
            )?;
            slide.elements.extend(links::attach(
                &font_capture,
                page_idx,
                &(start..i).collect::<Vec<_>>(),
                vec![fallback],
            ));
        }
        slide.elements = slide
            .elements
            .into_iter()
            .map(crate::graphics::gradients::expand)
            .collect();
        output.slides.push(slide);
    }
    if embed_fonts {
        crate::assets::fonts::collect(&font_capture, document, &mut output);
        if options.animations == AnimationFormat::Native {
            animation::combine(&mut output, &crate::compiler::overlays::groups(document)?)?;
        }
    }
    Ok(output)
}
