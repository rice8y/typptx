//! Portable presentation model. All geometry uses typographic points (1/72 inch).
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(self) -> f64 {
        self.y + self.height
    }
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            width: self.right().max(other.right()) - x,
            height: self.bottom().max(other.bottom()) - y,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextStyle {
    #[serde(default)]
    pub rtl: bool,
    pub font: String,
    /// DrawingML pitch/family hint, used when Office substitutes a missing font.
    #[serde(default)]
    pub pitch_family: u8,
    pub size: f64,
    /// Baseline shift in points; positive raises the text.
    #[serde(default)]
    pub baseline: f64,
    /// Whether the source enables font kerning.
    #[serde(default)]
    pub kerning: bool,
    /// Extra character spacing in points.
    #[serde(default)]
    pub letter_spacing: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// RGBA, including alpha. No theme-dependent colors.
    pub color: [u8; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Brush>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Stroke>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: TextStyle,
    pub hyperlink: Option<LinkTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub math: Option<MathExpr>,
    /// Keep an inline equation inline even when it occupies a whole paragraph.
    #[serde(default)]
    pub math_inline: bool,
    /// Unambiguous character advances from the source shaper, in em units.
    /// Empty for ligatures, reordered glyphs and other complex clusters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advances: Vec<TextAdvance>,
    /// Source visual line within the containing paragraph. This is layout
    /// evidence, not a request to insert a hard break into editable prose.
    #[serde(default)]
    pub source_line: usize,
    /// Realized horizontal advance in units of this run's font size. Kept even
    /// for clusters that must be shaped natively and for inline equations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_width: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextAdvance {
    pub character: char,
    pub nominal: f64,
    pub shaped: f64,
}

/// Equations remain structured Office Math rather than positioned glyphs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MathExpr {
    Text(String),
    /// An alignment/spacer marker within a multiline equation array.
    AlignPoint,
    /// Hidden content whose full mathematical dimensions still occupy space.
    Phantom {
        body: Box<MathExpr>,
    },
    Sized {
        body: Box<MathExpr>,
        scale: f64,
        /// Vertical offset as a fraction of the enclosing em.
        #[serde(default)]
        offset: f64,
    },
    Cancel {
        body: Box<MathExpr>,
        rising: bool,
        falling: bool,
        horizontal: bool,
        vertical: bool,
    },
    Sequence(Vec<MathExpr>),
    Rows(Vec<MathExpr>),
    Fraction {
        numerator: Box<MathExpr>,
        denominator: Box<MathExpr>,
        format: String,
    },
    Scripts {
        base: Box<MathExpr>,
        sub: Option<Box<MathExpr>>,
        sup: Option<Box<MathExpr>>,
    },
    PreScripts {
        base: Box<MathExpr>,
        sub: Option<Box<MathExpr>>,
        sup: Option<Box<MathExpr>>,
    },
    Bar {
        body: Box<MathExpr>,
        top: bool,
    },
    Group {
        body: Box<MathExpr>,
        character: String,
        top: bool,
    },
    Root {
        body: Box<MathExpr>,
        index: Option<Box<MathExpr>>,
    },
    Delimiter {
        open: String,
        close: String,
        body: Box<MathExpr>,
    },
    /// Delimited expressions separated by a shared, vertically stretching glyph.
    DelimitedParts {
        open: String,
        close: String,
        separator: String,
        parts: Vec<MathExpr>,
    },
    Matrix {
        rows: Vec<Vec<MathExpr>>,
        alignment: String,
        /// Column boundaries with native stretching vertical separators.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        separators: Vec<usize>,
    },
    Styled {
        body: Box<MathExpr>,
        style: MathStyle,
    },
    Limits {
        base: Box<MathExpr>,
        sub: Option<Box<MathExpr>>,
        sup: Option<Box<MathExpr>>,
    },
    /// A native Office n-ary operator. Its operand stays a sibling expression:
    /// Typst has no operand boundary for an operator, so we do not invent one.
    Operator {
        character: String,
        sub: Option<Box<MathExpr>>,
        sup: Option<Box<MathExpr>>,
        limits: bool,
    },
    Accent {
        character: String,
        body: Box<MathExpr>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MathStyle {
    pub bold: bool,
    pub italic: Option<bool>,
    pub variant: Option<String>,
    pub normal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Brush {
    Solid {
        color: [u8; 4],
    },
    Linear {
        stops: Vec<(f64, [u8; 4])>,
        angle: f64,
    },
    Radial {
        stops: Vec<(f64, [u8; 4])>,
        center: [f64; 2],
        radius: f64,
        focal: [f64; 2],
        inner_radius: f64,
        /// Maps gradient coordinates into normalized shape coordinates.
        transform: Box<[f64; 6]>,
    },
    Conic {
        stops: Vec<(f64, [u8; 4])>,
        center: [f64; 2],
        angle: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "points", rename_all = "snake_case")]
pub enum PathCommand {
    Move([f64; 2]),
    Line([f64; 2]),
    Cubic([[f64; 2]; 3]),
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stroke {
    pub paint: Brush,
    pub width: f64,
    pub cap: String,
    pub join: String,
    pub dash: Vec<f64>,
    pub miter_limit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorShape {
    pub bounds: Rect,
    pub commands: Vec<PathCommand>,
    pub fill: Option<Brush>,
    pub stroke: Option<Stroke>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Bullet {
    Picture {
        size: f64,
        extension: String,
        #[serde(skip)]
        bytes: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        svg: Option<String>,
    },
    Character {
        character: String,
        font: String,
        color: [u8; 4],
        size: f64,
    },
    Number {
        scheme: String,
        start: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Paragraph {
    #[serde(default)]
    pub rtl: bool,
    #[serde(default)]
    pub margin_right: f64,
    pub runs: Vec<Run>,
    /// Fixed source lines around independently positioned SVG equations.
    /// Empty for ordinary, reflowable native paragraphs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<ParagraphLine>,
    pub level: u8,
    pub bullet: Option<Bullet>,
    /// Position of text and marker relative to the text box's left edge.
    pub margin_left: f64,
    pub indent: f64,
    pub alignment: String,
    pub line_spacing: f64,
    pub space_before: f64,
    pub space_after: f64,
    #[serde(default)]
    pub tab_stops: Vec<f64>,
    #[serde(default)]
    pub break_latin: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ParagraphLine {
    /// Realized start of the line in slide coordinates.
    pub x: f64,
    #[serde(default)]
    pub right: f64,
    pub tab_stops: Vec<f64>,
    /// Distance from the preceding baseline; the first line uses paragraph spacing.
    pub spacing: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextBlock {
    /// Mirror the glyphs as well as their enclosing horizontal reflection.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirror_x: bool,
    /// DrawingML text orientation; absent for ordinary horizontal paragraphs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical: Option<String>,
    /// Rectangular clipping in slide coordinates; text and its layout stay intact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<Rect>,
    /// Native text fitting for bounded code blocks (1.0 keeps the source size).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_scale: Option<f64>,
    pub source_id: String,
    pub role: String,
    pub bounds: Rect,
    pub paragraphs: Vec<Paragraph>,
    pub wrap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub wrap: bool,
    pub row: usize,
    pub column: usize,
    pub row_span: usize,
    pub column_span: usize,
    pub paragraphs: Vec<Paragraph>,
    pub inset: [f64; 4],
    /// DrawingML uses full font line metrics; Typst commonly uses cap height.
    pub text_inset: [f64; 4],
    pub fill: Option<Brush>,
    pub vertical_alignment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Table {
    pub source_id: String,
    pub bounds: Rect,
    pub column_widths: Vec<f64>,
    pub row_heights: Vec<f64>,
    pub cells: Vec<TableCell>,
    /// Resolved edges, including the outer border and segments beside merges.
    pub horizontal_borders: Vec<Vec<Option<Stroke>>>,
    pub vertical_borders: Vec<Vec<Option<Stroke>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Element {
    /// A hyperlink owned by its object, so moving or deleting the object also
    /// moves or removes its click target. This adds no PowerPoint shape.
    Linked {
        target: LinkTarget,
        element: Box<Element>,
    },
    /// A native PowerPoint group. Child objects remain independently editable.
    Group(ObjectGroup),
    Text(TextBlock),
    Table(Table),
    Shape(VectorShape),
    /// Each imported asset stays one PowerPoint picture. Vector assets carry
    /// SVG with a PNG preview for Office versions without SVG support.
    Picture {
        bounds: Rect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clip: Option<Vec<PathCommand>>,
        extension: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        svg: Option<String>,
        #[serde(skip)]
        bytes: Vec<u8>,
    },
    /// A contiguous section of the original display list. PNG is the fallback
    /// for Office versions without SVG support; both have identical geometry.
    Drawing {
        bounds: Rect,
        svg: String,
        #[serde(skip)]
        png: Vec<u8>,
    },
    /// An explicitly requested Typst equation image, not an unsupported fallback.
    MathSvg {
        source_id: String,
        bounds: Rect,
        svg: String,
        #[serde(skip)]
        png: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectGroup {
    pub bounds: Rect,
    pub content_bounds: Rect,
    pub rotation: f64,
    #[serde(default = "full_opacity")]
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
    pub flip_x: bool,
    pub flip_y: bool,
    pub elements: Vec<Element>,
}

impl Element {
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Linked { element, .. } => element.bounds(),
            Self::Group(g) => g.bounds,
            Self::Text(t) => t.bounds,
            Self::Table(t) => t.bounds,
            Self::Shape(s) => s.bounds,
            Self::Picture { bounds, .. }
            | Self::Drawing { bounds, .. }
            | Self::MathSvg { bounds, .. } => *bounds,
        }
    }

    pub fn group(elements: Vec<Self>) -> Self {
        let bounds = elements
            .iter()
            .map(Self::bounds)
            .reduce(Rect::union)
            .unwrap_or_default();
        Self::Group(ObjectGroup {
            bounds,
            content_bounds: bounds,
            rotation: 0.,
            opacity: 1.,
            effect: None,
            flip_x: false,
            flip_y: false,
            elements,
        })
    }

    pub fn walk(&self) -> impl Iterator<Item = &Self> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            loop {
                let next = stack.pop()?;
                if let Self::Linked { element, .. } = next {
                    stack.push(element);
                    continue;
                }
                if let Self::Group(g) = next {
                    stack.extend(g.elements.iter().rev());
                }
                return Some(next);
            }
        })
    }
}

/// Native effects keep the underlying shapes and paragraphs editable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Blur {
        radius: f64,
    },
    Shadow {
        radius: f64,
        offset: [f64; 2],
        color: [u8; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    pub width: f64,
    pub height: f64,
    pub background: Option<[u8; 4]>,
    pub elements: Vec<Element>,
    /// Plain text in the editable PowerPoint speaker notes body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Independent clickable regions with no drawable source object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<SlideLink>,
    /// Ordered click states. Indices refer to this slide's top-level elements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<Animation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Animation {
    /// One-based source pages, including the initially visible state.
    pub source_pages: Vec<usize>,
    pub steps: Vec<AnimationStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationStep {
    /// Elements visible at this step, in their original stacking order.
    pub visible: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideLink {
    pub target: LinkTarget,
    pub region: VectorShape,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum LinkTarget {
    /// One-based destination slide number.
    Slide(usize),
    Url(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// Absolute project path, or a package-qualified path such as @preview/pkg:1.0.0/file.typ.
    pub file: String,
    /// One-based lines and Unicode character columns; the end is exclusive.
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub page: usize,
    pub source_id: String,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLocation>,
    /// Resolved by the compiler host before serializing the report.
    #[serde(skip)]
    pub(crate) span: Option<typst::syntax::Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presentation {
    pub schema_version: u32,
    pub slides: Vec<Slide>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub fonts: Vec<EmbeddedFont>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedFont {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// Complete font, so newly typed characters remain available.
    #[serde(skip)]
    pub data: Vec<u8>,
}

fn full_opacity() -> f64 {
    1.
}
