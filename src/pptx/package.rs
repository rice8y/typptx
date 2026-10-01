//! Assemble package parts, slide masters, fonts, notes, and document properties.
use super::{Media, Relationships, defaults, drawing, element_node, emu, namespaces};
use crate::ir::*;
use anyhow::{Context, Result, ensure};
use ooxmlsdk::{
    parts::{
        font_part::FontPart, notes_master_part::NotesMasterPart, notes_slide_part::NotesSlidePart,
        presentation_document::PresentationDocument, presentation_part::PresentationPart,
        slide_layout_part::SlideLayoutPart, slide_master_part::SlideMasterPart,
        slide_part::SlidePart, theme_part::ThemePart,
    },
    schemas::{a, p},
    sdk::PresentationDocumentType,
};

const GENERATOR: &str = concat!("Typptx ", env!("CARGO_PKG_VERSION"));

pub fn write(presentation: &Presentation) -> Result<Vec<u8>> {
    ensure!(
        !presentation
            .diagnostics
            .iter()
            .any(|d| d.code.starts_with("unsupported_")),
        "native export has unsupported objects; see the conversion report"
    );
    let first = presentation
        .slides
        .first()
        .context("document has no pages")?;
    ensure!(presentation.slides.iter().all(|s| (s.width-first.width).abs()<0.01 && (s.height-first.height).abs()<0.01),
        "PowerPoint requires a single slide size; this document has different page sizes");
    ensure!(
        first.width > 0.0
            && first.height > 0.0
            && first.width.is_finite()
            && first.height.is_finite(),
        "invalid slide dimensions"
    );
    let mut doc = PresentationDocument::create(PresentationDocumentType::Presentation);
    let part = doc.add_new_part_auto_id::<PresentationPart>()?;
    let master = part.add_new_part_auto_id::<_, SlideMasterPart>(&mut doc)?;
    let layout = master.add_new_part_auto_id::<_, SlideLayoutPart>(&mut doc)?;
    layout.create_relationship_to_part(&mut doc, master.clone())?;
    let theme_part = master.add_new_part_auto_id::<_, ThemePart>(&mut doc)?;
    theme_part.set_root_element(&mut doc, defaults::theme())?;
    let layout_rel = master.get_id_of_part(&doc, &layout)?.to_owned();
    master.set_root_element(
        &mut doc,
        p::SlideMaster {
            xmlns: namespaces(),
            common_slide_data: defaults::slide_data(),
            color_map: defaults::color_map(),
            slide_layout_id_list: Some(p::SlideLayoutIdList {
                slide_layout_id: vec![p::SlideLayoutId {
                    id: Some(2147483649),
                    relationship_id: layout_rel,
                    ..Default::default()
                }],
            }),
            text_styles: Some(Box::new(p::TextStyles {
                title_style: Some(Default::default()),
                body_style: Some(Default::default()),
                other_style: Some(Default::default()),
                ..Default::default()
            })),
            ..Default::default()
        },
    )?;
    layout.set_root_element(
        &mut doc,
        p::SlideLayout {
            xmlns: namespaces(),
            r#type: Some(drawing::enumeration("blank")),
            preserve: Some(true.into()),
            common_slide_data: {
                let mut data = defaults::slide_data();
                data.name = Some("Blank".into());
                data
            },
            color_map_override: defaults::color_override(),
            ..Default::default()
        },
    )?;
    let mut root = p::Presentation {
        xmlns: namespaces(),
        embed_true_type_fonts: Some(true.into()),
        save_subset_fonts: Some(false.into()),
        slide_master_id_list: Some(p::SlideMasterIdList {
            slide_master_id: vec![p::SlideMasterId {
                id: Some(2147483648),
                relationship_id: part.get_id_of_part(&doc, &master)?.into(),
                ..Default::default()
            }],
        }),
        slide_size: Some(p::SlideSize {
            cx: emu(first.width).try_into()?,
            cy: emu(first.height).try_into()?,
            ..Default::default()
        }),
        notes_size: p::NotesSize {
            cx: 6858000,
            cy: 9144000,
        },
        ..Default::default()
    };
    let notes_master = if presentation.slides.iter().any(|s| s.notes.is_some()) {
        let notes = part.add_new_part_auto_id::<_, NotesMasterPart>(&mut doc)?;
        let theme_part = notes.add_new_part_auto_id::<_, ThemePart>(&mut doc)?;
        let mut notes_theme = defaults::theme();
        notes_theme.name = Some("Typptx Notes".into());
        theme_part.set_root_element(&mut doc, notes_theme)?;
        notes.set_root_element(
            &mut doc,
            p::NotesMaster {
                xmlns: namespaces(),
                common_slide_data: defaults::notes_data(""),
                color_map: defaults::color_map(),
                notes_style: Some(Box::new(p::NotesStyle {
                    level1_paragraph_properties: Some(Box::new(a::Level1ParagraphProperties {
                        default_run_properties: Some(Box::new(a::DefaultRunProperties {
                            font_size: Some(1200),
                            ..Default::default()
                        })),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                ..Default::default()
            },
        )?;
        root.notes_master_id_list = Some(Box::new(p::NotesMasterIdList {
            notes_master_id: Some(Box::new(p::NotesMasterId {
                id: part.get_id_of_part(&doc, &notes)?.into(),
                ..Default::default()
            })),
        }));
        Some(notes)
    } else {
        None
    };
    let mut ids = Vec::new();
    let mut media = Media::default();
    let slide_parts = (0..presentation.slides.len())
        .map(|_| part.add_new_part_auto_id::<_, SlidePart>(&mut doc))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (index, slide) in presentation.slides.iter().enumerate() {
        let slide_part = slide_parts[index].clone();
        slide_part.create_relationship_to_part(&mut doc, layout.clone())?;
        ids.push(p::SlideId {
            id: 256 + index as u32,
            relationship_id: part.get_id_of_part(&doc, &slide_part)?.into(),
            ..Default::default()
        });
        if let Some(text) = &slide.notes {
            let notes = slide_part.add_new_part_auto_id::<_, NotesSlidePart>(&mut doc)?;
            notes.create_relationship_to_part(&mut doc, notes_master.as_ref().unwrap().clone())?;
            notes.create_relationship_to_part(&mut doc, slide_part.clone())?;
            notes.set_root_element(
                &mut doc,
                p::NotesSlide {
                    xmlns: namespaces(),
                    common_slide_data: defaults::notes_data(text),
                    color_map_override: defaults::color_override(),
                    ..Default::default()
                },
            )?;
        }
        let mut rels = Relationships::default();
        rels.prepare_bullets(slide, &slide_part, &mut doc, &mut media)?;
        let mut shapes = defaults::shape_tree();
        let mut next_id = 2;
        for element in &slide.elements {
            shapes.shape_tree_choice.push(element_node(
                element,
                &mut next_id,
                &mut rels,
                &mut doc,
                &slide_part,
                &mut media,
            )?);
        }
        for link in &slide.links {
            let mut shape = drawing::vector(&link.region, next_id);
            next_id += 1;
            let properties = &mut shape
                .non_visual_shape_properties
                .non_visual_drawing_properties;
            properties.name = "Empty link area".into();
            properties.hyperlink_on_click = Some(Box::new(rels.hyperlink(&link.target)));
            shapes
                .shape_tree_choice
                .push(p::ShapeTreeChoice::Shape(Box::new(shape)));
        }
        for (index, target) in rels.links.iter().enumerate() {
            let id = format!("link{}", index + 1);
            match target {
                LinkTarget::Url(url) => {
                    slide_part.add_hyperlink_relationship(&mut doc, id, url.clone())?;
                }
                LinkTarget::Slide(number) => {
                    let target = number
                        .checked_sub(1)
                        .and_then(|i| slide_parts.get(i))
                        .context("internal link targets a nonexistent slide")?;
                    slide_part.create_relationship_to_part_with_id(&mut doc, target.clone(), id)?;
                }
            }
        }
        let background = slide.background.map(|c| {
            Box::new(p::Background {
                background_choice: Some(p::BackgroundChoice::BackgroundProperties(Box::new(
                    p::BackgroundProperties {
                        background_properties_choice1: Some(
                            p::BackgroundPropertiesChoice::SolidFill(Box::new(drawing::solid(c))),
                        ),
                        background_properties_choice2: Some(
                            p::BackgroundPropertiesChoice2::EffectList(Default::default()),
                        ),
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            })
        });
        slide_part.set_root_element(
            &mut doc,
            p::Slide {
                xmlns: namespaces(),
                common_slide_data: Box::new(p::CommonSlideData {
                    background,
                    shape_tree: Box::new(shapes),
                    ..Default::default()
                }),
                color_map_override: defaults::color_override(),
                ..Default::default()
            },
        )?;
    }
    root.slide_id_list = Some(p::SlideIdList { slide_id: ids });
    let mut families = std::collections::BTreeMap::<&str, p::EmbeddedFont>::new();
    for font in &presentation.fonts {
        let f = part.add_new_part_with_content_type_and_extension_auto_id::<_, FontPart>(
            &mut doc,
            "application/x-fontdata",
            ".fntdata",
        )?;
        f.set_data(&mut doc, font.data.clone())?;
        let rel = part.get_id_of_part(&doc, &f)?.to_owned();
        let entry = families
            .entry(&font.family)
            .or_insert_with(|| p::EmbeddedFont {
                font: Box::new(p::Font {
                    typeface: Some(font.family.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        match (font.bold, font.italic) {
            (false, false) => entry.regular_font = Some(p::RegularFont { id: rel }),
            (true, false) => entry.bold_font = Some(p::BoldFont { id: rel }),
            (false, true) => entry.italic_font = Some(p::ItalicFont { id: rel }),
            (true, true) => entry.bold_italic_font = Some(p::BoldItalicFont { id: rel }),
        }
    }
    if !families.is_empty() {
        root.embedded_font_list = Some(p::EmbeddedFontList {
            embedded_font: families.into_values().collect(),
        });
    }
    part.set_root_element(&mut doc, root)?;
    let core = doc.add_core_file_properties_part()?;
    use ooxmlsdk::{
        common::XmlNamespace as N, namespaces::XmlKnownNamespace as K,
        schemas::opc_core_properties as cp,
    };
    core.set_root_element(
        &mut doc,
        cp::CoreProperties {
            xmlns: vec![N::known(K::Cp), N::known(K::Dc)],
            creator: Some(cp::Creator {
                xml_content: Some(GENERATOR.into()),
                ..Default::default()
            }),
            title: Some("Typst presentation".into()),
            ..Default::default()
        },
    )?;
    let app = doc.add_extended_file_properties_part()?;
    app.set_root_element(
        &mut doc,
        ooxmlsdk::schemas::ap::Properties {
            xmlns: vec![N::known(K::Ap)],
            application: Some(GENERATOR.into()),
            slides: Some(presentation.slides.len() as i32),
            ..Default::default()
        },
    )?;
    let errors = doc.validate()?;
    ensure!(
        errors.is_empty(),
        "PPTX schema validation failed: {errors:?}"
    );
    Ok(doc.to_package_bytes()?)
}
