//! Minimal masters and theme for presentations whose styles are explicit.
use super::drawing::*;
use super::*;
pub(super) fn shape_tree() -> p::ShapeTree {
    p::ShapeTree {
        non_visual_group_shape_properties: Box::new(p::NonVisualGroupShapeProperties {
            non_visual_drawing_properties: Box::new(properties(1, String::new(), None)),
            ..Default::default()
        }),
        group_shape_properties: Box::new(p::GroupShapeProperties {
            transform_group: Some(Box::new(a::TransformGroup {
                offset: Some(Default::default()),
                extents: Some(Default::default()),
                child_offset: Some(Default::default()),
                child_extents: Some(Default::default()),
                ..Default::default()
            })),
            ..Default::default()
        }),
        ..Default::default()
    }
}
pub(super) fn slide_data() -> Box<p::CommonSlideData> {
    Box::new(p::CommonSlideData {
        shape_tree: Box::new(shape_tree()),
        ..Default::default()
    })
}
pub(super) fn color_override() -> Option<Box<p::ColorMapOverride>> {
    Some(Box::new(p::ColorMapOverride {
        color_map_override_choice: Some(p::ColorMapOverrideChoice::MasterColorMapping),
    }))
}
pub(super) fn color_map() -> Box<p::ColorMap> {
    Box::new(p::ColorMap {
        background1: enumeration("lt1"),
        text1: enumeration("dk1"),
        background2: enumeration("lt2"),
        text2: enumeration("dk2"),
        accent1: enumeration("accent1"),
        accent2: enumeration("accent2"),
        accent3: enumeration("accent3"),
        accent4: enumeration("accent4"),
        accent5: enumeration("accent5"),
        accent6: enumeration("accent6"),
        hyperlink: enumeration("hlink"),
        followed_hyperlink: enumeration("folHlink"),
        ..Default::default()
    })
}
pub(super) fn notes_data(notes: &str) -> Box<p::CommonSlideData> {
    let mut data = slide_data();
    let mut nv = shape_properties(2, "Speaker notes".into(), None, false);
    nv.application_non_visual_drawing_properties
        .placeholder_shape = Some(Box::new(p::PlaceholderShape {
        r#type: Some(enumeration("body")),
        index: Some(1),
        ..Default::default()
    }));
    let shape = p::Shape {
        non_visual_shape_properties: Box::new(nv),
        shape_properties: Box::new(p::ShapeProperties {
            transform2_d: Some(Box::new(transform(Rect {
                x: 54.,
                y: 324.,
                width: 432.,
                height: 324.,
            }))),
            ..Default::default()
        }),
        text_body: Some(Box::new(p::TextBody {
            body_properties: Default::default(),
            list_style: Some(Default::default()),
            paragraph: notes
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .split('\n')
                .map(|line| a::Paragraph {
                    paragraph_choice: vec![a::ParagraphChoice::Run(Box::new(a::Run {
                        run_properties: Some(Box::new(a::RunProperties {
                            language: Some("en-US".into()),
                            font_size: Some(1200),
                            ..Default::default()
                        })),
                        text: line.into(),
                    }))],
                    ..Default::default()
                })
                .collect(),
        })),
        ..Default::default()
    };
    data.shape_tree
        .shape_tree_choice
        .push(p::ShapeTreeChoice::Shape(Box::new(shape)));
    data
}
pub(super) fn theme() -> a::Theme {
    let rgb = |s: &str| {
        Box::new(a::RgbColorModelHex {
            val: s.into(),
            ..Default::default()
        })
    };
    let scheme = Box::new(a::ColorScheme {
        name: "Typst".into(),
        dark1_color: Box::new(a::Dark1Color {
            dark1_color_choice: Some(a::Dark1ColorChoice::RgbColorModelHex(rgb("000000"))),
        }),
        light1_color: Box::new(a::Light1Color {
            light1_color_choice: Some(a::Light1ColorChoice::RgbColorModelHex(rgb("FFFFFF"))),
        }),
        dark2_color: Box::new(a::Dark2Color {
            dark2_color_choice: Some(a::Dark2ColorChoice::RgbColorModelHex(rgb("222222"))),
        }),
        light2_color: Box::new(a::Light2Color {
            light2_color_choice: Some(a::Light2ColorChoice::RgbColorModelHex(rgb("EEEEEE"))),
        }),
        accent1_color: Box::new(a::Accent1Color {
            accent1_color_choice: Some(a::Accent1ColorChoice::RgbColorModelHex(rgb("4472C4"))),
        }),
        accent2_color: Box::new(a::Accent2Color {
            accent2_color_choice: Some(a::Accent2ColorChoice::RgbColorModelHex(rgb("ED7D31"))),
        }),
        accent3_color: Box::new(a::Accent3Color {
            accent3_color_choice: Some(a::Accent3ColorChoice::RgbColorModelHex(rgb("A5A5A5"))),
        }),
        accent4_color: Box::new(a::Accent4Color {
            accent4_color_choice: Some(a::Accent4ColorChoice::RgbColorModelHex(rgb("FFC000"))),
        }),
        accent5_color: Box::new(a::Accent5Color {
            accent5_color_choice: Some(a::Accent5ColorChoice::RgbColorModelHex(rgb("5B9BD5"))),
        }),
        accent6_color: Box::new(a::Accent6Color {
            accent6_color_choice: Some(a::Accent6ColorChoice::RgbColorModelHex(rgb("70AD47"))),
        }),
        hyperlink: Box::new(a::Hyperlink {
            hyperlink_choice: Some(a::HyperlinkChoice::RgbColorModelHex(rgb("0563C1"))),
        }),
        followed_hyperlink_color: Box::new(a::FollowedHyperlinkColor {
            followed_hyperlink_color_choice: Some(
                a::FollowedHyperlinkColorChoice::RgbColorModelHex(rgb("954F72")),
            ),
        }),
        ..Default::default()
    });
    let fill = || {
        Box::new(a::SolidFill {
            solid_fill_choice: Some(a::SolidFillChoice::SchemeColor(a::SchemeColor {
                val: enumeration("phClr"),
                ..Default::default()
            })),
            ..Default::default()
        })
    };
    let latin = || {
        Box::new(a::LatinFont {
            typeface: Some("Arial".into()),
            ..Default::default()
        })
    };
    let ea = || {
        Box::new(a::EastAsianFont {
            typeface: Some(String::new()),
            ..Default::default()
        })
    };
    let cs = || {
        Box::new(a::ComplexScriptFont {
            typeface: Some(String::new()),
            ..Default::default()
        })
    };
    a::Theme {
        xmlns: namespaces(),
        name: Some("Typst".into()),
        theme_elements: Box::new(a::ThemeElements {
            color_scheme: scheme,
            font_scheme: Box::new(a::FontScheme {
                name: "Typst".into(),
                major_font: Box::new(a::MajorFont {
                    latin_font: latin(),
                    east_asian_font: ea(),
                    complex_script_font: cs(),
                    ..Default::default()
                }),
                minor_font: Box::new(a::MinorFont {
                    latin_font: latin(),
                    east_asian_font: ea(),
                    complex_script_font: cs(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            format_scheme: Box::new(a::FormatScheme {
                name: Some("Typst".into()),
                fill_style_list: a::FillStyleList {
                    fill_style_list_choice: (0..3)
                        .map(|_| a::FillStyleListChoice::SolidFill(fill()))
                        .collect(),
                },
                line_style_list: a::LineStyleList {
                    outline: (0..3)
                        .map(|_| a::Outline {
                            width: Some(12700),
                            outline_choice1: Some(a::OutlineChoice::SolidFill(fill())),
                            outline_choice2: Some(a::OutlineChoice2::PresetDash(a::PresetDash {
                                val: Some(enumeration("solid")),
                            })),
                            ..Default::default()
                        })
                        .collect(),
                },
                effect_style_list: a::EffectStyleList {
                    effect_style: (0..3)
                        .map(|_| a::EffectStyle {
                            effect_style_choice: Some(a::EffectStyleChoice::EffectList(
                                Default::default(),
                            )),
                            ..Default::default()
                        })
                        .collect(),
                },
                background_fill_style_list: a::BackgroundFillStyleList {
                    background_fill_style_list_choice: (0..3)
                        .map(|_| a::BackgroundFillStyleListChoice::SolidFill(fill()))
                        .collect(),
                },
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}
