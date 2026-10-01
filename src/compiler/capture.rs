//! Capture Typst's semantic tags and display list from the SAME compilation.
//! Tag identity, rather than text/coordinate matching, connects the two.
use crate::ir::Rect;
use std::collections::{BTreeMap, HashMap, HashSet};
use typst::foundations::{Content, StyleChain, StyledElem, Value};
use typst::introspection::{Location, Tag};
use typst::layout::{Frame, FrameItem, GridCell, GridElem, Point, Sizing, Transform};
use typst::pdf::{ArtifactElem, ArtifactKind, PdfMarkerTag, PdfMarkerTagKind};
use typst_layout::PagedDocument;

fn page_decoration(content: &Content) -> bool {
    content.to_packed::<ArtifactElem>().is_some_and(|a| {
        matches!(
            a.kind.get(StyleChain::default()),
            ArtifactKind::Header
                | ArtifactKind::Footer
                | ArtifactKind::Background
                | ArtifactKind::Watermark
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    List,
    Enum,
    Label,
    ItemBody,
    Paragraph,
    Heading,
    Equation,
    Bibliography,
    BibEntry,
    Table,
    Cell,
    Other,
}

#[derive(Clone)]
pub struct Node {
    pub content: Content,
    pub kind: Kind,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub pages: BTreeMap<usize, NodePage>,
}

impl Node {
    pub fn id(&self) -> String {
        format!("{:?}", self.content.location().unwrap())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FrameGeometry {
    pub(crate) size: (f64, f64),
    pub(crate) transform: Transform,
}
impl FrameGeometry {
    fn bounds(&self, transform: Transform) -> Rect {
        transformed_rect(
            Point::zero(),
            self.size.0,
            self.size.1,
            transform.pre_concat(self.transform),
        )
    }
}

#[derive(Clone, Debug)]
pub struct NodePage {
    pub(crate) context_frame: FrameGeometry,
    pub(crate) layout_frames: Vec<FrameGeometry>,
    pub(crate) baseline_point: Option<Point>,
    pub origin: (f64, f64),
    pub end: Option<(f64, f64)>,
    pub baseline: Option<f64>,
    pub context: Rect,
    pub layout: Option<(usize, Rect)>,
    pub leaves: Vec<usize>,
}

#[derive(Clone)]
pub struct Leaf {
    pub(crate) frame_geometry: FrameGeometry,
    pub(crate) paint_geometry: FrameGeometry,
    pub item: FrameItem,
    pub position: (f64, f64),
    pub transform: Transform,
    pub clipped: bool,
    pub(crate) clips: Vec<crate::geometry::paths::Contours>,
    pub ancestors: Vec<usize>,
    pub frame: Rect,
    pub kerning: bool,
    /// Resolved source tracking in untransformed points. Glyph advances already
    /// include this spacing; it is retained separately for native text styles.
    pub tracking: f64,
}

impl Leaf {
    pub fn is_drawable(&self) -> bool {
        !matches!(self.item, FrameItem::Link(..))
    }
    pub fn plain_transform(&self) -> bool {
        (!self.clipped
            || self.ink_bounds().is_some_and(|b| {
                self.clips
                    .iter()
                    .all(|clip| crate::geometry::paths::contains_rect(clip, b))
            }))
            && self.unclipped_transform()
    }
    pub(crate) fn unclipped_transform(&self) -> bool {
        self.transform.sx.get() > 0.0
            && (self.transform.sx.get() - self.transform.sy.get()).abs() < 1e-6
            && self.transform.kx.get().abs() < 1e-6
            && self.transform.ky.get().abs() < 1e-6
    }
    pub fn scale(&self) -> f64 {
        self.transform.sx.get()
    }
    pub(crate) fn ink_bounds(&self) -> Option<Rect> {
        let leaf = self;
        let bbox = match &leaf.item {
            FrameItem::Text(text) => {
                let bbox = text.bbox();
                let pad = Point::splat(
                    text.stroke
                        .as_ref()
                        .map_or(typst::layout::Abs::zero(), |s| s.thickness / 2.),
                );
                typst::layout::Rect::new(bbox.min.min(bbox.max) - pad, bbox.min.max(bbox.max) + pad)
            }
            FrameItem::Shape(shape, _) => shape.bbox(true),
            FrameItem::Image(_, size, _) => {
                typst::layout::Rect::from_pos_size(Point::zero(), *size)
            }
            _ => return None,
        };
        let origin = Point::zero().transform(leaf.transform);
        let points = [
            bbox.min,
            bbox.max,
            Point::new(bbox.min.x, bbox.max.y),
            Point::new(bbox.max.x, bbox.min.y),
        ]
        .map(|p| p.transform(leaf.transform))
        .map(|p| {
            (
                p.x.to_pt() - origin.x.to_pt() + leaf.position.0,
                p.y.to_pt() - origin.y.to_pt() + leaf.position.1,
            )
        });
        let x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let right = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let bottom = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        (x.is_finite() && y.is_finite() && right.is_finite() && bottom.is_finite()).then_some(
            Rect {
                x,
                y,
                width: right - x,
                height: bottom - y,
            },
        )
    }
}

#[derive(Clone)]
struct CellContinuation {
    stack: Vec<usize>,
    text_styles: Vec<(Location, bool, f64, f64)>,
}

#[derive(Default, Clone)]
pub struct Capture {
    /// Resolved targets indexed by the semantic link-marker node. Text and
    /// pictures share the same source identity, including internal references.
    pub(crate) link_targets: HashMap<usize, crate::ir::LinkTarget>,
    /// Equation nodes replaced by layout placeholders for SVG export.
    pub(crate) svg_math: HashSet<usize>,
    pub(crate) inline_objects: HashSet<(usize, usize)>,
    pub(crate) fixed_math_layout: HashSet<usize>,
    pub nodes: Vec<Node>,
    pub pages: Vec<Vec<Leaf>>,
    pub paragraph_alignment: HashMap<Location, String>,
    pub paragraph_rtl: HashMap<Location, bool>,
    pub equations: HashMap<Location, Content>,
    pub resolved: HashMap<Location, Content>,
    pub page_bounds: Vec<Rect>,
    /// Realized hard breaks: next display-list leaf and owning semantic node.
    pub line_breaks: Vec<Vec<(usize, Option<usize>)>>,
    pub horizontal_spaces: Vec<Vec<(usize, Option<usize>)>>,
    stack: Vec<usize>,
    text_styles: Vec<(Location, bool, f64, f64)>,
    /// Page decorations interrupt, but do not belong to, continued body tags.
    page_artifacts: Vec<(Location, Vec<usize>)>,
    /// Each split cell owns its open tags and styles across physical pages.
    /// They must not leak into table borders or neighboring cell fragments.
    cell_continuations: HashMap<usize, CellContinuation>,
    locations: HashMap<Location, usize>,
}

impl Capture {
    pub fn new(document: &PagedDocument) -> Self {
        let mut result = Self::default();
        for (page, data) in document.pages().iter().enumerate() {
            result.pages.push(Vec::new());
            result.line_breaks.push(Vec::new());
            result.horizontal_spaces.push(Vec::new());
            result.page_bounds.push(Rect {
                x: 0.,
                y: 0.,
                width: data.frame.width().to_pt(),
                height: data.frame.height().to_pt(),
            });
            result.walk(&data.frame, Transform::identity(), &[], 0, page, None);
        }
        for leaves in &result.pages {
            for leaf in leaves {
                let FrameItem::Link(dest, _) = &leaf.item else {
                    continue;
                };
                let Some(&node) = leaf
                    .ancestors
                    .iter()
                    .rev()
                    .find(|&&i| result.nodes[i].content.elem().name() == "link-marker")
                else {
                    continue;
                };
                use typst::model::Destination;
                let target = match dest {
                    Destination::Url(url) => crate::ir::LinkTarget::Url(url.to_string()),
                    Destination::Position(p) => crate::ir::LinkTarget::Slide(p.page.get()),
                    Destination::Location(l) => {
                        let Some(p) = document.introspector().position(*l) else {
                            continue;
                        };
                        crate::ir::LinkTarget::Slide(p.page.get())
                    }
                };
                result.link_targets.insert(node, target);
            }
        }
        result
    }

    pub(crate) fn link_target(&self, leaf: &Leaf) -> Option<&crate::ir::LinkTarget> {
        leaf.ancestors
            .iter()
            .rev()
            .find_map(|i| self.link_targets.get(i))
    }

    fn walk(
        &mut self,
        frame: &Frame,
        transform: Transform,
        clips: &[crate::geometry::paths::Contours],
        depth: usize,
        page: usize,
        paint_geometry: Option<&FrameGeometry>,
    ) {
        let bounds = transformed_rect(
            Point::zero(),
            frame.size().x.to_pt(),
            frame.size().y.to_pt(),
            transform,
        );
        let geometry = FrameGeometry {
            size: (frame.width().to_pt(), frame.height().to_pt()),
            transform,
        };
        let paint_geometry = if frame.kind() == typst::layout::FrameKind::Hard {
            &geometry
        } else {
            paint_geometry.unwrap_or(&geometry)
        };
        for (position, item) in frame.items() {
            let position_global = position.transform(transform);
            let xy = (position_global.x.to_pt(), position_global.y.to_pt());
            match item {
                FrameItem::Tag(Tag::Start(content, _)) => {
                    if content.elem().name() == "metadata"
                        && let Ok(Value::Dict(value)) = content.field_by_name("value")
                        && let Ok(Value::Bool(kerning)) = value.get("typptx-kerning")
                    {
                        self.text_styles.push((
                            content.location().unwrap(),
                            *kerning,
                            match value.get("typptx-tracking") {
                                Ok(Value::Float(tracking)) => *tracking,
                                _ => 0.,
                            },
                            match value.get("typptx-text-size") {
                                Ok(Value::Float(size)) => *size,
                                _ => 0.,
                            },
                        ));
                        continue;
                    }
                    if content.elem().name() == "metadata"
                        && let Ok(Value::Dict(value)) = content.field_by_name("value")
                        && matches!(value.get("typptx-hspace"), Ok(Value::Bool(true)))
                    {
                        self.horizontal_spaces[page]
                            .push((self.pages[page].len(), self.stack.last().copied()));
                        continue;
                    }
                    if content.elem().name() == "metadata"
                        && let Ok(Value::Dict(value)) = content.field_by_name("value")
                        && matches!(value.get("typptx-linebreak"), Ok(Value::Bool(true)))
                    {
                        self.line_breaks[page]
                            .push((self.pages[page].len(), self.stack.last().copied()));
                        continue;
                    }
                    if content.elem().name() == "metadata"
                        && let Ok(Value::Dict(value)) = content.field_by_name("value")
                        && let Ok(source) = value.get("typptx-source")
                        && let Ok(location) = source.clone().cast::<Location>()
                        && let Ok(Value::Str(alignment)) = value.get("alignment")
                    {
                        self.paragraph_alignment
                            .insert(location, alignment.to_string());
                        if let Ok(Value::Bool(rtl)) = value.get("rtl") {
                            self.paragraph_rtl.insert(location, *rtl);
                        }
                        if let Ok(Value::Content(resolved)) = value.get("resolved") {
                            self.resolved.insert(location, resolved.clone());
                        }
                        if let Ok(Value::Content(equation)) = value.get("equation") {
                            self.equations.insert(location, equation.clone());
                        }
                        continue;
                    }
                    let location = content.location().unwrap();
                    if depth == 0 && page_decoration(content) {
                        self.page_artifacts
                            .push((location, std::mem::take(&mut self.stack)));
                    }
                    let idx = if let Some(&idx) = self.locations.get(&location) {
                        idx
                    } else {
                        let idx = self.nodes.len();
                        let parent = self.stack.last().copied();
                        self.nodes.push(Node {
                            content: content.clone(),
                            kind: kind(content),
                            parent,
                            children: Vec::new(),
                            pages: BTreeMap::new(),
                        });
                        self.locations.insert(location, idx);
                        if let Some(parent) = parent {
                            self.nodes[parent].children.push(idx);
                        }
                        idx
                    };
                    self.nodes[idx]
                        .pages
                        .entry(page)
                        .or_insert_with(|| NodePage {
                            context_frame: geometry.clone(),
                            layout_frames: Vec::new(),
                            baseline_point: None,
                            origin: xy,
                            end: None,
                            baseline: None,
                            context: bounds,
                            layout: None,
                            leaves: Vec::new(),
                        });
                    self.stack.push(idx);
                }
                FrameItem::Tag(Tag::End(location, ..)) => {
                    if let Some(at) = self
                        .text_styles
                        .iter()
                        .rposition(|(loc, _, _, _)| loc == location)
                    {
                        self.text_styles.remove(at);
                    }
                    if let Some(&idx) = self.locations.get(location)
                        && let Some(at) = self.stack.iter().rposition(|&v| v == idx)
                    {
                        if let Some(np) = self.nodes[idx].pages.get_mut(&page) {
                            np.end = Some(xy);
                        }
                        self.stack.remove(at);
                    }
                    if self
                        .page_artifacts
                        .last()
                        .is_some_and(|(loc, _)| loc == location)
                    {
                        self.stack = self.page_artifacts.pop().unwrap().1;
                    }
                }
                FrameItem::Group(group) => {
                    // Page overlays may keep their artifact tags inside a
                    // group, or flatten them directly into the page frame.
                    // Isolate the whole group before recording its bounds.
                    let decoration_stack = (depth == 0
                        && group.frame.items().any(|(_, item)| {
                            matches!(item, FrameItem::Tag(Tag::Start(c, _)) if page_decoration(c))
                        }))
                    .then(|| std::mem::take(&mut self.stack));
                    // Split cells use FrameParent instead of surrounding tags.
                    let inherited_cell = group
                        .parent
                        .and_then(|parent| self.locations.get(&parent.location).copied())
                        .filter(|&idx| {
                            self.nodes[idx].kind == Kind::Cell && !self.stack.contains(&idx)
                        });
                    let cell_context = inherited_cell.map(|idx| {
                        let context = self.cell_continuations.remove(&idx).unwrap_or_else(|| {
                            let mut stack = vec![idx];
                            let mut parent = self.nodes[idx].parent;
                            while let Some(i) = parent {
                                stack.push(i);
                                parent = self.nodes[i].parent;
                            }
                            stack.reverse();
                            CellContinuation {
                                stack,
                                text_styles: self.text_styles.clone(),
                            }
                        });
                        CellContinuation {
                            stack: std::mem::replace(&mut self.stack, context.stack),
                            text_styles: std::mem::replace(
                                &mut self.text_styles,
                                context.text_styles,
                            ),
                        }
                    });
                    let ts = transform
                        .pre_concat(Transform::translate(position.x, position.y))
                        .pre_concat(group.transform);
                    let group_bounds = transformed_rect(
                        Point::zero(),
                        group.frame.width().to_pt(),
                        group.frame.height().to_pt(),
                        ts,
                    );
                    for &idx in &self.stack {
                        let np = self.nodes[idx]
                            .pages
                            .entry(page)
                            .or_insert_with(|| NodePage {
                                context_frame: geometry.clone(),
                                layout_frames: Vec::new(),
                                baseline_point: None,
                                origin: (group_bounds.x, group_bounds.y),
                                end: None,
                                baseline: None,
                                context: bounds,
                                layout: None,
                                leaves: Vec::new(),
                            });
                        let layout_frame = FrameGeometry {
                            size: (group.frame.width().to_pt(), group.frame.height().to_pt()),
                            transform: ts,
                        };
                        match np.layout {
                            Some((d, old)) if d == depth => {
                                np.layout = Some((d, old.union(group_bounds)));
                                np.layout_frames.push(layout_frame);
                            }
                            Some((d, _)) if d < depth => {}
                            _ => {
                                np.layout = Some((depth, group_bounds));
                                np.layout_frames = vec![layout_frame];
                                np.baseline_point =
                                    Some(Point::with_y(group.frame.baseline()).transform(ts));
                                np.baseline = Some(
                                    Point::with_y(group.frame.baseline())
                                        .transform(ts)
                                        .y
                                        .to_pt(),
                                );
                            }
                        }
                    }
                    let mut child_clips = clips.to_vec();
                    if let Some(clip) = &group.clip
                        && let Some(path) = crate::geometry::paths::curve(clip, ts)
                    {
                        child_clips.push(crate::geometry::paths::simplify(&path, false));
                    }
                    self.walk(
                        &group.frame,
                        ts,
                        &child_clips,
                        depth + 1,
                        page,
                        Some(paint_geometry),
                    );
                    if let Some((idx, outer)) = inherited_cell.zip(cell_context) {
                        self.cell_continuations.insert(
                            idx,
                            CellContinuation {
                                stack: std::mem::replace(&mut self.stack, outer.stack),
                                text_styles: std::mem::replace(
                                    &mut self.text_styles,
                                    outer.text_styles,
                                ),
                            },
                        );
                    }
                    if let Some(stack) = decoration_stack {
                        self.stack = stack;
                    }
                }
                _ => {
                    let index = self.pages[page].len();
                    for &idx in &self.stack {
                        let np = self.nodes[idx]
                            .pages
                            .entry(page)
                            .or_insert_with(|| NodePage {
                                context_frame: geometry.clone(),
                                layout_frames: Vec::new(),
                                baseline_point: None,
                                origin: xy,
                                end: None,
                                baseline: None,
                                context: bounds,
                                layout: None,
                                leaves: Vec::new(),
                            });
                        np.leaves.push(index);
                    }
                    self.pages[page].push(Leaf {
                        frame_geometry: geometry.clone(),
                        paint_geometry: paint_geometry.clone(),
                        item: item.clone(),
                        position: xy,
                        transform,
                        clipped: !clips.is_empty(),
                        clips: clips.to_vec(),
                        ancestors: self.stack.clone(),
                        frame: bounds,
                        kerning: self.text_styles.last().is_none_or(|(_, kern, _, _)| *kern),
                        // Typst resolves tracking before synthesizing script
                        // glyph sizes, then stores it in em advances. Apply
                        // the same glyph-size ratio for the native text style.
                        tracking: self.text_styles.last().map_or(0., |(_, _, value, size)| {
                            if let FrameItem::Text(text) = item
                                && *size > 0.
                            {
                                *value * text.size.to_pt() / *size
                            } else {
                                *value
                            }
                        }),
                    });
                }
            }
        }
    }

    pub(crate) fn untransformed(&self, page: usize, transform: Transform) -> Option<Self> {
        let inverse = transform.invert()?;
        let mut out = self.clone();
        let point = |p: (f64, f64)| {
            let p = Point::new(typst::layout::Abs::pt(p.0), typst::layout::Abs::pt(p.1))
                .transform(inverse);
            (p.x.to_pt(), p.y.to_pt())
        };
        for leaf in &mut out.pages[page] {
            leaf.position = point(leaf.position);
            for contour in leaf.clips.iter_mut().flatten() {
                for p in contour {
                    let q = point((p[0], p[1]));
                    *p = [q.0, q.1];
                }
            }
            leaf.transform = inverse.pre_concat(leaf.transform);
            leaf.frame = leaf.frame_geometry.bounds(inverse);
            leaf.frame_geometry.transform = inverse.pre_concat(leaf.frame_geometry.transform);
            leaf.paint_geometry.transform = inverse.pre_concat(leaf.paint_geometry.transform);
        }
        for node in &mut out.nodes {
            if let Some(np) = node.pages.get_mut(&page) {
                np.origin = point(np.origin);
                np.end = np.end.map(point);
                np.context = np.context_frame.bounds(inverse);
                np.context_frame.transform = inverse.pre_concat(np.context_frame.transform);
                if let Some(p) = np.baseline_point {
                    let p = p.transform(inverse);
                    np.baseline = Some(p.y.to_pt());
                    np.baseline_point = Some(p);
                }
                if let Some((depth, _)) = np.layout {
                    if let Some(bounds) = np
                        .layout_frames
                        .iter()
                        .map(|g| g.bounds(inverse))
                        .reduce(Rect::union)
                    {
                        np.layout = Some((depth, bounds));
                    }
                    for g in &mut np.layout_frames {
                        g.transform = inverse.pre_concat(g.transform);
                    }
                }
            }
        }
        Some(out)
    }

    pub fn descendants(&self, idx: usize) -> Vec<usize> {
        let mut result = Vec::new();
        for &child in &self.nodes[idx].children {
            result.push(child);
            result.extend(self.descendants(child));
        }
        result
    }

    pub fn nearest(&self, leaf: &Leaf, kind: Kind) -> Option<usize> {
        leaf.ancestors
            .iter()
            .rev()
            .copied()
            .find(|&i| self.nodes[i].kind == kind)
    }

    pub(crate) fn is_svg_math(&self, leaf: &Leaf) -> bool {
        leaf.ancestors.iter().any(|i| self.svg_math.contains(i))
    }

    pub(crate) fn has_fixed_math_layout(&self, page: usize, ids: &[usize]) -> bool {
        ids.iter().any(|&id| {
            self.pages[page][id]
                .ancestors
                .iter()
                .any(|idx| self.fixed_math_layout.contains(idx))
        })
    }

    /// Keep the text containing an inline picture or SVG equation on its
    /// realized lines. Generic contexts can contain unrelated cells and even
    /// whole documents, so they must never propagate this text-layout mode.
    pub(crate) fn fix_inline_layout(&mut self, idx: usize) {
        let mut ancestor = Some(idx);
        while let Some(i) = ancestor {
            if self.nodes[i].kind == Kind::Table {
                break;
            }
            if matches!(
                self.nodes[i].kind,
                Kind::Equation
                    | Kind::Paragraph
                    | Kind::Heading
                    | Kind::List
                    | Kind::Enum
                    | Kind::Bibliography
                    | Kind::Cell
            ) {
                self.fixed_math_layout.insert(i);
            }
            ancestor = self.nodes[i].parent;
        }
    }

    /// Recover a grid cell's horizontal extent when Typst has flattened its
    /// frame into the grid. Semantic cell origins and resolved gutter tracks
    /// retain the boundary; the surrounding frame can cover several columns.
    pub fn grid_cell_extent(&self, mut idx: usize, page: usize) -> Option<(f64, f64)> {
        while !self.nodes[idx].content.is::<GridCell>() {
            idx = self.nodes[idx].parent?;
        }
        let cell = &self.nodes[idx];
        let column = cell.content.field_by_name("x").ok()?.cast::<usize>().ok()?;
        let span = cell
            .content
            .field_by_name("colspan")
            .ok()
            .and_then(|v| v.cast::<usize>().ok())
            .unwrap_or(1);
        let mut parent = cell.parent?;
        while !self.nodes[parent].content.is::<GridElem>() {
            parent = self.nodes[parent].parent?;
        }
        let node = &self.nodes[parent];
        let grid = node.content.to_packed::<GridElem>()?.grid.as_ref()?;
        let bounds = node.pages.get(&page)?.layout?.1;
        let rtl = node
            .content
            .location()
            .and_then(|loc| self.paragraph_rtl.get(&loc))
            .copied()
            .unwrap_or(false);
        let end_column = if rtl { column } else { column + span };
        let right =
            if (rtl && column == 0) || (!rtl && end_column == grid.non_gutter_column_count()) {
                bounds.right()
            } else {
                let next = node.children.iter().find_map(|&i| {
                    let next = &self.nodes[i];
                    let x = next.content.field_by_name("x").ok()?.cast::<usize>().ok()?;
                    let span = next
                        .content
                        .field_by_name("colspan")
                        .ok()
                        .and_then(|v| v.cast::<usize>().ok())
                        .unwrap_or(1);
                    (next.content.is::<GridCell>()
                        && if rtl {
                            x + span == end_column
                        } else {
                            x == end_column
                        })
                    .then(|| next.pages.get(&page).map(|p| p.origin.0))
                    .flatten()
                })?;
                let gutter = if grid.has_gutter {
                    match grid.cols.get(end_column * 2 - 1)? {
                        Sizing::Rel(size) if size.rel.get().abs() < 1e-6 => {
                            let resolved = self
                                .resolved
                                .get(&node.content.location()?)?
                                .to_packed::<StyledElem>()?;
                            let font_size = StyleChain::new(&resolved.styles)
                                .resolve(typst::text::TextElem::size);
                            size.abs.at(font_size).to_pt()
                        }
                        _ => return None,
                    }
                } else {
                    0.0
                };
                next - gutter
            };
        let left = cell.pages.get(&page)?.origin.0;
        (right > left).then_some((left, right))
    }
}

fn kind(content: &Content) -> Kind {
    if let Some(marker) = content.to_packed::<PdfMarkerTag>() {
        return match marker.kind {
            PdfMarkerTagKind::Bibliography(true) => Kind::Bibliography,
            PdfMarkerTagKind::BibEntry => Kind::BibEntry,
            PdfMarkerTagKind::ListItemLabel => Kind::Label,
            PdfMarkerTagKind::ListItemBody => Kind::ItemBody,
            _ => Kind::Other,
        };
    }
    match content.elem().name() {
        "list" => Kind::List,
        "enum" => Kind::Enum,
        "par" => Kind::Paragraph,
        "heading" => Kind::Heading,
        "equation" => Kind::Equation,
        "table" => Kind::Table,
        "cell" => Kind::Cell,
        _ => Kind::Other,
    }
}

pub fn transformed_rect(origin: Point, width: f64, height: f64, ts: Transform) -> Rect {
    use typst::layout::Abs;
    let points = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
        .map(|(x, y)| (origin + Point::new(Abs::pt(x), Abs::pt(y))).transform(ts));
    let x = points
        .iter()
        .map(|p| p.x.to_pt())
        .fold(f64::INFINITY, f64::min);
    let y = points
        .iter()
        .map(|p| p.y.to_pt())
        .fold(f64::INFINITY, f64::min);
    let right = points
        .iter()
        .map(|p| p.x.to_pt())
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = points
        .iter()
        .map(|p| p.y.to_pt())
        .fold(f64::NEG_INFINITY, f64::max);
    Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

/// Retain original transforms, clipping, and paint order while selecting a
/// contiguous portion of the display list. Native objects replace other leaves.
pub fn filter_frame(frame: &Frame, wanted: &dyn Fn(usize) -> bool, cursor: &mut usize) -> Frame {
    let mut result = Frame::new(frame.size(), frame.kind());
    for (pos, item) in frame.items() {
        match item {
            FrameItem::Group(group) => {
                let mut group = group.clone();
                group.frame = filter_frame(&group.frame, wanted, cursor);
                if !group.frame.is_empty() {
                    result.push(*pos, FrameItem::Group(group));
                }
            }
            FrameItem::Tag(_) => {}
            _ => {
                if wanted(*cursor) {
                    result.push(*pos, item.clone());
                }
                *cursor += 1;
            }
        }
    }
    result
}
