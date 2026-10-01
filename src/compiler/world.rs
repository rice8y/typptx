//! The compiler host. File, package, and font resolution follow Typst's CLI.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use typst::diag::FileResult;
use typst::foundations::{Bytes, Datetime, Dict, Duration, IntoValue};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, World, WorldExt};
use typst_kit::datetime::Time;
use typst_kit::downloader::SystemDownloader;
use typst_kit::files::{FileStore, FsRoot, SystemFiles};
use typst_kit::fonts::{self, FontStore};
use typst_kit::packages::SystemPackages;
use typst_layout::PagedDocument;

pub struct CompilerWorld {
    library: LazyHash<Library>,
    fonts: FontStore,
    files: FileStore<SystemFiles>,
    main: FileId,
    root: PathBuf,
    now: Time,
}

impl CompilerWorld {
    pub fn new(
        input: &Path,
        root: Option<&Path>,
        font_paths: &[PathBuf],
        inputs: &[(String, String)],
    ) -> Result<Self> {
        let input = input
            .canonicalize()
            .with_context(|| format!("cannot open {}", input.display()))?;
        let root = root.unwrap_or(input.parent().unwrap()).canonicalize()?;
        let relative = input
            .strip_prefix(&root)
            .context("input file must be inside --root")?;
        let main = RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new(relative.to_string_lossy().as_ref())?,
        )
        .intern();
        let mut fonts = FontStore::new();
        for path in font_paths {
            fonts.extend(fonts::scan(path));
        }
        fonts.extend(fonts::system());
        fonts.extend(fonts::embedded());
        let inputs: Dict = inputs
            .iter()
            .map(|(k, v)| (k.as_str().into(), v.as_str().into_value()))
            .collect();
        Ok(Self {
            library: LazyHash::new(
                typst::LibraryBuilder::from_routines(&crate::compiler::semantics::ROUTINES)
                    .with_inputs(inputs)
                    .build(),
            ),
            fonts,
            files: FileStore::new(SystemFiles::new(
                FsRoot::new(root.clone()),
                SystemPackages::new(SystemDownloader::new("typptx/0.1")),
            )),
            main,
            root,
            now: Time::system(),
        })
    }

    pub fn compile(&self) -> Result<(PagedDocument, Vec<String>)> {
        let result = typst::compile::<PagedDocument>(self);
        let warnings = result.warnings.iter().map(|d| self.diagnostic(d)).collect();
        match result.output {
            Ok(document) => Ok((document, warnings)),
            Err(errors) => bail!(
                "{}",
                errors
                    .iter()
                    .map(|d| self.diagnostic(d))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }

    /// Resolve diagnostic spans using the same world that compiled the document.
    /// Generated content without a source span keeps `source: None`.
    pub fn locate_diagnostics(&self, presentation: &mut crate::ir::Presentation) {
        for diagnostic in &mut presentation.diagnostics {
            if diagnostic.source.is_none() {
                diagnostic.source = diagnostic.span.and_then(|span| self.source_location(span));
            }
        }
    }

    pub fn source_location(
        &self,
        span: impl Into<typst::syntax::DiagSpan>,
    ) -> Option<crate::ir::SourceLocation> {
        let span = span.into();
        let id = span.id()?;
        let source = self.source(id).ok()?;
        let range = self.range(span)?;
        let (line, column) = source.lines().byte_to_line_column(range.start)?;
        let (end_line, end_column) = source.lines().byte_to_line_column(range.end)?;
        let file = match id.root() {
            VirtualRoot::Project => self
                .root
                .join(id.vpath().get_without_slash())
                .display()
                .to_string(),
            VirtualRoot::Package(package) => format!("{package}{}", id.vpath().get_with_slash()),
        };
        Some(crate::ir::SourceLocation {
            file,
            line: line + 1,
            column: column + 1,
            end_line: end_line + 1,
            end_column: end_column + 1,
        })
    }

    fn diagnostic(&self, diagnostic: &typst::diag::SourceDiagnostic) -> String {
        let prefix = self
            .source_location(diagnostic.span)
            .map(|s| format!("{}:{}:{}: ", s.file, s.line, s.column))
            .unwrap_or_default();
        format!(
            "{prefix}{}{}",
            diagnostic.message,
            diagnostic
                .hints
                .iter()
                .map(|h| format!("\n  hint: {}", h.v))
                .collect::<String>()
        )
    }
}

impl World for CompilerWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        self.files.source(id)
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.file(id)
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }
    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        self.now.today(offset)
    }
}
