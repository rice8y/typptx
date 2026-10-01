//! Map the portable presentation model to OOXML through ooxmlsdk.
//! The SDK owns serialization, package parts, relationships and validation.
mod defaults;
mod drawing;
mod math;
mod package;
mod table;
mod text;
pub use package::write;

use crate::ir::*;
use anyhow::{Result, ensure};
use ooxmlsdk::{
    parts::{presentation_document::PresentationDocument, slide_part::SlidePart},
    schemas::{a, p},
    sdk::SdkType,
};

fn image_part(
    doc: &mut PresentationDocument,
    slide: &SlidePart,
    ext: &str,
    bytes: &[u8],
) -> Result<String> {
    let content_type = match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        _ => "image/png",
    };
    let image = slide.add_image_part(doc, content_type)?;
    image.set_data(doc, bytes.to_vec())?;
    Ok(slide.get_id_of_part(doc, &image)?.into())
}
#[derive(Default)]
struct Relationships {
    links: Vec<String>,
}
impl Relationships {
    fn add(&mut self, _kind: &str, target: &str, _external: bool) -> String {
        if let Some(index) = self.links.iter().position(|url| url == target) {
            return format!("link{}", index + 1);
        }
        self.links.push(target.into());
        format!("link{}", self.links.len())
    }
}

fn textbox(block: &TextBlock, id: usize, rels: &mut Relationships) -> Result<p::ShapeTreeChoice> {
    let native = text::textbox(block, id, rels);
    if block
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .any(|r| r.math.is_some())
    {
        let mut fallback = block.clone();
        for r in fallback.paragraphs.iter_mut().flat_map(|p| &mut p.runs) {
            r.math = None;
        }
        math_compatibility(&native, &text::textbox(&fallback, id, rels))
    } else {
        Ok(p::ShapeTreeChoice::Shape(Box::new(native)))
    }
}
fn table_element(table: &Table, id: usize, rels: &mut Relationships) -> Result<p::ShapeTreeChoice> {
    let native = table::table(table, id, rels);
    if table
        .cells
        .iter()
        .flat_map(|c| &c.paragraphs)
        .flat_map(|p| &p.runs)
        .any(|r| r.math.is_some())
    {
        let mut fallback = table.clone();
        for r in fallback
            .cells
            .iter_mut()
            .flat_map(|c| &mut c.paragraphs)
            .flat_map(|p| &mut p.runs)
        {
            r.math = None;
        }
        math_compatibility(&native, &table::table(&fallback, id, rels))
    } else {
        Ok(p::ShapeTreeChoice::GraphicFrame(Box::new(native)))
    }
}
fn math_compatibility<T: SdkType>(native: &T, fallback: &T) -> Result<p::ShapeTreeChoice> {
    use ooxmlsdk::{common::XmlNamespace as N, namespaces::XmlKnownNamespace as K, schemas::mc};
    let mut native_bytes = Vec::new();
    native.write_to(&mut native_bytes)?;
    let mut fallback_bytes = Vec::new();
    fallback.write_to(&mut fallback_bytes)?;
    Ok(p::ShapeTreeChoice::AlternateContent(Box::new(
        mc::AlternateContent {
            xmlns: vec![N::known(K::Mc)],
            alternate_content_choice: vec![
                mc::AlternateContentChoice::Choice(Box::new(mc::Choice {
                    xmlns: vec![N::known(K::A14)],
                    requires: vec![N::known(K::A14)],
                    xml_children: vec![native_bytes.into_boxed_slice()],
                    ..Default::default()
                })),
                mc::AlternateContentChoice::Fallback(Box::new(mc::Fallback {
                    xml_children: vec![fallback_bytes.into_boxed_slice()],
                    ..Default::default()
                })),
            ],
            ..Default::default()
        },
    )))
}
fn emu(points: f64) -> i64 {
    (points * 12700.).round() as i64
}
fn centipt(points: f64) -> i64 {
    (points * 100.).round() as i64
}

fn element_node(
    element: &Element,
    next_id: &mut usize,
    rels: &mut Relationships,
    doc: &mut PresentationDocument,
    slide_part: &SlidePart,
) -> Result<p::ShapeTreeChoice> {
    let nonlinear = match element {
        Element::Shape(s) => {
            s.fill
                .as_ref()
                .is_some_and(crate::graphics::gradients::nonlinear)
                || s.stroke
                    .as_ref()
                    .is_some_and(|s| crate::graphics::gradients::nonlinear(&s.paint))
        }
        Element::Table(t) => t.cells.iter().any(|c| {
            c.fill
                .as_ref()
                .is_some_and(crate::graphics::gradients::nonlinear)
        }),
        _ => false,
    };
    if nonlinear {
        return element_node(
            &crate::graphics::gradients::expand(element.clone()),
            next_id,
            rels,
            doc,
            slide_part,
        );
    }
    let id = *next_id;
    *next_id += 1;
    match element {
        Element::Group(g) => {
            let mut children = Vec::new();
            for child in &g.elements {
                let child = element_node(child, next_id, rels, doc, slide_part)?;
                children.push(match child {
                    p::ShapeTreeChoice::Shape(v) => p::GroupShapeChoice::Shape(v),
                    p::ShapeTreeChoice::GroupShape(v) => p::GroupShapeChoice::GroupShape(v),
                    p::ShapeTreeChoice::GraphicFrame(v) => p::GroupShapeChoice::GraphicFrame(v),
                    p::ShapeTreeChoice::ConnectionShape(v) => {
                        p::GroupShapeChoice::ConnectionShape(v)
                    }
                    p::ShapeTreeChoice::Picture(v) => p::GroupShapeChoice::Picture(v),
                    p::ShapeTreeChoice::ContentPart(v) => p::GroupShapeChoice::ContentPart(v),
                    p::ShapeTreeChoice::AlternateContent(v) => {
                        p::GroupShapeChoice::AlternateContent(v)
                    }
                });
            }
            Ok(p::ShapeTreeChoice::GroupShape(Box::new(p::GroupShape {
                non_visual_group_shape_properties: Box::new(p::NonVisualGroupShapeProperties {
                    non_visual_drawing_properties: Box::new(drawing::properties(
                        id,
                        format!("Group {id}"),
                        None,
                    )),
                    ..Default::default()
                }),
                group_shape_properties: Box::new(p::GroupShapeProperties {
                    group_shape_properties_choice2: if let Some(effect) = &g.effect {
                        Some(p::GroupShapePropertiesChoice2::EffectList(Box::new(
                            drawing::effect(effect),
                        )))
                    } else {
                        (g.opacity < 1.).then(|| {
                            p::GroupShapePropertiesChoice2::EffectDag(Box::new(a::EffectDag {
                                r#type: Some(drawing::enumeration("tree")),
                                effect_dag_choice: vec![a::EffectDagChoice::AlphaModulationFixed(
                                    a::AlphaModulationFixed {
                                        amount: Some(drawing::percentage(g.opacity)),
                                    },
                                )],
                                ..Default::default()
                            }))
                        })
                    },
                    transform_group: Some(Box::new(a::TransformGroup {
                        offset: drawing::transform(g.bounds).offset,
                        extents: drawing::transform(g.bounds).extents,
                        child_offset: Some(a::ChildOffset {
                            x: drawing::coordinate(g.content_bounds.x),
                            y: drawing::coordinate(g.content_bounds.y),
                        }),
                        child_extents: Some(a::ChildExtents {
                            cx: drawing::coordinate(g.content_bounds.width.max(0.01)),
                            cy: drawing::coordinate(g.content_bounds.height.max(0.01)),
                        }),
                        rotation: (g.rotation != 0.)
                            .then_some((g.rotation * 60000.).round() as i32),
                        horizontal_flip: g.flip_x.then(|| true.into()),
                        vertical_flip: g.flip_y.then(|| true.into()),
                        ..Default::default()
                    })),
                    ..Default::default()
                }),
                group_shape_choice: children,
                ..Default::default()
            })))
        }
        Element::Text(block) => textbox(block, id, rels),
        Element::Table(table) => table_element(table, id, rels),
        Element::Shape(shape) => Ok(p::ShapeTreeChoice::Shape(Box::new(drawing::vector(
            shape, id,
        )))),
        Element::Picture {
            bounds,
            extension,
            bytes,
            clip,
            svg,
        } => {
            let rel = image_part(doc, slide_part, extension, bytes)?;
            let svg_rel = svg
                .as_ref()
                .map(|svg| image_part(doc, slide_part, "svg", svg.as_bytes()))
                .transpose()?;
            let mut picture = drawing::picture(id, *bounds, &rel, svg_rel.as_deref(), None);
            if let Some(commands) = clip {
                // Office stretches a picture into the geometry's visible bounds.
                // Crop the source image to that rectangle before applying a mask.
                let points: Vec<_> = commands
                    .iter()
                    .flat_map(|c| match c {
                        PathCommand::Move(p) | PathCommand::Line(p) => vec![*p],
                        PathCommand::Cubic(p) => p.to_vec(),
                        PathCommand::Close => vec![],
                    })
                    .collect();
                let left = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                let top = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                let right = points
                    .iter()
                    .map(|p| p[0])
                    .fold(f64::NEG_INFINITY, f64::max);
                let bottom = points
                    .iter()
                    .map(|p| p[1])
                    .fold(f64::NEG_INFINITY, f64::max);
                ensure!(right > left && bottom > top, "empty picture clipping path");
                let cropped = Rect {
                    x: bounds.x + left,
                    y: bounds.y + top,
                    width: right - left,
                    height: bottom - top,
                };
                let mut commands = commands.clone();
                for cmd in &mut commands {
                    let adjust = |p: &mut [f64; 2]| {
                        p[0] -= left;
                        p[1] -= top;
                    };
                    match cmd {
                        PathCommand::Move(p) | PathCommand::Line(p) => adjust(p),
                        PathCommand::Cubic(p) => p.iter_mut().for_each(adjust),
                        PathCommand::Close => {}
                    }
                }
                picture.blip_fill.as_mut().unwrap().source_rectangle = Some(a::SourceRectangle {
                    left: Some(drawing::percentage(left / bounds.width)),
                    top: Some(drawing::percentage(top / bounds.height)),
                    right: Some(drawing::percentage(1. - right / bounds.width)),
                    bottom: Some(drawing::percentage(1. - bottom / bounds.height)),
                    ..Default::default()
                });
                picture.shape_properties.transform2_d = Some(Box::new(drawing::transform(cropped)));
                let geometry = drawing::vector(
                    &VectorShape {
                        bounds: cropped,
                        commands,
                        fill: Some(Brush::Solid {
                            color: [0, 0, 0, 255],
                        }),
                        stroke: None,
                    },
                    id,
                );
                picture.shape_properties.shape_properties_choice1 =
                    geometry.shape_properties.shape_properties_choice1;
            }
            Ok(p::ShapeTreeChoice::Picture(Box::new(picture)))
        }
        Element::Drawing { bounds, svg, png }
        | Element::MathSvg {
            bounds, svg, png, ..
        } => {
            let png_rel = image_part(doc, slide_part, "png", png)?;
            let svg_rel = image_part(doc, slide_part, "svg", svg.as_bytes())?;
            let equation = if let Element::MathSvg { source_id, .. } = element {
                Some(source_id.as_str())
            } else {
                None
            };
            Ok(p::ShapeTreeChoice::Picture(Box::new(drawing::picture(
                id,
                *bounds,
                &png_rel,
                Some(&svg_rel),
                equation,
            ))))
        }
    }
}

fn namespaces() -> Vec<ooxmlsdk::common::XmlNamespace> {
    use ooxmlsdk::{common::XmlNamespace as N, namespaces::XmlKnownNamespace as K};
    vec![N::known(K::P), N::known(K::A), N::known(K::R)]
}
