use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use typptx::{compiler::world::CompilerWorld, lower, pptx};

#[derive(Parser)]
#[command(version, about = "Compile Typst to structurally editable PowerPoint")]
struct Args {
    /// Typst source file. Each compiled page becomes one slide.
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Typst project root (defaults to the input's directory).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Additional font directories, in priority order.
    #[arg(long)]
    font_path: Vec<PathBuf>,
    /// Set a Typst sys.inputs value (KEY=VALUE).
    #[arg(long="input",value_parser=parse_input)]
    inputs: Vec<(String, String)>,
    /// Write the intermediate representation as JSON for inspection.
    #[arg(long)]
    dump_ir: Option<PathBuf>,
    /// Write a JSON conversion report.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Require a report without diagnostics (native export is already the default).
    #[arg(long)]
    strict: bool,
    /// Explicitly permit SVG/PNG fallback for unsupported blocks (loses editability).
    #[arg(long, conflicts_with = "strict")]
    allow_image_fallback: bool,
    /// Limit embedded PNG/JPEG resolution and render fallback PNGs at this DPI (no upscaling).
    #[arg(long, value_name = "DPI", value_parser = clap::value_parser!(u32).range(1..))]
    image_dpi: Option<u32>,
    /// Export equations as editable Office Math or as Typst-rendered SVG pictures.
    #[arg(long, value_enum, default_value_t = lower::MathFormat::Office)]
    math_format: lower::MathFormat,
    /// Export the original Typst pages as PNGs for visual comparison.
    #[arg(long)]
    reference_dir: Option<PathBuf>,
}

fn parse_input(value: &str) -> std::result::Result<(String, String), String> {
    value
        .split_once('=')
        .filter(|(key, _)| !key.is_empty())
        .map(|(k, v)| (k.into(), v.into()))
        .ok_or_else(|| "expected KEY=VALUE".into())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let output = args
        .output
        .unwrap_or_else(|| args.input.with_extension("pptx"));
    let mut destinations = vec![output.clone()];
    destinations.extend(args.report.iter().chain(args.dump_ir.iter()).cloned());
    validate_destinations(&args.input, &destinations)?;
    let world = CompilerWorld::new(
        &args.input,
        args.root.as_deref(),
        &args.font_path,
        &args.inputs,
    )?;
    let (document, warnings) = world.compile()?;
    for warning in &warnings {
        eprintln!("Typst warning: {warning}");
    }
    let mut presentation = lower::convert_with_options(
        &document,
        &lower::Options {
            allow_image_fallback: args.allow_image_fallback,
            image_dpi: args.image_dpi,
            math_format: args.math_format,
        },
    )?;
    world.locate_diagnostics(&mut presentation);
    if let Some(dir) = &args.reference_dir {
        destinations
            .extend((0..document.pages().len()).map(|i| dir.join(format!("page-{}.png", i + 1))));
        validate_destinations(&args.input, &destinations)?;
    }
    for d in &presentation.diagnostics {
        let position = d
            .source
            .as_ref()
            .map(|s| format!("{}:{}:{}: ", s.file, s.line, s.column))
            .unwrap_or_default();
        eprintln!(
            "{position}page {} [{}] {}: {}",
            d.page,
            d.code,
            d.element.as_deref().unwrap_or(&d.source_id),
            d.message
        );
    }
    if let Some(path) = args.report {
        save(
            &path,
            &serde_json::to_vec_pretty(&presentation.diagnostics)?,
        )?;
    }
    if let Some(path) = args.dump_ir {
        save(&path, &serde_json::to_vec_pretty(&presentation)?)?;
    }
    ensure!(
        !args.strict || presentation.diagnostics.is_empty(),
        "strict conversion stopped: {} diagnostics; no PPTX was written",
        presentation.diagnostics.len()
    );
    let bytes = pptx::write(&presentation)?;
    save(&output, &bytes)?;
    if let Some(dir) = args.reference_dir {
        std::fs::create_dir_all(&dir)?;
        for (idx, page) in document.pages().iter().enumerate() {
            save(
                &dir.join(format!("page-{}.png", idx + 1)),
                &typst_render::render(page, &Default::default()).encode_png()?,
            )?;
        }
    }
    use typptx::ir::Element;
    let (mut text, mut tables, mut shapes, mut pictures, mut fallbacks, mut svg_math) =
        (0, 0, 0, 0, 0, 0);
    for element in presentation
        .slides
        .iter()
        .flat_map(|s| s.elements.iter().flat_map(Element::walk))
    {
        match element {
            Element::Group(_) => {}
            Element::Text(_) => text += 1,
            Element::Table(_) => tables += 1,
            Element::Shape(_) => shapes += 1,
            Element::Picture { .. } => pictures += 1,
            Element::Drawing { .. } => fallbacks += 1,
            Element::MathSvg { .. } => svg_math += 1,
        }
    }
    eprintln!(
        "Wrote {} ({} slides; {text} text boxes, {tables} tables, {shapes} shapes, {pictures} original pictures, {svg_math} SVG equations, {fallbacks} image fallbacks)",
        output.display(),
        presentation.slides.len()
    );
    Ok(())
}

fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending
        .persist(path)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

fn validate_destinations(input: &Path, destinations: &[PathBuf]) -> Result<()> {
    let input = input.canonicalize()?;
    let mut seen = std::collections::HashSet::new();
    for path in destinations {
        let resolved = resolved_path(path)?;
        ensure!(
            resolved != input,
            "output must not overwrite the Typst source: {}",
            path.display()
        );
        ensure!(
            seen.insert(resolved),
            "output paths must be distinct: {}",
            path.display()
        );
    }
    Ok(())
}

fn resolved_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    if absolute.exists() {
        return Ok(absolute.canonicalize()?);
    }
    let parent = absolute.parent().context("output needs a filename")?;
    let name = absolute.file_name().context("output needs a filename")?;
    Ok(resolved_path(parent)?.join(name))
}
