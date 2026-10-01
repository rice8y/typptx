//! Retain compiler source spans until the host resolves them to file positions.
use crate::{compiler::capture::Capture, ir::Diagnostic};
use std::fmt;
use typst::{foundations::Content, layout::FrameItem, syntax::Span};

#[derive(Clone, Debug)]
pub(crate) struct Origin {
    pub span: Span,
    pub element: String,
}

impl Origin {
    pub fn content(content: &Content) -> Option<Self> {
        content.span().id()?;
        Some(Self {
            span: content.span(),
            element: content.elem().name().into(),
        })
    }

    pub fn leaf(capture: &Capture, page: usize, id: usize) -> Option<Self> {
        let leaf = &capture.pages[page][id];
        let (span, element) = match &leaf.item {
            FrameItem::Text(t) => (
                t.glyphs.iter().map(|g| g.span.0).find(|s| s.id().is_some()),
                "text",
            ),
            FrameItem::Image(_, _, span) => (Some(*span), "image"),
            FrameItem::Shape(_, span) => (Some(*span), "shape"),
            _ => (None, "object"),
        };
        if let Some(span) = span.filter(|s| s.id().is_some()) {
            return Some(Self {
                span,
                element: element.into(),
            });
        }
        leaf.ancestors
            .iter()
            .rev()
            .find_map(|&i| Self::content(&capture.nodes[i].content))
    }

    pub fn node(capture: &Capture, idx: usize, page: usize) -> Option<Self> {
        Self::content(&capture.nodes[idx].content).or_else(|| {
            capture.nodes[idx]
                .pages
                .get(&page)?
                .leaves
                .iter()
                .find_map(|&id| Self::leaf(capture, page, id))
                .map(|mut origin| {
                    origin.element = capture.nodes[idx].content.elem().name().into();
                    origin
                })
        })
    }
}

#[derive(Debug)]
struct LocatedError {
    origin: Origin,
    error: anyhow::Error,
}

impl fmt::Display for LocatedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for LocatedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}

fn error_origin(error: &anyhow::Error) -> Option<&Origin> {
    error
        .chain()
        .find_map(|e| e.downcast_ref::<LocatedError>().map(|e| &e.origin))
}

/// Keep the innermost failure's span as it propagates through a list or table.
pub(crate) fn at(error: anyhow::Error, origin: Option<Origin>) -> anyhow::Error {
    if error_origin(&error).is_some() {
        return error;
    }
    match origin {
        Some(origin) => LocatedError { origin, error }.into(),
        None => error,
    }
}

pub(crate) fn from_error(
    page: usize,
    source_id: String,
    code: &str,
    element: &str,
    error: anyhow::Error,
    fallback: Option<Origin>,
) -> Diagnostic {
    let origin = error_origin(&error).cloned().or(fallback);
    Diagnostic {
        page,
        source_id,
        code: code.into(),
        message: error.to_string(),
        element: Some(origin.as_ref().map_or(element, |o| &o.element).into()),
        source: None,
        span: origin.map(|o| o.span),
    }
}
