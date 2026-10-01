//! Translate evaluated Typst math content to a typed Office Math tree.
pub(crate) mod svg;

use crate::ir::{MathExpr as M, MathStyle};
use anyhow::{Result, anyhow, bail, ensure};
use codex::styling::MathVariant;
use typst::foundations::{Content, SequenceElem, StyleChain, StyledElem, Value};
use typst::layout::{HAlignment, HElem, Spacing};
use typst::math::{
    AccentElem, AttachElem, BinomElem, CasesElem, DelimiterPair, EquationElem, FracElem, FracStyle,
    LimitsElem, MatElem, OpElem, VecElem,
};
use typst::text::TextElem;

fn field(c: &Content, name: &str) -> Option<Content> {
    match c.field_by_name(name).ok()? {
        Value::Content(c) => Some(c),
        _ => None,
    }
}
fn required(c: &Content, name: &str, styles: StyleChain, block: bool) -> Result<M> {
    convert_inner(
        &field(c, name).ok_or_else(|| anyhow!("missing math field {name}"))?,
        styles,
        block,
    )
}
fn optional(c: &Content, name: &str, styles: StyleChain, block: bool) -> Result<Option<Box<M>>> {
    field(c, name)
        .as_ref()
        .map(|c| convert_inner(c, styles, block))
        .transpose()
        .map(|v| v.map(Box::new))
}
fn text(c: &Content) -> Option<String> {
    match c.field_by_name("text").ok()? {
        Value::Str(t) => Some(t.to_string()),
        _ => None,
    }
}

pub fn convert(c: &Content) -> Result<M> {
    convert_inner(c, StyleChain::default(), false)
}

pub(crate) fn inline(c: &Content) -> bool {
    fn resolve(c: &Content, styles: StyleChain) -> bool {
        if let Some(styled) = c.to_packed::<StyledElem>() {
            resolve(&styled.child, styles.chain(&styled.styles))
        } else {
            c.to_packed::<EquationElem>()
                .is_some_and(|eq| !eq.block.get(styles))
        }
    }
    resolve(c, StyleChain::default())
}

pub(crate) fn single_fraction(c: &Content) -> bool {
    if let Some(styled) = c.to_packed::<StyledElem>() {
        single_fraction(&styled.child)
    } else if let Some(eq) = c.to_packed::<EquationElem>() {
        single_fraction(&eq.body)
    } else if let Some(seq) = c.to_packed::<SequenceElem>() {
        let mut children = seq.children.iter().filter(|c| c.elem().name() != "space");
        children.next().is_some_and(single_fraction) && children.next().is_none()
    } else {
        c.is::<FracElem>()
    }
}

fn convert_inner(c: &Content, styles: StyleChain, block: bool) -> Result<M> {
    convert_element(c, styles, block).map_err(|error| {
        crate::compiler::diagnostics::at(error, crate::compiler::diagnostics::Origin::content(c))
    })
}

fn convert_element(c: &Content, styles: StyleChain, block: bool) -> Result<M> {
    if let Some(styled) = c.to_packed::<StyledElem>() {
        return convert_inner(&styled.child, styles.chain(&styled.styles), block);
    }
    if let Some(eq) = c.to_packed::<EquationElem>() {
        return convert_inner(&eq.body, styles, eq.block.get(styles));
    }
    if let Some(seq) = c.to_packed::<SequenceElem>() {
        let mut rows = vec![Vec::new()];
        for child in &seq.children {
            if child.elem().name() == "linebreak" {
                rows.push(Vec::new());
            } else {
                rows.last_mut()
                    .unwrap()
                    .push(convert_inner(child, styles, block)?);
            }
        }
        while rows.last().is_some_and(|r| r.iter().all(empty)) {
            rows.pop();
        }
        return Ok(if rows.len() > 1 {
            M::Rows(rows.into_iter().map(M::Sequence).collect())
        } else {
            M::Sequence(rows.pop().unwrap_or_default())
        });
    }
    if let Some(matrix) = c.to_packed::<MatElem>() {
        let rows: Vec<Vec<M>> = matrix
            .rows
            .iter()
            .map(|r| r.iter().map(|c| convert_inner(c, styles, block)).collect())
            .collect::<Result<_>>()?;
        let mut separators = Vec::new();
        if let Some(augment) = matrix.augment.get_cloned(styles) {
            let columns = rows.first().map_or(0, Vec::len) as isize;
            let custom = !augment.hline.0.is_empty()
                || !augment.stroke.is_auto()
                || augment.vline.0.iter().any(|&offset| {
                    let boundary = if offset < 0 { columns + offset } else { offset };
                    boundary == 0 || boundary == columns
                });
            ensure!(
                !custom,
                "horizontal, styled, or outer matrix rules need layout information from Office; source-positioned lines can overlap the reflowed equation; use --math-format svg"
            );
            if !augment.vline.0.is_empty() {
                // The writer aligns matrix segments with zero-width phantoms
                // carrying the complete source row's ascent and descent.
                for &offset in &augment.vline.0 {
                    let boundary = if offset < 0 { columns + offset } else { offset };
                    ensure!(
                        boundary > 0 && boundary < columns,
                        "invalid matrix augmentation offset"
                    );
                    separators.push(boundary as usize);
                }
                separators.sort_unstable();
                separators.dedup();
            }
        }
        return Ok(delimited(
            matrix.delim.get(styles),
            M::Matrix {
                rows,
                alignment: alignment(matrix.align.get(styles)).into(),
                separators,
            },
        ));
    }
    if let Some(vector) = c.to_packed::<VecElem>() {
        let rows = vector
            .children
            .iter()
            .map(|c| Ok(vec![convert_inner(c, styles, block)?]))
            .collect::<Result<_>>()?;
        return Ok(delimited(
            vector.delim.get(styles),
            M::Matrix {
                rows,
                alignment: alignment(vector.align.get(styles)).into(),
                separators: vec![],
            },
        ));
    }
    if let Some(cases) = c.to_packed::<CasesElem>() {
        let body = M::Matrix {
            rows: cases
                .children
                .iter()
                .map(|c| Ok(vec![convert_inner(c, styles, block)?]))
                .collect::<Result<_>>()?,
            alignment: "left".into(),
            separators: vec![],
        };
        let delim = cases.delim.get(styles);
        return Ok(M::Delimiter {
            open: if cases.reverse.get(styles) {
                String::new()
            } else {
                delim.open().map(String::from).unwrap_or_default()
            },
            close: if cases.reverse.get(styles) {
                delim.close().map(String::from).unwrap_or_default()
            } else {
                String::new()
            },
            body: Box::new(body),
        });
    }
    if let Some(accent) = c.to_packed::<AccentElem>() {
        return Ok(M::Accent {
            character: accent.accent.0.to_string(),
            body: Box::new(convert_inner(&accent.base, styles, block)?),
        });
    }
    if let Some(binom) = c.to_packed::<BinomElem>() {
        let mut lower = Vec::new();
        for item in &binom.lower {
            if !lower.is_empty() {
                lower.push(M::Text(",".into()));
            }
            lower.push(convert_inner(item, styles, block)?);
        }
        return Ok(M::Delimiter {
            open: "(".into(),
            close: ")".into(),
            body: Box::new(M::Fraction {
                numerator: Box::new(convert_inner(&binom.upper, styles, block)?),
                denominator: Box::new(M::Sequence(lower)),
                format: "noBar".into(),
            }),
        });
    }
    if let Some(frac) = c.to_packed::<FracElem>() {
        let format = match frac.style.get(styles) {
            FracStyle::Vertical => "bar",
            FracStyle::Skewed => "skw",
            FracStyle::Horizontal => "lin",
        };
        let part = |c: &Content, parentheses: bool| -> Result<Box<M>> {
            let body = convert_inner(c, styles, block)?;
            Ok(Box::new(if format == "lin" && parentheses {
                M::Delimiter {
                    open: "(".into(),
                    close: ")".into(),
                    body: Box::new(body),
                }
            } else {
                body
            }))
        };
        return Ok(M::Fraction {
            numerator: part(&frac.num, frac.num_deparenthesized.get(styles))?,
            denominator: part(&frac.denom, frac.denom_deparenthesized.get(styles))?,
            format: format.into(),
        });
    }
    if let Some((character, limits)) = operator(c, styles, block) {
        return Ok(M::Operator {
            character,
            sub: None,
            sup: None,
            limits,
        });
    }
    Ok(match c.elem().name() {
        "text" | "symbol" => {
            let text = text(c).ok_or_else(|| anyhow!("unresolved math text"))?;
            let normal = text.chars().count() > 1;
            styled_text(text, styles, normal)?
        }
        "space" => M::Text(" ".into()),
        "h" => {
            let spacing = c.to_packed::<HElem>().unwrap();
            let Spacing::Rel(amount) = spacing.amount else {
                bail!("fractional math spacing is not supported")
            };
            ensure!(
                amount.rel.get() == 0.0,
                "relative math spacing is not supported"
            );
            let size = styles.resolve(TextElem::size);
            let em = amount.abs.at(size).to_pt() / size.to_pt();
            ensure!(
                (0.0..=32.0).contains(&em),
                "negative or excessive math spacing is not supported"
            );
            // Office Math has no arbitrary kern. Use standard Unicode math
            // spaces, quantized to one sixth of an em.
            let sixths = (em * 6.0).round() as usize;
            M::Text(format!(
                "{}{}",
                "\u{2003}".repeat(sixths / 6),
                "\u{2006}".repeat(sixths % 6)
            ))
        }
        "align-point" => M::AlignPoint,
        "hide" => M::Phantom {
            body: Box::new(required(c, "body", styles, block)?),
        },
        "tag" | "metadata" | "counter-update" | "linebreak" => M::Sequence(vec![]),
        "class" => required(c, "body", styles, block)?,
        "op" => {
            let op = c.to_packed::<OpElem>().unwrap();
            styled_text(op.text.plain_text().to_string(), styles, true)?
        }
        "limits" | "scripts" => required(c, "body", styles, block)?,
        "attach" => attachments(c.to_packed::<AttachElem>().unwrap(), styles, block)?,
        "underline" | "overline" => M::Bar {
            body: Box::new(required(c, "body", styles, block)?),
            top: c.elem().name() == "overline",
        },
        "underbrace" | "overbrace" | "underbracket" | "overbracket" | "underparen"
        | "overparen" | "overshell" => {
            let name = c.elem().name();
            macro_rules! annotation {
                ($element:ident) => {
                    c.to_packed::<typst::math::$element>()
                        .unwrap()
                        .annotation
                        .get_cloned(styles)
                };
            }
            let (character, annotation) = match name {
                "underbrace" => ('⏟', annotation!(UnderbraceElem)),
                "overbrace" => ('⏞', annotation!(OverbraceElem)),
                "underbracket" => ('⎵', annotation!(UnderbracketElem)),
                "overbracket" => ('⎴', annotation!(OverbracketElem)),
                "underparen" => ('⏝', annotation!(UnderparenElem)),
                "overparen" => ('⏜', annotation!(OverparenElem)),
                "overshell" => ('⏠', annotation!(OvershellElem)),
                _ => unreachable!(),
            };
            let top = name.starts_with("over");
            let body = M::Group {
                body: Box::new(required(c, "body", styles, block)?),
                character: character.into(),
                top,
            };
            if let Some(annotation) = annotation {
                let annotation = Box::new(convert_inner(&annotation, styles, block)?);
                if top {
                    M::Limits {
                        base: Box::new(body),
                        sub: None,
                        sup: Some(annotation),
                    }
                } else {
                    // Equation-array rows measure their own height. This avoids
                    // PowerPoint's overlapping limit below a grouping glyph.
                    M::Rows(vec![
                        body,
                        M::Sized {
                            body: annotation,
                            scale: 0.7,
                            offset: -0.16,
                        },
                    ])
                }
            } else {
                body
            }
        }
        "cancel" => {
            use typst::foundations::Smart;
            use typst::math::{CancelAngle, CancelElem};
            let cancel = c.to_packed::<CancelElem>().unwrap();
            let default_length = typst::layout::Rel::new(
                typst::layout::Ratio::one(),
                typst::layout::Em::new(0.3).into(),
            );
            let default_stroke = typst::visualize::Stroke {
                thickness: Smart::Custom(typst::layout::Em::new(0.05).into()),
                ..Default::default()
            };
            let custom_style = cancel.length.get(styles) != default_length
                || cancel.stroke.get_cloned(styles) != default_stroke;
            let angle = cancel.angle.get_cloned(styles);
            let (horizontal, vertical) = match angle {
                Smart::Auto => (false, false),
                Smart::Custom(CancelAngle::Angle(a))
                    if a.to_deg().rem_euclid(180.).abs() < 0.001 =>
                {
                    (false, true)
                }
                Smart::Custom(CancelAngle::Angle(a))
                    if (a.to_deg().rem_euclid(180.) - 90.).abs() < 0.001 =>
                {
                    (true, false)
                }
                _ => bail!(
                    "custom cancellation angles need layout information from Office; use --math-format svg"
                ),
            };
            ensure!(
                !custom_style,
                "custom cancellation stroke or length needs layout information from Office; source-positioned lines can miss the reflowed equation; use --math-format svg"
            );
            let cross = cancel.cross.get(styles);
            let inverted = cancel.inverted.get(styles);
            M::Cancel {
                body: Box::new(convert_inner(&cancel.body, styles, block)?),
                rising: !horizontal && !vertical && (!inverted || cross),
                falling: !horizontal && !vertical && (inverted || cross),
                horizontal,
                vertical,
            }
        }
        "root" => M::Root {
            body: Box::new(required(c, "radicand", styles, block)?),
            index: optional(c, "index", styles, block)?,
        },
        "lr" => left_right(c, styles, block)?,
        // Without a surrounding lr(), Typst leaves the glyph at its own size.
        "mid" => required(c, "body", styles, block)?,
        "primes" => {
            let count = c
                .field_by_name("count")
                .ok()
                .and_then(|v| v.cast::<usize>().ok())
                .unwrap_or(1);
            M::Text("′".repeat(count))
        }
        other => bail!("unsupported native math element {other}"),
    })
}

fn left_right(c: &Content, styles: StyleChain, block: bool) -> Result<M> {
    fn has_middle(c: &Content) -> bool {
        if let Some(styled) = c.to_packed::<StyledElem>() {
            has_middle(&styled.child)
        } else if let Some(sequence) = c.to_packed::<SequenceElem>() {
            sequence.children.iter().any(has_middle)
        } else {
            c.elem().name() == "mid"
        }
    }
    let body = field(c, "body").ok_or_else(|| anyhow!("missing math field body"))?;
    if !has_middle(&body) {
        let mut body = convert_inner(&body, styles, block)?;
        if let M::Sequence(ref mut sequence) = body
            && sequence.len() >= 2
            && let (Some(open), Some(close)) = (
                single_text(&sequence[0]),
                single_text(sequence.last().unwrap()),
            )
        {
            let (open, close) = (open.into(), close.into());
            sequence.pop();
            sequence.remove(0);
            return Ok(M::Delimiter {
                open,
                close,
                body: Box::new(body),
            });
        }
        return Ok(body);
    }
    enum Part {
        Body(M),
        Separator(String),
    }
    fn collect(c: &Content, styles: StyleChain, block: bool, out: &mut Vec<Part>) -> Result<()> {
        if let Some(styled) = c.to_packed::<StyledElem>() {
            return collect(&styled.child, styles.chain(&styled.styles), block, out);
        }
        if let Some(sequence) = c.to_packed::<SequenceElem>() {
            for child in &sequence.children {
                collect(child, styles, block, out)?;
            }
        } else if c.elem().name() == "mid" {
            let body = required(c, "body", styles, block)?;
            let separator = single_text(&body)
                .filter(|s| s.chars().count() == 1)
                .ok_or_else(|| anyhow!("a native middle delimiter requires one character"))?;
            out.push(Part::Separator(separator.into()));
        } else {
            ensure!(
                c.elem().name() != "linebreak",
                "multiline middle delimiters require --math-format svg"
            );
            // Nested lr() groups own their middle delimiters.
            out.push(Part::Body(convert_inner(c, styles, block)?));
        }
        Ok(())
    }
    let mut items = Vec::new();
    collect(&body, styles, block, &mut items)?;
    let edge = |part: &Part| match part {
        Part::Body(m) => single_text(m).map(str::to_owned),
        _ => None,
    };
    let (open, close) = if items.len() >= 2
        && let (Some(open), Some(close)) = (edge(&items[0]), edge(items.last().unwrap()))
    {
        items.pop();
        items.remove(0);
        (open, close)
    } else {
        (String::new(), String::new())
    };
    let mut separator = None;
    let mut parts = vec![Vec::new()];
    for item in items {
        match item {
            Part::Body(body) => parts.last_mut().unwrap().push(body),
            Part::Separator(value) => {
                ensure!(
                    separator.as_ref().is_none_or(|s| s == &value),
                    "different middle delimiters in one group require --math-format svg"
                );
                separator = Some(value);
                parts.push(Vec::new());
            }
        }
    }
    if let Some(separator) = separator {
        Ok(M::DelimitedParts {
            open,
            close,
            separator,
            parts: parts.into_iter().map(M::Sequence).collect(),
        })
    } else {
        let body = M::Sequence(parts.pop().unwrap());
        Ok(if open.is_empty() && close.is_empty() {
            body
        } else {
            M::Delimiter {
                open,
                close,
                body: Box::new(body),
            }
        })
    }
}

fn uses_limits(c: &Content, styles: StyleChain, block: bool) -> bool {
    if let Some(styled) = c.to_packed::<StyledElem>() {
        return uses_limits(&styled.child, styles.chain(&styled.styles), block);
    }
    if let Some(eq) = c.to_packed::<EquationElem>() {
        return uses_limits(&eq.body, styles, block);
    }
    if let Some(limits) = c.to_packed::<LimitsElem>() {
        return block || limits.inline.get(styles);
    }
    if c.elem().name() == "scripts" {
        return false;
    }
    if let Some(op) = c.to_packed::<OpElem>() {
        return block && op.limits.get(styles);
    }
    if let Some((_, limits)) = operator(c, styles, block) {
        return limits;
    }
    text(c).is_some_and(|t| {
        t.chars().count() == 1
            && typst::math::Limits::for_char(t.chars().next().unwrap()).active(styles)
    })
}

fn attachments(
    c: &typst::foundations::Packed<AttachElem>,
    styles: StyleChain,
    block: bool,
) -> Result<M> {
    let convert = |c: Option<Content>| {
        c.as_ref()
            .map(|c| convert_inner(c, styles, block).map(Box::new))
            .transpose()
    };
    let mut top = convert(c.t.get_cloned(styles))?;
    let mut bottom = convert(c.b.get_cloned(styles))?;
    let left_top = convert(c.tl.get_cloned(styles))?;
    let left_bottom = convert(c.bl.get_cloned(styles))?;
    let mut right_top = convert(c.tr.get_cloned(styles))?;
    let mut right_bottom = convert(c.br.get_cloned(styles))?;
    let limits = uses_limits(&c.base, styles, block);
    if !limits {
        if right_top.is_none() {
            right_top = top.take();
        }
        if right_bottom.is_none() {
            right_bottom = bottom.take();
        }
    }
    // Explicit corner attachments coexist with centered top/bottom limits.
    let mut base = if let Some((character, _)) = operator(&c.base, styles, block) {
        M::Operator {
            character,
            sub: bottom,
            sup: top,
            limits: true,
        }
    } else {
        let base = convert_inner(&c.base, styles, block)?;
        if top.is_some() || bottom.is_some() {
            M::Limits {
                base: Box::new(base),
                sub: bottom,
                sup: top,
            }
        } else {
            base
        }
    };
    if left_top.is_some() || left_bottom.is_some() {
        base = M::PreScripts {
            base: Box::new(base),
            sub: left_bottom,
            sup: left_top,
        };
    }
    if right_top.is_some() || right_bottom.is_some() {
        // Keep native n-ary side limits when no other attachments intervene.
        if let M::Operator {
            sub: None,
            sup: None,
            ..
        } = &base
            && let M::Operator { character, .. } = base
        {
            base = M::Operator {
                character,
                sub: right_bottom,
                sup: right_top,
                limits: false,
            };
        } else {
            base = M::Scripts {
                base: Box::new(base),
                sub: right_bottom,
                sup: right_top,
            };
        }
    }
    Ok(base)
}

fn alignment(a: HAlignment) -> &'static str {
    match a {
        HAlignment::Left | HAlignment::Start => "left",
        HAlignment::Center => "center",
        HAlignment::Right | HAlignment::End => "right",
    }
}
pub(crate) fn uniform_matrix_cell(m: &M) -> bool {
    match m {
        M::Text(_) => true,
        M::Sequence(seq) => seq.iter().all(uniform_matrix_cell),
        M::Styled { body, .. } => uniform_matrix_cell(body),
        _ => false,
    }
}
fn delimited(delim: DelimiterPair, body: M) -> M {
    M::Delimiter {
        open: delim.open().map(String::from).unwrap_or_default(),
        close: delim.close().map(String::from).unwrap_or_default(),
        body: Box::new(body),
    }
}
fn operator(c: &Content, styles: StyleChain, block: bool) -> Option<(String, bool)> {
    if let Some(s) = c.to_packed::<StyledElem>() {
        return operator(&s.child, styles.chain(&s.styles), block);
    }
    if let Some(limits) = c.to_packed::<LimitsElem>() {
        return operator(&limits.body, styles, block)
            .map(|(c, _)| (c, block || limits.inline.get(styles)));
    }
    if c.elem().name() == "scripts" {
        return operator(&field(c, "body")?, styles, block).map(|(c, _)| (c, false));
    }
    let text = text(c)?;
    let character = text.chars().next()?;
    if text.chars().count() != 1
        || !matches!(
            character,
            '∑' | '∏'
                | '∐'
                | '∫'
                | '∬'
                | '∭'
                | '∮'
                | '∯'
                | '∰'
                | '⋀'
                | '⋁'
                | '⋂'
                | '⋃'
                | '⨀'
                | '⨁'
                | '⨂'
        )
    {
        return None;
    }
    Some((
        text,
        block && !matches!(character, '∫' | '∬' | '∭' | '∮' | '∯' | '∰'),
    ))
}
fn styled_text(text: String, styles: StyleChain, normal: bool) -> Result<M> {
    let variant = styles
        .get(EquationElem::variant)
        .map(|variant| {
            Ok(match variant {
                MathVariant::Plain => "roman",
                MathVariant::Fraktur => "fraktur",
                MathVariant::SansSerif => "sans-serif",
                MathVariant::Monospace => "monospace",
                MathVariant::DoubleStruck => "double-struck",
                MathVariant::Chancery | MathVariant::Roundhand => "script",
                _ => bail!("unsupported mathematical alphabet"),
            }
            .to_owned())
        })
        .transpose()?;
    let style = MathStyle {
        bold: styles.get(EquationElem::bold),
        italic: if normal {
            Some(false)
        } else {
            styles.get(EquationElem::italic)
        },
        variant,
        normal,
    };
    let body = M::Text(text);
    Ok(if style == MathStyle::default() {
        body
    } else {
        M::Styled {
            body: Box::new(body),
            style,
        }
    })
}
fn single_text(m: &M) -> Option<&str> {
    match m {
        M::Text(t) => Some(t),
        M::Styled { body, .. } => single_text(body),
        _ => None,
    }
}
pub fn empty(m: &M) -> bool {
    match m {
        M::AlignPoint => true,
        M::Text(t) => t.trim().is_empty(),
        M::Sequence(s) | M::Rows(s) => s.iter().all(empty),
        M::Styled { body, .. } => empty(body),
        _ => false,
    }
}
pub fn plain(m: &M) -> String {
    match m {
        M::AlignPoint | M::Phantom { .. } => String::new(),
        M::Text(t) => t.clone(),
        M::Sequence(s) => s.iter().map(plain).collect(),
        M::Rows(s) => s.iter().map(plain).collect::<Vec<_>>().join("\n"),
        M::Fraction {
            numerator,
            denominator,
            format,
        } => {
            if format == "noBar" {
                format!("binom({},{})", plain(numerator), plain(denominator))
            } else {
                format!("({})/({})", plain(numerator), plain(denominator))
            }
        }
        M::Scripts { base, sub, sup } | M::Limits { base, sub, sup } => {
            format!("{}{}", plain(base), plain_scripts(sub, sup))
        }
        M::PreScripts { base, sub, sup } => format!("{}{}", plain_scripts(sub, sup), plain(base)),
        M::Operator {
            character,
            sub,
            sup,
            ..
        } => format!("{character}{}", plain_scripts(sub, sup)),
        M::Root { body, .. } => format!("√({})", plain(body)),
        M::Delimiter { open, close, body } => format!("{open}{}{close}", plain(body)),
        M::DelimitedParts {
            open,
            close,
            separator,
            parts,
        } => {
            format!(
                "{open}{}{close}",
                parts.iter().map(plain).collect::<Vec<_>>().join(separator)
            )
        }
        M::Matrix { rows, .. } => rows
            .iter()
            .map(|r| r.iter().map(plain).collect::<Vec<_>>().join(","))
            .collect::<Vec<_>>()
            .join(";"),
        M::Sized { body, .. }
        | M::Cancel { body, .. }
        | M::Accent { body, .. }
        | M::Styled { body, .. }
        | M::Bar { body, .. }
        | M::Group { body, .. } => plain(body),
    }
}
fn plain_scripts(sub: &Option<Box<M>>, sup: &Option<Box<M>>) -> String {
    format!(
        "{}{}",
        sub.as_ref()
            .map(|s| format!("_{{{}}}", plain(s)))
            .unwrap_or_default(),
        sup.as_ref()
            .map(|s| format!("^{{{}}}", plain(s)))
            .unwrap_or_default()
    )
}
