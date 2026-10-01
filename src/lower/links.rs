//! Preserve internal links from Typst's resolved, positioned link regions.
use crate::{compiler::capture::Leaf, geometry::paths, ir::*};
use typst::{layout::FrameItem, model::Destination};

pub(super) fn collect(leaves: &[Leaf], document: &typst_layout::PagedDocument) -> Vec<SlideLink> {
    leaves
        .iter()
        .filter_map(|leaf| {
            let FrameItem::Link(dest, size) = &leaf.item else {
                return None;
            };
            let target = match dest {
                Destination::Position(p) => p.page.get(),
                Destination::Location(l) => document.introspector().position(*l)?.page.get(),
                Destination::Url(_) => return None, // Text runs already carry URL links.
            };
            if target > document.pages().len() {
                return None;
            }
            let t = leaf.transform;
            let point = |x: f64, y: f64| {
                [
                    leaf.position.0 + t.sx.get() * x + t.kx.get() * y,
                    leaf.position.1 + t.ky.get() * x + t.sy.get() * y,
                ]
            };
            let mut area = vec![vec![
                point(0., 0.),
                point(size.x.to_pt(), 0.),
                point(size.x.to_pt(), size.y.to_pt()),
                point(0., size.y.to_pt()),
            ]];
            for clip in &leaf.clips {
                area = paths::intersect(&area, clip);
            }
            let region = paths::shape(
                &paths::path(&area)?,
                [0., 0.],
                Some(Brush::Solid {
                    color: [0, 0, 0, 0],
                }),
            )?;
            Some(SlideLink { target, region })
        })
        .collect()
}
