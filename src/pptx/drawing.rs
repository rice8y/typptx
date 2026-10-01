//! DrawingML primitives expressed with SDK schema types.
use super::*;
use ooxmlsdk::{
    sdk::SdkEnum,
    units::{Coordinate32Value, CoordinateValue, DrawingmlPercentageValue},
};

pub(super) fn enumeration<T: SdkEnum>(value: &str) -> T {
    T::from_xml_bytes(value.as_bytes()).expect("internal OOXML enum mapping")
}
pub(super) fn coordinate(points: f64) -> CoordinateValue {
    CoordinateValue::Emu(emu(points))
}
pub(super) fn coordinate32(points: f64) -> Coordinate32Value {
    Coordinate32Value::Emu(emu(points) as i32)
}
pub(super) fn percentage(value: f64) -> DrawingmlPercentageValue {
    DrawingmlPercentageValue::Decimal((value * 100000.).round() as i32)
}
pub(super) fn color(c: [u8; 4]) -> a::RgbColorModelHex {
    a::RgbColorModelHex {
        val: format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
        rgb_color_model_hex_choice: vec![a::RgbColorModelHexChoice::Alpha(a::Alpha {
            val: DrawingmlPercentageValue::Decimal(i32::from(c[3]) * 100000 / 255),
        })],
        ..Default::default()
    }
}
pub(super) fn solid(c: [u8; 4]) -> a::SolidFill {
    a::SolidFill {
        solid_fill_choice: Some(a::SolidFillChoice::RgbColorModelHex(Box::new(color(c)))),
        ..Default::default()
    }
}
pub(super) fn effect(effect: &Effect) -> a::EffectList {
    let mut list = a::EffectList::default();
    match effect {
        Effect::Blur { radius } => {
            list.blur = Some(a::Blur {
                radius: Some(coordinate(*radius)),
                grow: Some(true.into()),
            })
        }
        Effect::Shadow {
            radius,
            offset,
            color: c,
        } => {
            list.outer_shadow = Some(Box::new(a::OuterShadow {
                blur_radius: Some(coordinate(*radius)),
                distance: Some(coordinate(offset[0].hypot(offset[1]))),
                direction: Some(
                    (offset[1].atan2(offset[0]).to_degrees().rem_euclid(360.) * 60000.).round()
                        as i32
                        % 21600000,
                ),
                alignment: Some(enumeration("ctr")),
                rotate_with_shape: Some(true.into()),
                outer_shadow_choice: Some(a::OuterShadowChoice::RgbColorModelHex(Box::new(color(
                    *c,
                )))),
                ..Default::default()
            }))
        }
    }
    list
}
pub(super) fn gradient(brush: &Brush) -> a::GradientFill {
    let (stops, kind) = match brush {
        Brush::Linear { stops, angle } => (
            stops,
            a::GradientFillChoice::LinearGradientFill(a::LinearGradientFill {
                angle: Some((angle.rem_euclid(360.) * 60000.).round() as i32),
                scaled: Some(true.into()),
            }),
        ),
        Brush::Radial { stops, center, .. } => (
            stops,
            a::GradientFillChoice::PathGradientFill(Box::new(a::PathGradientFill {
                path: Some(enumeration("circle")),
                fill_to_rectangle: Some(a::FillToRectangle {
                    left: Some(percentage(center[0])),
                    top: Some(percentage(center[1])),
                    right: Some(percentage(1. - center[0])),
                    bottom: Some(percentage(1. - center[1])),
                }),
            })),
        ),
        Brush::Solid { .. } | Brush::Conic { .. } => {
            unreachable!("paint must be resolved before gradient export")
        }
    };
    a::GradientFill {
        rotate_with_shape: Some(true.into()),
        gradient_fill_choice: Some(kind),
        gradient_stop_list: Some(a::GradientStopList {
            gradient_stop: stops
                .iter()
                .map(|(t, c)| a::GradientStop {
                    position: percentage(*t),
                    gradient_stop_choice: Some(a::GradientStopChoice::RgbColorModelHex(Box::new(
                        color(*c),
                    ))),
                })
                .collect(),
        }),
        tile_rectangle: Some(Default::default()),
        ..Default::default()
    }
}
pub(super) fn paint(brush: Option<&Brush>) -> p::ShapePropertiesChoice2 {
    match brush {
        None => p::ShapePropertiesChoice2::NoFill(Default::default()),
        Some(Brush::Solid { color }) => {
            p::ShapePropertiesChoice2::SolidFill(Box::new(solid(*color)))
        }
        Some(b) => p::ShapePropertiesChoice2::GradientFill(Box::new(gradient(b))),
    }
}
pub(super) fn outline(stroke: Option<&Stroke>) -> a::Outline {
    let Some(s) = stroke else {
        return a::Outline {
            outline_choice1: Some(a::OutlineChoice::NoFill(Default::default())),
            ..Default::default()
        };
    };
    a::Outline {
        width: Some(emu(s.width) as i32),
        cap_type: Some(enumeration(&s.cap)),
        outline_choice1: Some(match &s.paint {
            Brush::Solid { color } => a::OutlineChoice::SolidFill(Box::new(solid(*color))),
            b => a::OutlineChoice::GradientFill(Box::new(gradient(b))),
        }),
        outline_choice2: Some(if s.dash.is_empty() {
            a::OutlineChoice2::PresetDash(a::PresetDash {
                val: Some(enumeration("solid")),
            })
        } else {
            a::OutlineChoice2::CustomDash(a::CustomDash {
                dash_stop: s
                    .dash
                    .chunks(2)
                    .map(|pair| a::DashStop {
                        dash_length: percentage(pair[0] / s.width.max(0.01)),
                        space_length: percentage(
                            pair.get(1).unwrap_or(&pair[0]) / s.width.max(0.01),
                        ),
                    })
                    .collect(),
            })
        }),
        outline_choice3: Some(match s.join.as_str() {
            "round" => a::OutlineChoice3::Round,
            "bevel" => a::OutlineChoice3::LineJoinBevel,
            _ => a::OutlineChoice3::Miter(a::Miter {
                limit: Some(percentage(s.miter_limit)),
            }),
        }),
        ..Default::default()
    }
}
pub(super) fn transform(b: Rect) -> a::Transform2D {
    a::Transform2D {
        offset: Some(a::Offset {
            x: coordinate(b.x),
            y: coordinate(b.y),
        }),
        extents: Some(a::Extents {
            cx: coordinate(b.width.max(0.01)),
            cy: coordinate(b.height.max(0.01)),
        }),
        ..Default::default()
    }
}
pub(super) fn properties(
    id: usize,
    name: String,
    description: Option<String>,
) -> p::NonVisualDrawingProperties {
    p::NonVisualDrawingProperties {
        id: id as u32,
        name,
        description,
        ..Default::default()
    }
}
pub(super) fn shape_properties(
    id: usize,
    name: String,
    description: Option<String>,
    text: bool,
) -> p::NonVisualShapeProperties {
    p::NonVisualShapeProperties {
        non_visual_drawing_properties: Box::new(properties(id, name, description)),
        non_visual_shape_drawing_properties: Box::new(p::NonVisualShapeDrawingProperties {
            text_box: text.then(|| true.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}
pub(super) fn rectangle_geometry() -> p::ShapePropertiesChoice {
    p::ShapePropertiesChoice::PresetGeometry(Box::new(a::PresetGeometry {
        preset: enumeration("rect"),
        adjust_value_list: Some(Default::default()),
        ..Default::default()
    }))
}
pub(super) fn vector(shape: &VectorShape, id: usize) -> p::Shape {
    let point = |p: [f64; 2]| a::Point {
        x: emu(p[0]).to_string(),
        y: emu(p[1]).to_string(),
    };
    let path = a::Path {
        width: Some(coordinate(shape.bounds.width)),
        height: Some(coordinate(shape.bounds.height)),
        fill: Some(enumeration(if shape.fill.is_some() {
            "norm"
        } else {
            "none"
        })),
        stroke: Some(shape.stroke.is_some().into()),
        extrusion_ok: Some(false.into()),
        path_choice: shape
            .commands
            .iter()
            .map(|c| match c {
                PathCommand::Move(p) => {
                    a::PathChoice::MoveTo(Box::new(a::MoveTo { point: point(*p) }))
                }
                PathCommand::Line(p) => {
                    a::PathChoice::LineTo(Box::new(a::LineTo { point: point(*p) }))
                }
                PathCommand::Cubic(p) => a::PathChoice::CubicBezierCurveTo(a::CubicBezierCurveTo {
                    point: p.iter().copied().map(point).collect(),
                }),
                PathCommand::Close => a::PathChoice::CloseShapePath,
            })
            .collect(),
    };
    p::Shape {
        non_visual_shape_properties: Box::new(shape_properties(
            id,
            format!("Vector {id}"),
            None,
            false,
        )),
        shape_properties: Box::new(p::ShapeProperties {
            transform2_d: Some(Box::new(transform(shape.bounds))),
            shape_properties_choice1: Some(p::ShapePropertiesChoice::CustomGeometry(Box::new(
                a::CustomGeometry {
                    adjust_value_list: Some(Default::default()),
                    shape_guide_list: Some(Default::default()),
                    adjust_handle_list: Some(Default::default()),
                    connection_site_list: Some(Default::default()),
                    rectangle: Some(a::Rectangle {
                        left: "0".into(),
                        top: "0".into(),
                        right: "r".into(),
                        bottom: "b".into(),
                    }),
                    path_list: a::PathList { path: vec![path] },
                },
            ))),
            shape_properties_choice2: Some(paint(shape.fill.as_ref())),
            outline: Some(Box::new(outline(shape.stroke.as_ref()))),
            ..Default::default()
        }),
        ..Default::default()
    }
}
pub(super) fn picture(
    id: usize,
    bounds: Rect,
    png: &str,
    svg: Option<&str>,
    equation: Option<&str>,
) -> p::Picture {
    let mut blip = a::Blip {
        embed: Some(png.into()),
        ..Default::default()
    };
    if let Some(svg) = svg {
        blip.blip_extension_list = Some(a::BlipExtensionList {
            blip_extension: vec![a::BlipExtension {
                uri: "{96DAC541-7B7A-43D3-8B79-37D633B846F1}".into(),
                blip_extension_choice: Some(a::BlipExtensionChoice::SvgBlip(
                    ooxmlsdk::schemas::asvg::SvgBlip {
                        xmlns: vec![ooxmlsdk::common::XmlNamespace::known(
                            ooxmlsdk::namespaces::XmlKnownNamespace::Asvg,
                        )],
                        embed: Some(svg.into()),
                        ..Default::default()
                    },
                )),
            }],
            ..Default::default()
        });
    }
    p::Picture {
        non_visual_picture_properties: Box::new(p::NonVisualPictureProperties {
            non_visual_drawing_properties: Box::new(properties(
                id,
                format!(
                    "{} {id}",
                    if equation.is_some() {
                        "Equation"
                    } else if svg.is_some() {
                        "Drawing"
                    } else {
                        "Image"
                    }
                ),
                equation.map(str::to_owned),
            )),
            non_visual_picture_drawing_properties: Box::new(p::NonVisualPictureDrawingProperties {
                picture_locks: svg.map(|_| {
                    Box::new(a::PictureLocks {
                        no_change_aspect: Some(true.into()),
                        ..Default::default()
                    })
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        blip_fill: Some(Box::new(p::BlipFill {
            blip: Some(Box::new(blip)),
            blip_fill_choice: Some(p::BlipFillChoice::Stretch(Box::new(a::Stretch {
                fill_rectangle: Some(Default::default()),
                ..Default::default()
            }))),
            ..Default::default()
        })),
        shape_properties: Box::new(p::ShapeProperties {
            transform2_d: Some(Box::new(transform(bounds))),
            shape_properties_choice1: Some(rectangle_geometry()),
            ..Default::default()
        }),
        ..Default::default()
    }
}
