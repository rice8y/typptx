//! Logical slide boundaries published by Touying and Polylux.
use anyhow::{Result, ensure};
use std::ops::Range;
use typst::foundations::{NativeElement, Value};
use typst::introspection::{Introspector, MetadataElem};
use typst_layout::PagedDocument;

pub fn groups(document: &PagedDocument) -> Result<Vec<Range<usize>>> {
    let introspector = document.introspector();
    let count = document.pages().len();
    let mut starts = vec![false; count];
    let mut steps = vec![false; count];
    let mut touying = vec![false; count];
    let mut subslides = vec![false; count];
    let mut overlay_starts = vec![false; count];
    for content in introspector.query(&MetadataElem::ELEM.select()) {
        let Some(label) = content.label() else {
            continue;
        };
        let label = label.resolve();
        if !matches!(&*label, "touying-metadata" | "pdfpc") {
            continue;
        }
        let Some(page) = content
            .location()
            .and_then(|loc| introspector.position(loc))
            .map(|pos| pos.page.get() - 1)
        else {
            continue;
        };
        ensure!(
            page < count,
            "animation metadata points outside the document"
        );
        let metadata = content.to_packed::<MetadataElem>().unwrap();
        let Value::Dict(value) = &metadata.value else {
            continue;
        };
        if &*label == "touying-metadata" {
            match value.get("kind") {
                Ok(Value::Str(kind))
                    if matches!(
                        kind.as_str(),
                        "touying-new-slide" | "touying-new-subslide" | "touying-new-page"
                    ) =>
                {
                    touying[page] = true;
                    steps[page] = true;
                    starts[page] |= kind.as_str() == "touying-new-slide";
                    subslides[page] |= kind.as_str() == "touying-new-subslide";
                }
                _ => {}
            }
        } else if matches!(value.get("t"), Ok(Value::Str(tag)) if tag.as_str() == "Overlay") {
            let Ok(Value::Int(overlay)) = value.get("v") else {
                continue;
            };
            ensure!(*overlay >= 0, "animation overlay index must be nonnegative");
            steps[page] = true;
            overlay_starts[page] |= *overlay == 0;
        }
    }
    let mut groups: Vec<Range<usize>> = Vec::new();
    for page in 0..count {
        // Touying's page header repeats Overlay=0 on overflow pages. Its
        // explicit logical-slide marker is authoritative for those pages.
        let start = starts[page] || (!touying[page] && overlay_starts[page]);
        if page > 0 && steps[page] && steps[page - 1] && !start {
            groups.last_mut().unwrap().end = page + 1;
        } else {
            groups.push(page..page + 1);
        }
    }
    // Ordinary multipage content and handouts have only one subslide. Keep
    // their pages static; overflow creates extra click states only when the
    // logical slide actually contains animation steps.
    Ok(groups
        .into_iter()
        .flat_map(|pages| {
            if touying[pages.start] && subslides[pages.clone()].iter().filter(|&&s| s).count() == 1
            {
                pages.map(|page| page..page + 1).collect::<Vec<_>>()
            } else {
                vec![pages]
            }
        })
        .collect())
}
