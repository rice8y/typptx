//! Retain resolved paragraph styles in non-rendering compiler tags.
//!
//! Paragraph tags alone only materialize fields owned by `par`; alignment is
//! owned by `align`. Capture it at Typst's realization boundary, using the same
//! style chain that the layouter sees. This avoids coordinate-based guesses.
use std::sync::LazyLock;
use typst::diag::SourceResult;
use typst::engine::Engine;
use typst::foundations::{Content, Dict, IntoValue, NativeElement, Resolve, Smart, StyleChain};
use typst::introspection::{MetadataElem, SplitLocator, Tag, TagElem, TagFlags};
use typst::layout::{Abs, AlignElem, Binding, Dir, FixedAlignment, Length, PageElem, Rel, Size};
use typst::math::EquationElem;
use typst::model::ParElem;
use typst::routines::{Arenas, FragmentKind, Pair, RealizationKind, Routines};
use typst::text::{LinebreakElem, SmartQuoteElem, SpaceElem, SubElem, SuperElem, TextElem};
use typst::{Library, LibraryExt};

static BASE: LazyLock<Library> = LazyLock::new(Library::default);
pub static ROUTINES: LazyLock<Routines> = LazyLock::new(|| {
    let r = BASE.routines;
    Routines {
        rules: r.rules,
        eval_string: r.eval_string,
        eval_closure: r.eval_closure,
        realize,
        layout_frame: r.layout_frame,
        html_module: r.html_module,
        html_mathml_body: r.html_mathml_body,
        html_span_filled: r.html_span_filled,
    }
});

fn realize<'a>(
    kind: RealizationKind,
    engine: &mut Engine,
    locator: &mut SplitLocator,
    arenas: &'a Arenas,
    content: &'a Content,
    styles: StyleChain<'a>,
) -> SourceResult<Vec<Pair<'a>>> {
    let pairs = match kind {
        RealizationKind::Fragment { kind } => {
            let mut pairs = (BASE.routines.realize)(
                RealizationKind::Fragment { kind },
                engine,
                locator,
                arenas,
                content,
                styles,
            )?;
            if *kind == FragmentKind::Inline && !pairs.is_empty() {
                // Inline container contents have no `par` tag in Typst. Add
                // semantic boundaries without changing its inline layout.
                let first = pairs
                    .iter()
                    .position(|(c, _)| !c.is::<TagElem>())
                    .unwrap_or(0);
                let end = pairs
                    .iter()
                    .rposition(|(c, _)| !c.is::<TagElem>())
                    .map_or(first, |i| i + 1);
                let shared = StyleChain::trunk_from_pairs(&pairs[first..end]).unwrap_or(styles);
                let location = locator.next_location(engine, 0x545950505458, content.span());
                let mut par = ParElem::new(content.clone()).pack();
                par.set_location(location);
                let flags = TagFlags {
                    introspectable: false,
                    tagged: false,
                };
                pairs.insert(
                    0,
                    (
                        &*arenas
                            .content
                            .alloc(TagElem::packed(Tag::Start(par, flags))),
                        shared,
                    ),
                );
                pairs.push((
                    &*arenas
                        .content
                        .alloc(TagElem::packed(Tag::End(location, 0, flags))),
                    shared,
                ));
            }
            pairs
        }
        kind => (BASE.routines.realize)(kind, engine, locator, arenas, content, styles)?,
    };
    let mut out = Vec::with_capacity(pairs.len());
    // Typst's inline collector skips zero horizontal spaces before merging
    // text. Keeping a metadata boundary for them would change the shaping.
    let mut pairs = pairs
        .into_iter()
        .filter(|(c, _)| {
            !c.to_packed::<typst::layout::HElem>()
                .is_some_and(|h| h.amount.is_zero())
        })
        .peekable();
    let mut text_location = None;
    while let Some((content, styles)) = pairs.next() {
        if let Some(tag) = content.to_packed::<TagElem>()
            && let Tag::Start(par, _) = &tag.tag
            && (par.is::<ParElem>()
                || par.is::<EquationElem>()
                || par.is::<typst::layout::GridElem>()
                || par.is::<typst::model::TableElem>()
                || par.is::<typst::model::ListElem>()
                || par.is::<typst::model::EnumElem>()
                || par.is::<SuperElem>()
                || par.is::<SubElem>())
            && let Some(location) = par.location()
        {
            let alignment = match styles
                .get(AlignElem::alignment)
                .fix(styles.resolve(TextElem::dir))
                .x
            {
                FixedAlignment::Start => "l",
                FixedAlignment::Center => "ctr",
                FixedAlignment::End => "r",
            };
            let mut value: Dict = [
                ("typptx-source", location.into_value()),
                ("alignment", alignment.into_value()),
                (
                    "rtl",
                    (styles.resolve(TextElem::dir) == Dir::RTL).into_value(),
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();
            value.insert(
                "resolved".into(),
                par.clone().styled_with_map(styles.to_map()).into_value(),
            );
            if par.is::<EquationElem>() {
                value.insert(
                    "equation".into(),
                    par.clone().styled_with_map(styles.to_map()).into_value(),
                );
            }
            let mut marker = MetadataElem::new(value.into_value()).pack();
            let loc = location.variant(0x54595050);
            marker.set_location(loc);
            let flags = TagFlags {
                introspectable: false,
                tagged: false,
            };
            out.push((
                &*arenas
                    .content
                    .alloc(TagElem::packed(Tag::Start(marker, flags))),
                styles,
            ));
            out.push((
                &*arenas
                    .content
                    .alloc(TagElem::packed(Tag::End(loc, 0, flags))),
                styles,
            ));
        }
        if inline_text(content) {
            // Frame text retains the glyph advances but loses its style chain.
            // These non-rendering tags carry the resolved text settings. In
            // particular, tracking cannot be recovered from advances alone:
            // they also contain kerning, justification and font substitutions.
            // Tags interrupt Typst's adjacent-text merging. Wrap an entire
            // contiguous style segment, including spaces and smart quotes,
            // rather than inserting boundaries between its text elements.
            if text_location.is_none() {
                let mut kerning = styles.get(TextElem::kerning);
                for (tag, value) in styles.get_cloned(TextElem::features).0 {
                    if tag.to_bytes() == *b"kern" {
                        kerning = value != 0;
                    }
                }
                let value: Dict = [
                    ("typptx-kerning".into(), kerning.into_value()),
                    (
                        "typptx-tracking".into(),
                        styles.resolve(TextElem::tracking).to_pt().into_value(),
                    ),
                    (
                        "typptx-text-size".into(),
                        styles.resolve(TextElem::size).to_pt().into_value(),
                    ),
                ]
                .into_iter()
                .collect();
                let mut marker = MetadataElem::new(value.into_value()).pack();
                let loc = locator.next_location(engine, 0x4B45524E, content.span());
                marker.set_location(loc);
                let flags = TagFlags {
                    introspectable: false,
                    tagged: false,
                };
                out.push((
                    &*arenas
                        .content
                        .alloc(TagElem::packed(Tag::Start(marker, flags))),
                    styles,
                ));
                text_location = Some(loc);
            }
            out.push((content, styles));
            let flags = TagFlags {
                introspectable: false,
                tagged: false,
            };
            if content.is::<LinebreakElem>()
                || !pairs
                    .peek()
                    .is_some_and(|(next, next_styles)| inline_text(next) && *next_styles == styles)
            {
                out.push((
                    &*arenas.content.alloc(TagElem::packed(Tag::End(
                        text_location.take().unwrap(),
                        0,
                        flags,
                    ))),
                    styles,
                ));
            }
        } else {
            out.push((content, styles));
        }
        if content.is::<LinebreakElem>() || content.is::<typst::layout::HElem>() {
            // Keep the actual break after show rules and generated references
            // have been realized. Source-text matching cannot recover this.
            let key = if content.is::<LinebreakElem>() {
                "typptx-linebreak"
            } else {
                "typptx-hspace"
            };
            let value: Dict = [(key.into(), true.into_value())].into_iter().collect();
            let mut marker = MetadataElem::new(value.into_value()).pack();
            let loc = locator.next_location(engine, 0x425245414B, content.span());
            marker.set_location(loc);
            let flags = TagFlags {
                introspectable: false,
                tagged: false,
            };
            out.push((
                &*arenas
                    .content
                    .alloc(TagElem::packed(Tag::Start(marker, flags))),
                styles,
            ));
            out.push((
                &*arenas
                    .content
                    .alloc(TagElem::packed(Tag::End(loc, 0, flags))),
                styles,
            ));
        }
    }
    Ok(out)
}

/// The inline collector merges these elements into text before shaping. Its
/// other elements already introduce layout boundaries of their own.
fn inline_text(content: &Content) -> bool {
    content.is::<TextElem>()
        || content.is::<SpaceElem>()
        || content.is::<SmartQuoteElem>()
        || content.is::<LinebreakElem>()
}

/// A flat page frame no longer carries the inner content frame's size. Resolve
/// the page margins from the captured styles, just as Typst's page layout does.
pub fn page_area(
    content: &Content,
    bounds: crate::ir::Rect,
    page: usize,
) -> Option<crate::ir::Rect> {
    let styled = content.to_packed::<typst::foundations::StyledElem>()?;
    let styles = StyleChain::new(&styled.styles);
    let mut min = styles
        .resolve(PageElem::width)
        .unwrap_or(Abs::inf())
        .min(styles.resolve(PageElem::height).unwrap_or(Abs::inf()));
    if !min.to_pt().is_finite() {
        min = Abs::mm(210.0);
    }
    let default = Rel::<Length>::from((2.5 / 21.0) * min);
    let margins = styles.get(PageElem::margin).unwrap_or_default();
    let mut sides = margins
        .sides
        .map(|s| s.and_then(Smart::custom).unwrap_or(default))
        .resolve(styles)
        .relative_to(Size::new(Abs::pt(bounds.width), Abs::pt(bounds.height)));
    let binding = styles.get(PageElem::binding).unwrap_or_else(|| {
        if styles.resolve(TextElem::dir) == Dir::RTL {
            Binding::Right
        } else {
            Binding::Left
        }
    });
    if margins.two_sided.unwrap_or(false)
        && binding.swap(std::num::NonZeroUsize::new(page + 1).unwrap())
    {
        std::mem::swap(&mut sides.left, &mut sides.right);
    }
    Some(crate::ir::Rect {
        x: bounds.x + sides.left.to_pt(),
        y: bounds.y + sides.top.to_pt(),
        width: bounds.width - sides.left.to_pt() - sides.right.to_pt(),
        height: bounds.height - sides.top.to_pt() - sides.bottom.to_pt(),
    })
}
