//! Native Appear/Disappear effects, with all changes in a step on one click.
use super::drawing::enumeration;
use crate::ir::Animation;
use anyhow::{Result, ensure};
use ooxmlsdk::schemas::p;
use std::collections::BTreeSet;

type Node = p::ChildTimeNodeListChoice;

pub(super) fn timing(
    animation: &Animation,
    shape_ids: &[u32],
    pause_id: Option<u32>,
) -> Result<Box<p::Timing>> {
    ensure!(
        animation.steps.len() >= 2,
        "an animation needs at least two states"
    );
    let mut states = Vec::new();
    for step in &animation.steps {
        ensure!(
            step.visible.iter().all(|&i| i < shape_ids.len()),
            "animation target is outside the slide"
        );
        let state: BTreeSet<_> = step.visible.iter().copied().collect();
        ensure!(
            state.len() == step.visible.len(),
            "animation repeats a visible target"
        );
        ensure!(
            step.visible.windows(2).all(|pair| pair[0] < pair[1]),
            "animation visibility must follow the slide's stacking order"
        );
        states.push(state);
    }
    ensure!(
        states
            .iter()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            == shape_ids.len(),
        "animation contains an object that is never visible"
    );
    let mut next_id = 3;
    let mut steps = Vec::new();
    for pair in states.windows(2) {
        let mut effects = Vec::new();
        for (targets, visible) in [
            (pair[0].difference(&pair[1]), false),
            (pair[1].difference(&pair[0]), true),
        ] {
            for &index in targets {
                effects.push(effect(
                    &mut next_id,
                    shape_ids[index],
                    visible,
                    effects.is_empty(),
                ));
            }
        }
        // PowerPoint removes an empty time node instead of consuming a click.
        // An entrance on a transparent shape keeps the source pause intact.
        if effects.is_empty() {
            let target = pause_id
                .ok_or_else(|| anyhow::anyhow!("an empty animation step needs a pause target"))?;
            effects.push(effect(&mut next_id, target, true, true));
        }
        let effect_group = parallel(p::CommonTimeNode {
            id: Some(take_id(&mut next_id)),
            fill: Some(enumeration("hold")),
            start_condition_list: Some(delay("0")),
            child_time_node_list: Some(children(effects)),
            ..Default::default()
        });
        steps.push(parallel(p::CommonTimeNode {
            id: Some(take_id(&mut next_id)),
            fill: Some(enumeration("hold")),
            start_condition_list: Some(delay("indefinite")),
            child_time_node_list: Some(children(vec![effect_group])),
            ..Default::default()
        }));
    }
    let sequence = Node::SequenceTimeNode(Box::new(p::SequenceTimeNode {
        concurrent: Some(true.into()),
        next_action: Some(enumeration("seek")),
        common_time_node: Box::new(p::CommonTimeNode {
            id: Some(2),
            duration: Some("indefinite".into()),
            node_type: Some(enumeration("mainSeq")),
            child_time_node_list: Some(children(steps)),
            ..Default::default()
        }),
        previous_condition_list: Some(p::PreviousConditionList {
            condition: vec![slide_event("onPrev")],
        }),
        next_condition_list: Some(p::NextConditionList {
            condition: vec![slide_event("onNext")],
        }),
        ..Default::default()
    }));
    Ok(Box::new(p::Timing {
        time_node_list: Some(Box::new(p::TimeNodeList {
            parallel_time_node: Box::new(p::ParallelTimeNode {
                common_time_node: Box::new(p::CommonTimeNode {
                    id: Some(1),
                    duration: Some("indefinite".into()),
                    restart: Some(enumeration("never")),
                    node_type: Some(enumeration("tmRoot")),
                    child_time_node_list: Some(children(vec![sequence])),
                    ..Default::default()
                }),
            }),
        })),
        ..Default::default()
    }))
}

pub(super) fn needs_pause(animation: &Animation) -> bool {
    animation
        .steps
        .windows(2)
        .any(|pair| pair[0].visible == pair[1].visible)
}

pub(super) fn pause_shape(id: usize) -> p::ShapeTreeChoice {
    use crate::ir::{Brush, PathCommand, Rect, VectorShape};
    let mut shape = super::drawing::vector(
        &VectorShape {
            bounds: Rect {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            },
            commands: vec![
                PathCommand::Move([0., 0.]),
                PathCommand::Line([1., 0.]),
                PathCommand::Line([1., 1.]),
                PathCommand::Line([0., 1.]),
                PathCommand::Close,
            ],
            fill: Some(Brush::Solid { color: [0; 4] }),
            stroke: None,
        },
        id,
    );
    shape
        .non_visual_shape_properties
        .non_visual_drawing_properties
        .name = "Animation pause".into();
    p::ShapeTreeChoice::Shape(Box::new(shape))
}

fn take_id(id: &mut u32) -> u32 {
    let result = *id;
    *id += 1;
    result
}
fn children(nodes: Vec<Node>) -> p::ChildTimeNodeList {
    p::ChildTimeNodeList {
        child_time_node_list_choice: nodes,
    }
}
fn parallel(node: p::CommonTimeNode) -> Node {
    Node::ParallelTimeNode(Box::new(p::ParallelTimeNode {
        common_time_node: Box::new(node),
    }))
}
fn delay(value: &str) -> p::StartConditionList {
    p::StartConditionList {
        condition: vec![p::Condition {
            delay: Some(value.into()),
            ..Default::default()
        }],
    }
}
fn slide_event(event: &str) -> p::Condition {
    p::Condition {
        event: Some(enumeration(event)),
        delay: Some("0".into()),
        condition_choice: Some(p::ConditionChoice::TargetElement(Box::new(
            p::TargetElement {
                target_element_choice: Some(p::TargetElementChoice::SlideTarget),
            },
        ))),
    }
}
fn effect(id: &mut u32, target: u32, visible: bool, first: bool) -> Node {
    let behavior = p::SetBehavior {
        common_behavior: Box::new(p::CommonBehavior {
            common_time_node: Box::new(p::CommonTimeNode {
                id: Some(take_id(id)),
                duration: Some("1".into()),
                fill: Some(enumeration("hold")),
                start_condition_list: Some(delay("0")),
                ..Default::default()
            }),
            target_element: Box::new(p::TargetElement {
                target_element_choice: Some(p::TargetElementChoice::ShapeTarget(Box::new(
                    p::ShapeTarget {
                        shape_id: target.to_string(),
                        ..Default::default()
                    },
                ))),
            }),
            attribute_name_list: Some(p::AttributeNameList {
                attribute_name: vec!["style.visibility".into()],
            }),
            ..Default::default()
        }),
        to_variant_value: Some(Box::new(p::ToVariantValue {
            to_variant_value_choice: Some(p::ToVariantValueChoice::StringVariantValue(
                p::StringVariantValue {
                    val: if visible { "visible" } else { "hidden" }.into(),
                },
            )),
        })),
    };
    parallel(p::CommonTimeNode {
        id: Some(take_id(id)),
        preset_id: Some(1),
        preset_subtype: Some(0),
        preset_class: Some(enumeration(if visible { "entr" } else { "exit" })),
        fill: Some(enumeration("hold")),
        node_type: Some(enumeration(if first {
            "clickEffect"
        } else {
            "withEffect"
        })),
        start_condition_list: Some(delay("0")),
        child_time_node_list: Some(children(vec![Node::SetBehavior(Box::new(behavior))])),
        ..Default::default()
    })
}

pub(super) fn shape_id(node: &p::ShapeTreeChoice) -> u32 {
    match node {
        p::ShapeTreeChoice::Shape(s) => {
            s.non_visual_shape_properties
                .non_visual_drawing_properties
                .id
        }
        p::ShapeTreeChoice::Picture(s) => {
            s.non_visual_picture_properties
                .non_visual_drawing_properties
                .id
        }
        p::ShapeTreeChoice::GroupShape(s) => {
            s.non_visual_group_shape_properties
                .non_visual_drawing_properties
                .id
        }
        p::ShapeTreeChoice::GraphicFrame(s) => {
            s.non_visual_graphic_frame_properties
                .non_visual_drawing_properties
                .id
        }
        _ => unreachable!("animated elements have a native shape or group target"),
    }
}
