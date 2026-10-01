//! Combine evaluated overlays without changing any step's geometry or order.
use crate::ir::*;
use anyhow::{Result, ensure};
use std::ops::Range;

pub(super) fn combine(presentation: &mut Presentation, groups: &[Range<usize>]) -> Result<()> {
    let mut destinations = vec![0; presentation.slides.len()];
    for (slide, pages) in groups.iter().enumerate() {
        for page in pages.clone() {
            destinations[page] = slide + 1;
        }
    }
    for slide in &mut presentation.slides {
        for element in &mut slide.elements {
            remap(element, &destinations);
        }
        for link in &mut slide.links {
            remap_target(&mut link.target, &destinations);
        }
    }
    let mut slides = std::mem::take(&mut presentation.slides).into_iter();
    for pages in groups {
        let mut overlays: Vec<_> = slides.by_ref().take(pages.len()).collect();
        if overlays.len() == 1 {
            presentation.slides.push(overlays.remove(0));
            continue;
        }
        ensure!(
            overlays
                .iter()
                .all(|s| (s.width - overlays[0].width).abs() < 0.01
                    && (s.height - overlays[0].height).abs() < 0.01),
            "PowerPoint requires a single slide size; animation overlays have different page sizes"
        );
        let background = overlays[0].background;
        let changing_background = overlays.iter().any(|s| s.background != background);
        let mut slots: Vec<Slot> = Vec::new();
        let mut notes = Vec::new();
        for (step, slide) in overlays.iter_mut().enumerate() {
            if let Some(note) = &slide.notes {
                // Package notes are repeated on each overlay. Keep each note
                // paragraph once, including additions on a later step.
                for paragraph in note.split("\n\n") {
                    if !notes.iter().any(|n| n == paragraph) {
                        notes.push(paragraph.to_owned());
                    }
                }
            }
            // A changing background and otherwise empty click targets must take
            // part in the same visibility sequence as the drawable objects.
            if changing_background {
                slide.elements.insert(0, background_shape(slide));
            }
            slide
                .elements
                .extend(
                    std::mem::take(&mut slide.links)
                        .into_iter()
                        .map(|link| Element::Linked {
                            target: link.target,
                            element: Box::new(Element::Shape(link.region)),
                        }),
                );
            for slot in &mut slots {
                slot.visible.push(false);
            }
            let mut cursor = 0;
            for element in std::mem::take(&mut slide.elements) {
                let mut key = element.clone();
                clear_source_ids(&mut key);
                if let Some(offset) = slots[cursor..].iter().position(|slot| slot.key == key) {
                    cursor += offset;
                    slots[cursor].visible[step] = true;
                } else {
                    let mut visible = vec![false; step + 1];
                    visible[step] = true;
                    slots.insert(
                        cursor,
                        Slot {
                            element,
                            key,
                            visible,
                        },
                    );
                }
                cursor += 1;
            }
        }
        let steps = (0..overlays.len())
            .map(|step| AnimationStep {
                visible: slots
                    .iter()
                    .enumerate()
                    .filter_map(|(i, slot)| slot.visible[step].then_some(i))
                    .collect(),
            })
            .collect();
        let mut slide = overlays.remove(0);
        slide.background = if changing_background {
            None
        } else {
            background
        };
        slide.elements = slots.into_iter().map(|slot| slot.element).collect();
        slide.notes = (!notes.is_empty()).then(|| notes.join("\n\n"));
        slide.animation = Some(Animation {
            source_pages: pages.clone().map(|p| p + 1).collect(),
            steps,
        });
        presentation.slides.push(slide);
    }
    Ok(())
}

struct Slot {
    element: Element,
    key: Element,
    visible: Vec<bool>,
}

fn clear_source_ids(element: &mut Element) {
    match element {
        Element::Linked { element, .. } => clear_source_ids(element),
        Element::Group(g) => g.elements.iter_mut().for_each(clear_source_ids),
        Element::Text(t) => t.source_id.clear(),
        Element::Table(t) => t.source_id.clear(),
        Element::MathSvg { source_id, .. } => source_id.clear(),
        _ => {}
    }
}

fn remap_target(target: &mut LinkTarget, destinations: &[usize]) {
    if let LinkTarget::Slide(page) = target
        && let Some(slide) = page.checked_sub(1).and_then(|i| destinations.get(i))
    {
        *page = *slide;
    }
}

fn remap(element: &mut Element, destinations: &[usize]) {
    let paragraphs = match element {
        Element::Linked { target, element } => {
            remap_target(target, destinations);
            remap(element, destinations);
            return;
        }
        Element::Group(g) => {
            for e in &mut g.elements {
                remap(e, destinations);
            }
            return;
        }
        Element::Text(t) => t.paragraphs.iter_mut().collect::<Vec<_>>(),
        Element::Table(t) => t.cells.iter_mut().flat_map(|c| &mut c.paragraphs).collect(),
        _ => return,
    };
    for run in paragraphs.into_iter().flat_map(|p| &mut p.runs) {
        if let Some(target) = &mut run.hyperlink {
            remap_target(target, destinations);
        }
    }
}

fn background_shape(slide: &Slide) -> Element {
    let (width, height) = (slide.width, slide.height);
    Element::Shape(VectorShape {
        bounds: Rect {
            x: 0.,
            y: 0.,
            width,
            height,
        },
        commands: vec![
            PathCommand::Move([0., 0.]),
            PathCommand::Line([width, 0.]),
            PathCommand::Line([width, height]),
            PathCommand::Line([0., height]),
            PathCommand::Close,
        ],
        fill: Some(Brush::Solid {
            color: slide.background.unwrap_or([255; 4]),
        }),
        stroke: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(elements: Vec<Vec<Element>>) -> Presentation {
        Presentation {
            schema_version: 19,
            diagnostics: vec![],
            fonts: vec![],
            slides: elements
                .into_iter()
                .map(|elements| Slide {
                    width: 600.,
                    height: 400.,
                    background: Some([255; 4]),
                    elements,
                    notes: None,
                    links: vec![],
                    animation: None,
                })
                .collect(),
        }
    }

    fn shape(color: [u8; 4]) -> Element {
        Element::Shape(VectorShape {
            bounds: Rect {
                x: 10.,
                y: 10.,
                width: 100.,
                height: 100.,
            },
            commands: vec![
                PathCommand::Move([0., 0.]),
                PathCommand::Line([100., 0.]),
                PathCommand::Line([100., 100.]),
                PathCommand::Close,
            ],
            fill: Some(Brush::Solid { color }),
            stroke: None,
        })
    }

    #[test]
    fn changing_layer_order_and_duplicate_objects_preserve_every_state() {
        let a = shape([255, 0, 0, 255]);
        let b = shape([0, 0, 255, 255]);
        let source = vec![
            vec![a.clone(), b.clone(), a.clone()],
            vec![b.clone(), a.clone(), b],
            vec![a.clone(), a],
        ];
        let mut presentation = deck(source.clone());
        combine(&mut presentation, std::slice::from_ref(&(0..3))).unwrap();
        let slide = &presentation.slides[0];
        for (step, original) in slide.animation.as_ref().unwrap().steps.iter().zip(source) {
            assert_eq!(
                step.visible
                    .iter()
                    .map(|&i| slide.elements[i].clone())
                    .collect::<Vec<_>>(),
                original
            );
        }
    }

    #[test]
    fn picture_identity_includes_binary_data() {
        let picture = |byte| Element::Picture {
            bounds: Rect {
                x: 0.,
                y: 0.,
                width: 10.,
                height: 10.,
            },
            extension: "png".into(),
            bytes: vec![byte],
            clip: None,
            alt: None,
            svg: None,
        };
        let mut presentation = deck(vec![vec![picture(1)], vec![picture(2)]]);
        combine(&mut presentation, std::slice::from_ref(&(0..2))).unwrap();
        let slide = &presentation.slides[0];
        assert_eq!(slide.elements.len(), 2);
        let steps = &slide.animation.as_ref().unwrap().steps;
        assert_ne!(steps[0].visible, steps[1].visible);
    }

    #[test]
    fn backgrounds_and_empty_links_change_with_their_overlay() {
        let mut presentation = deck(vec![vec![], vec![]]);
        presentation.slides[1].background = Some([255, 0, 0, 255]);
        let Element::Shape(region) = shape([0; 4]) else {
            unreachable!()
        };
        presentation.slides[1].links.push(SlideLink {
            target: LinkTarget::Slide(2),
            region,
        });
        combine(&mut presentation, std::slice::from_ref(&(0..2))).unwrap();
        let slide = &presentation.slides[0];
        assert_eq!(slide.background, None);
        let steps = &slide.animation.as_ref().unwrap().steps;
        assert_eq!(steps[0].visible.len(), 1);
        assert_eq!(steps[1].visible.len(), 2);
        for (step, expected) in steps.iter().zip([[255; 4], [255, 0, 0, 255]]) {
            let Element::Shape(background) = &slide.elements[step.visible[0]] else {
                panic!()
            };
            assert_eq!(background.fill, Some(Brush::Solid { color: expected }));
        }
        assert!(matches!(
            &slide.elements[steps[1].visible[1]],
            Element::Linked {
                target: LinkTarget::Slide(1),
                ..
            }
        ));
    }

    #[test]
    fn overlays_with_different_sizes_are_rejected() {
        let mut presentation = deck(vec![vec![], vec![]]);
        presentation.slides[1].width = 700.;
        assert!(
            combine(&mut presentation, std::slice::from_ref(&(0..2)))
                .unwrap_err()
                .to_string()
                .contains("different page sizes")
        );
    }
}
