//! Structured Office Math. SDK types enforce element ordering and choices.
use super::drawing::enumeration;
use super::*;
use ooxmlsdk::schemas::m;
type Node = m::OfficeMathChoice;

// OMML gives each argument position its own (identical) choice enum.
macro_rules! argument {
    ($fn:ident,$ty:ident,$field:ident,$choice:ident) => {
        fn $fn(nodes: Vec<Node>) -> Box<m::$ty> {
            Box::new(m::$ty {
                $field: nodes
                    .into_iter()
                    .map(|n| match n {
                        Node::Accent(v) => m::$choice::Accent(v),
                        Node::Bar(v) => m::$choice::Bar(v),
                        Node::Box(v) => m::$choice::Box(v),
                        Node::BorderBox(v) => m::$choice::BorderBox(v),
                        Node::Delimiter(v) => m::$choice::Delimiter(v),
                        Node::EquationArray(v) => m::$choice::EquationArray(v),
                        Node::Fraction(v) => m::$choice::Fraction(v),
                        Node::MathFunction(v) => m::$choice::MathFunction(v),
                        Node::GroupChar(v) => m::$choice::GroupChar(v),
                        Node::LimitLower(v) => m::$choice::LimitLower(v),
                        Node::LimitUpper(v) => m::$choice::LimitUpper(v),
                        Node::Matrix(v) => m::$choice::Matrix(v),
                        Node::Nary(v) => m::$choice::Nary(v),
                        Node::Phantom(v) => m::$choice::Phantom(v),
                        Node::Radical(v) => m::$choice::Radical(v),
                        Node::PreSubSuper(v) => m::$choice::PreSubSuper(v),
                        Node::Subscript(v) => m::$choice::Subscript(v),
                        Node::SubSuperscript(v) => m::$choice::SubSuperscript(v),
                        Node::Superscript(v) => m::$choice::Superscript(v),
                        Node::Run(v) => m::$choice::Run(v),
                        _ => unreachable!("typptx only constructs math nodes"),
                    })
                    .collect(),
                ..Default::default()
            })
        }
    };
}
argument!(base, Base, base_choice, BaseChoice);
argument!(numerator, Numerator, numerator_choice, NumeratorChoice);
argument!(
    denominator,
    Denominator,
    denominator_choice,
    DenominatorChoice
);
argument!(sub, SubArgument, sub_argument_choice, SubArgumentChoice);
argument!(
    sup,
    SuperArgument,
    super_argument_choice,
    SuperArgumentChoice
);
argument!(degree, Degree, degree_choice, DegreeChoice);
argument!(limit, Limit, limit_choice, LimitChoice);

pub(super) fn equation(expr: &MathExpr, run: &Run) -> m::OfficeMath {
    m::OfficeMath {
        xmlns: vec![ooxmlsdk::common::XmlNamespace::known(
            ooxmlsdk::namespaces::XmlKnownNamespace::M,
        )],
        office_math_choice: expression(expr, run, &MathStyle::default(), false),
    }
}
fn math_run(run: &Run) -> Run {
    let mut r = run.clone();
    r.style.font = "Cambria Math".into();
    r.style.pitch_family = 0x12;
    r.style.bold = false;
    r.style.italic = false;
    r
}
fn control(run: &Run) -> Box<m::ControlProperties> {
    Box::new(m::ControlProperties {
        control_properties_choice: Some(m::ControlPropertiesChoice::DrawingRunProperties(
            Box::new(super::text::run_properties(&math_run(run), None)),
        )),
    })
}
fn expression(expr: &MathExpr, run: &Run, style: &MathStyle, in_array: bool) -> Vec<Node> {
    use MathExpr as M;
    let child = |m: &M| expression(m, run, style, false);
    let invisible = || child(&M::Text("\u{200b}".into()));
    vec![match expr {
        M::Phantom { body } => Node::Phantom(Box::new(m::Phantom {
            phantom_properties: Some(Box::new(m::PhantomProperties {
                show_phantom: Some(m::ShowPhantom {
                    val: Some(enumeration("0")),
                }),
                ..Default::default()
            })),
            base: base(child(body)),
        })),
        M::AlignPoint => {
            if !in_array {
                return vec![];
            }
            // Equation arrays use alternating ampersands for alignment and
            // spacing points. Literal ampersands have m:lit instead.
            let mut nodes = child(&M::Text("&".into()));
            if let Node::Run(r) = &mut nodes[0] {
                r.math_run_properties.as_mut().unwrap().literal = None;
            }
            return nodes;
        }
        M::Styled { body, style } => return expression(body, run, style, in_array),
        M::Sized {
            body,
            scale,
            offset,
        } => {
            let mut r = run.clone();
            r.style.size *= scale;
            r.style.baseline += run.style.size * offset;
            return expression(body, &r, style, in_array);
        }
        M::Cancel {
            body,
            rising,
            falling,
            horizontal,
            vertical,
        } => Node::BorderBox(Box::new(m::BorderBox {
            border_box_properties: Some(Box::new(m::BorderBoxProperties {
                hide_top: Some(m::HideTop {
                    val: Some(enumeration("1")),
                }),
                hide_bottom: Some(m::HideBottom {
                    val: Some(enumeration("1")),
                }),
                hide_left: Some(m::HideLeft {
                    val: Some(enumeration("1")),
                }),
                hide_right: Some(m::HideRight {
                    val: Some(enumeration("1")),
                }),
                strike_bottom_left_to_top_right: rising.then(|| m::StrikeBottomLeftToTopRight {
                    val: Some(enumeration("1")),
                }),
                strike_top_left_to_bottom_right: falling.then(|| m::StrikeTopLeftToBottomRight {
                    val: Some(enumeration("1")),
                }),
                strike_horizontal: horizontal.then(|| m::StrikeHorizontal {
                    val: Some(enumeration("1")),
                }),
                strike_vertical: vertical.then(|| m::StrikeVertical {
                    val: Some(enumeration("1")),
                }),
                control_properties: Some(control(run)),
            })),
            base: base(child(body)),
        })),
        M::Text(t) => {
            let mut r = math_run(run);
            let mut properties = if style.normal {
                r.style.bold = style.bold;
                r.style.italic = style.italic.unwrap_or(false);
                Some(Box::new(m::RunProperties {
                    run_properties_choice: Some(m::RunPropertiesChoice::NormalText(
                        Default::default(),
                    )),
                    ..Default::default()
                }))
            } else if style.variant.is_some() || style.bold || style.italic.is_some() {
                Some(Box::new(m::RunProperties {
                    run_properties_choice: Some(m::RunPropertiesChoice::Sequence(Box::new(
                        m::RunPropertiesChoiceSequence {
                            script: style.variant.as_ref().map(|v| m::Script {
                                val: enumeration(v),
                            }),
                            style: (style.bold || style.italic.is_some()).then(|| {
                                let italic = style
                                    .italic
                                    .unwrap_or_else(|| t.chars().any(char::is_alphabetic));
                                m::Style {
                                    val: enumeration(match (style.bold, italic) {
                                        (true, true) => "bi",
                                        (true, false) => "b",
                                        (false, true) => "i",
                                        _ => "p",
                                    }),
                                }
                            }),
                        },
                    ))),
                    ..Default::default()
                }))
            } else {
                None
            };
            if t.contains('&') {
                properties.get_or_insert_with(Default::default).literal = Some(m::Literal {
                    val: Some(enumeration("1")),
                });
            }
            Node::Run(Box::new(m::Run {
                math_run_properties: properties,
                run_choice: vec![
                    m::RunChoice::DrawingRunProperties(Box::new(super::text::run_properties(
                        &r, None,
                    ))),
                    m::RunChoice::MText(m::Text {
                        space: Some(enumeration("preserve")),
                        xml_content: Some(t.clone()),
                    }),
                ],
                ..Default::default()
            }))
        }
        M::Sequence(seq) => {
            return seq
                .iter()
                .flat_map(|m| expression(m, run, style, in_array))
                .collect();
        }
        M::Rows(rows) => Node::EquationArray(Box::new(m::EquationArray {
            equation_array_properties: rows
                .last()
                .is_some_and(|r| matches!(r, M::Sized { .. }))
                .then(|| {
                    Box::new(m::EquationArrayProperties {
                        base_justification: Some(m::BaseJustification {
                            val: enumeration("top"),
                        }),
                        ..Default::default()
                    })
                }),
            base: rows
                .iter()
                .map(|r| *base(expression(r, run, style, true)))
                .collect(),
        })),
        M::Fraction {
            numerator: n,
            denominator: d,
            format,
        } => Node::Fraction(Box::new(m::Fraction {
            fraction_properties: Some(Box::new(m::FractionProperties {
                fraction_type: Some(m::FractionType {
                    val: enumeration(format),
                }),
                ..Default::default()
            })),
            numerator: numerator(child(n)),
            denominator: denominator(child(d)),
        })),
        M::PreScripts {
            base: b,
            sub: s,
            sup: t,
        } => Node::PreSubSuper(Box::new(m::PreSubSuper {
            base: base(child(b)),
            sub_argument: sub(s.as_ref().map(|m| child(m)).unwrap_or_else(invisible)),
            super_argument: sup(t.as_ref().map(|m| child(m)).unwrap_or_else(invisible)),
            ..Default::default()
        })),
        M::Scripts {
            base: b,
            sub: s,
            sup: t,
        } => match (s, t) {
            (Some(s), Some(t)) => Node::SubSuperscript(Box::new(m::SubSuperscript {
                base: base(child(b)),
                sub_argument: sub(child(s)),
                super_argument: sup(child(t)),
                ..Default::default()
            })),
            (Some(s), None) => Node::Subscript(Box::new(m::Subscript {
                base: base(child(b)),
                sub_argument: sub(child(s)),
                ..Default::default()
            })),
            (None, Some(t)) => Node::Superscript(Box::new(m::Superscript {
                base: base(child(b)),
                super_argument: sup(child(t)),
                ..Default::default()
            })),
            _ => return child(b),
        },
        M::Bar { body, top } => Node::Bar(Box::new(m::Bar {
            bar_properties: Some(Box::new(m::BarProperties {
                position: Some(m::Position {
                    val: enumeration(if *top { "top" } else { "bot" }),
                }),
                ..Default::default()
            })),
            base: base(child(body)),
        })),
        M::Group {
            body,
            character,
            top,
        } => Node::GroupChar(Box::new(m::GroupChar {
            group_char_properties: Some(Box::new(m::GroupCharProperties {
                accent_char: Some(m::AccentChar {
                    val: character.clone(),
                }),
                position: Some(m::Position {
                    val: enumeration(if *top { "top" } else { "bot" }),
                }),
                vertical_justification: Some(m::VerticalJustification {
                    val: enumeration(if *top { "bot" } else { "top" }),
                }),
                control_properties: Some(control(run)),
            })),
            base: base(child(body)),
        })),
        M::Root { body, index } => Node::Radical(Box::new(m::Radical {
            radical_properties: Some(Box::new(m::RadicalProperties {
                hide_degree: Some(m::HideDegree {
                    val: Some(enumeration(if index.is_none() { "1" } else { "0" })),
                }),
                ..Default::default()
            })),
            degree: degree(index.as_ref().map(|i| child(i)).unwrap_or_default()),
            base: base(child(body)),
        })),
        M::Delimiter { open, close, body } => Node::Delimiter(Box::new(m::Delimiter {
            delimiter_properties: Some(Box::new(m::DelimiterProperties {
                begin_char: Some(m::BeginChar { val: open.clone() }),
                end_char: Some(m::EndChar { val: close.clone() }),
                ..Default::default()
            })),
            base: vec![*base(child(body))],
        })),
        M::DelimitedParts {
            open,
            close,
            separator,
            parts,
        } => Node::Delimiter(Box::new(m::Delimiter {
            delimiter_properties: Some(Box::new(m::DelimiterProperties {
                begin_char: Some(m::BeginChar { val: open.clone() }),
                end_char: Some(m::EndChar { val: close.clone() }),
                separator_char: Some(m::SeparatorChar {
                    val: separator.clone(),
                }),
                ..Default::default()
            })),
            base: parts.iter().map(|part| *base(child(part))).collect(),
        })),
        M::Matrix {
            rows,
            alignment,
            separators,
        } if !separators.is_empty() => {
            let mut start = 0;
            let mut parts = Vec::new();
            for end in separators
                .iter()
                .copied()
                .chain(std::iter::once(rows[0].len()))
            {
                let mut nodes = child(&M::Matrix {
                    rows: rows.iter().map(|r| r[start..end].to_vec()).collect(),
                    alignment: alignment.clone(),
                    separators: vec![],
                });
                if let Node::Matrix(matrix) = &mut nodes[0] {
                    for (source, row) in rows.iter().zip(&mut matrix.matrix_row) {
                        if source.iter().all(crate::math::uniform_matrix_cell) {
                            continue;
                        }
                        // Every segment receives the same ascent/descent, but
                        // the hidden row contributes no width to any column.
                        row.base[0]
                            .base_choice
                            .push(m::BaseChoice::Phantom(Box::new(m::Phantom {
                                phantom_properties: Some(Box::new(m::PhantomProperties {
                                    show_phantom: Some(m::ShowPhantom {
                                        val: Some(enumeration("0")),
                                    }),
                                    zero_width: Some(m::ZeroWidth {
                                        val: Some(enumeration("1")),
                                    }),
                                    ..Default::default()
                                })),
                                base: base(source.iter().flat_map(child).collect()),
                            })));
                    }
                }
                parts.push(*base(nodes));
                start = end;
            }
            Node::Delimiter(Box::new(m::Delimiter {
                delimiter_properties: Some(Box::new(m::DelimiterProperties {
                    begin_char: Some(m::BeginChar { val: String::new() }),
                    end_char: Some(m::EndChar { val: String::new() }),
                    separator_char: Some(m::SeparatorChar { val: "|".into() }),
                    grow_operators: Some(m::GrowOperators {
                        val: Some(enumeration("1")),
                    }),
                    ..Default::default()
                })),
                base: parts,
            }))
        }
        M::Matrix {
            rows, alignment, ..
        } => Node::Matrix(Box::new(m::Matrix {
            matrix_properties: Some(Box::new(m::MatrixProperties {
                matrix_columns: Some(m::MatrixColumns {
                    matrix_column: vec![m::MatrixColumn {
                        matrix_column_properties: Some(Box::new(m::MatrixColumnProperties {
                            matrix_column_count: Some(m::MatrixColumnCount {
                                val: rows.iter().map(Vec::len).max().unwrap_or(1) as i64,
                            }),
                            matrix_column_justification: Some(m::MatrixColumnJustification {
                                val: enumeration(alignment),
                            }),
                        })),
                    }],
                }),
                ..Default::default()
            })),
            matrix_row: rows
                .iter()
                .map(|r| m::MatrixRow {
                    base: r.iter().map(|e| *base(child(e))).collect(),
                })
                .collect(),
        })),
        M::Limits {
            base: b,
            sub: s,
            sup: t,
        } => {
            let mut out = child(b);
            if let Some(s) = s {
                out = vec![Node::LimitLower(Box::new(m::LimitLower {
                    base: base(out),
                    limit: limit(child(s)),
                    ..Default::default()
                }))];
            }
            if let Some(t) = t {
                out = vec![Node::LimitUpper(Box::new(m::LimitUpper {
                    base: base(out),
                    limit: limit(child(t)),
                    ..Default::default()
                }))];
            }
            return out;
        }
        M::Operator {
            character,
            sub: s,
            sup: t,
            limits,
        } => Node::Nary(Box::new(m::Nary {
            nary_properties: Some(Box::new(m::NaryProperties {
                accent_char: Some(m::AccentChar {
                    val: character.clone(),
                }),
                limit_location: Some(m::LimitLocation {
                    val: enumeration(if *limits { "undOvr" } else { "subSup" }),
                }),
                hide_sub_argument: Some(m::HideSubArgument {
                    val: Some(enumeration(if s.is_none() { "1" } else { "0" })),
                }),
                hide_super_argument: Some(m::HideSuperArgument {
                    val: Some(enumeration(if t.is_none() { "1" } else { "0" })),
                }),
                control_properties: Some(control(run)),
                ..Default::default()
            })),
            sub_argument: sub(s.as_ref().map(|s| child(s)).unwrap_or_default()),
            super_argument: sup(t.as_ref().map(|s| child(s)).unwrap_or_default()),
            base: base(invisible()),
        })),
        M::Accent { character, body } => Node::Accent(Box::new(m::Accent {
            accent_properties: Some(Box::new(m::AccentProperties {
                accent_char: Some(m::AccentChar {
                    val: character.clone(),
                }),
                ..Default::default()
            })),
            base: base(child(body)),
        })),
    }]
}
