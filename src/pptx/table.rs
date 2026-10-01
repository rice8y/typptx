//! Native table cells, merges, text and borders.
use super::drawing::*;
use super::*;
macro_rules! border {
    ($s:expr,$ty:ident,$f1:ident,$e1:ident,$f2:ident,$e2:ident,$f3:ident,$e3:ident) => {{
        let o = outline($s);
        Box::new(a::$ty {
            width: o.width,
            cap_type: o.cap_type,
            compound_line_type: o.compound_line_type,
            alignment: o.alignment,
            head_end: o.head_end,
            tail_end: o.tail_end,
            line_properties_extension_list: o.line_properties_extension_list,
            $f1: o.outline_choice1.map(|v| match v {
                a::OutlineChoice::NoFill(v) => a::$e1::NoFill(v),
                a::OutlineChoice::SolidFill(v) => a::$e1::SolidFill(v),
                a::OutlineChoice::GradientFill(v) => a::$e1::GradientFill(v),
                a::OutlineChoice::PatternFill(v) => a::$e1::PatternFill(v),
            }),
            $f2: o.outline_choice2.map(|v| match v {
                a::OutlineChoice2::PresetDash(v) => a::$e2::PresetDash(v),
                a::OutlineChoice2::CustomDash(v) => a::$e2::CustomDash(v),
            }),
            $f3: o.outline_choice3.map(|v| match v {
                a::OutlineChoice3::Round => a::$e3::Round,
                a::OutlineChoice3::LineJoinBevel => a::$e3::LineJoinBevel,
                a::OutlineChoice3::Miter(v) => a::$e3::Miter(v),
            }),
        })
    }};
}
fn empty_paragraph() -> a::Paragraph {
    a::Paragraph {
        paragraph_properties: Some(Box::new(a::ParagraphProperties {
            line_spacing: Some(Box::new(a::LineSpacing {
                line_spacing_choice: Some(a::LineSpacingChoice::SpacingPoints(a::SpacingPoints {
                    val: 0,
                })),
            })),
            ..Default::default()
        })),
        end_paragraph_run_properties: Some(Box::new(a::EndParagraphRunProperties {
            font_size: Some(100),
            ..Default::default()
        })),
        ..Default::default()
    }
}
pub(super) fn table(table: &Table, id: usize, rels: &mut Relationships) -> p::GraphicFrame {
    let mut rows = Vec::new();
    for (row, height) in table.row_heights.iter().enumerate() {
        let mut cells = Vec::new();
        for column in 0..table.column_widths.len() {
            let cell = table
                .cells
                .iter()
                .find(|c| {
                    c.row <= row
                        && row < c.row + c.row_span
                        && c.column <= column
                        && column < c.column + c.column_span
                })
                .expect("validated table grid");
            let origin = cell.row == row && cell.column == column;
            let right = if origin {
                column + cell.column_span
            } else {
                column + 1
            };
            let bottom = if origin { row + cell.row_span } else { row + 1 };
            let paragraphs = if !origin || cell.paragraphs.is_empty() {
                vec![empty_paragraph()]
            } else {
                cell.paragraphs
                    .iter()
                    .flat_map(|p| {
                        // Match the actual EMU-encoded tracks and margins.
                        let width = (table.column_widths
                            [cell.column..cell.column + cell.column_span]
                            .iter()
                            .map(|w| emu(*w))
                            .sum::<i64>()
                            - emu(cell.text_inset[1])
                            - emu(cell.text_inset[3])) as f64
                            / 12700.;
                        let p = text::table_paragraph(p, width);
                        text::paragraphs(
                            &p,
                            rels,
                            table.bounds.x
                                + table.column_widths[..cell.column].iter().sum::<f64>()
                                + cell.text_inset[3],
                            table.bounds.x
                                + table.column_widths[..cell.column + cell.column_span]
                                    .iter()
                                    .sum::<f64>()
                                - cell.text_inset[1],
                        )
                    })
                    .collect()
            };
            cells.push(a::TableCell {
                row_span: (cell.row == row && cell.row_span > 1).then_some(cell.row_span as i32),
                grid_span: (cell.column == column && cell.column_span > 1)
                    .then_some(cell.column_span as i32),
                vertical_merge: (cell.row < row).then(|| true.into()),
                horizontal_merge: (cell.column < column).then(|| true.into()),
                text_body: Some(Box::new(a::TextBody {
                    body_properties: Box::new(a::BodyProperties {
                        wrap: Some(enumeration(if cell.wrap { "square" } else { "none" })),
                        ..Default::default()
                    }),
                    list_style: Some(Default::default()),
                    paragraph: paragraphs,
                })),
                table_cell_properties: Some(Box::new(a::TableCellProperties {
                    left_margin: Some(coordinate32(cell.text_inset[3])),
                    right_margin: Some(coordinate32(cell.text_inset[1])),
                    top_margin: Some(coordinate32(cell.text_inset[0])),
                    bottom_margin: Some(coordinate32(cell.text_inset[2])),
                    anchor: Some(enumeration(&cell.vertical_alignment)),
                    left_border_line_properties: Some(border!(
                        table.vertical_borders[row][column].as_ref(),
                        LeftBorderLineProperties,
                        left_border_line_properties_choice1,
                        LeftBorderLinePropertiesChoice,
                        left_border_line_properties_choice2,
                        LeftBorderLinePropertiesChoice2,
                        left_border_line_properties_choice3,
                        LeftBorderLinePropertiesChoice3
                    )),
                    right_border_line_properties: Some(border!(
                        table.vertical_borders[row][right].as_ref(),
                        RightBorderLineProperties,
                        right_border_line_properties_choice1,
                        RightBorderLinePropertiesChoice,
                        right_border_line_properties_choice2,
                        RightBorderLinePropertiesChoice2,
                        right_border_line_properties_choice3,
                        RightBorderLinePropertiesChoice3
                    )),
                    top_border_line_properties: Some(border!(
                        table.horizontal_borders[row][column].as_ref(),
                        TopBorderLineProperties,
                        top_border_line_properties_choice1,
                        TopBorderLinePropertiesChoice,
                        top_border_line_properties_choice2,
                        TopBorderLinePropertiesChoice2,
                        top_border_line_properties_choice3,
                        TopBorderLinePropertiesChoice3
                    )),
                    bottom_border_line_properties: Some(border!(
                        table.horizontal_borders[bottom][column].as_ref(),
                        BottomBorderLineProperties,
                        bottom_border_line_properties_choice1,
                        BottomBorderLinePropertiesChoice,
                        bottom_border_line_properties_choice2,
                        BottomBorderLinePropertiesChoice2,
                        bottom_border_line_properties_choice3,
                        BottomBorderLinePropertiesChoice3
                    )),
                    table_cell_properties_choice: Some(cell.fill.as_ref().map_or_else(
                        || a::TableCellPropertiesChoice::NoFill(Default::default()),
                        |c| match c {
                            Brush::Solid { color } => {
                                a::TableCellPropertiesChoice::SolidFill(Box::new(solid(*color)))
                            }
                            _ => a::TableCellPropertiesChoice::GradientFill(Box::new(gradient(c))),
                        },
                    )),
                    ..Default::default()
                })),
                ..Default::default()
            });
        }
        rows.push(a::TableRow {
            height: coordinate(*height),
            table_cell: cells,
            ..Default::default()
        });
    }
    let t = transform(table.bounds);
    p::GraphicFrame {
        non_visual_graphic_frame_properties: Box::new(p::NonVisualGraphicFrameProperties {
            non_visual_drawing_properties: Box::new(properties(
                id,
                format!("Table {id}"),
                Some(table.source_id.clone()),
            )),
            ..Default::default()
        }),
        transform: Box::new(p::Transform {
            offset: t.offset,
            extents: t.extents,
            ..Default::default()
        }),
        graphic: Box::new(a::Graphic {
            graphic_data: a::GraphicData {
                uri: "http://schemas.openxmlformats.org/drawingml/2006/table".into(),
                graphic_data_choice: vec![a::GraphicDataChoice::Table(Box::new(a::Table {
                    table_properties: Some(Default::default()),
                    table_grid: a::TableGrid {
                        grid_column: table
                            .column_widths
                            .iter()
                            .map(|w| a::GridColumn {
                                width: coordinate(*w),
                                ..Default::default()
                            })
                            .collect(),
                    },
                    table_row: rows,
                }))],
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}
