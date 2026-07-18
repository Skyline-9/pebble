//! Bounded chunking and packaged Tree-sitter source extraction.

mod chunk;
mod language;
mod parser;
mod queries;
mod scip;
mod scip_wire;

use crate::domain::{FileId, SymbolId};
use crate::repository::{RepositoryConfig, SourceFile};

pub use chunk::{Chunk, MAX_CHUNK_BYTES};
pub use language::Language;
pub use scip::{ScipError, ScipExtraction, ScipImporter};

/// Kind of conservative structural relationship extracted from source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeKind {
    /// A file defines a named symbol.
    Defines,
    /// A file imports a named module or path.
    Imports,
    /// Source invokes a syntactically explicit call target.
    Calls,
    /// Source contains an identifier reference.
    References,
}

/// One named declaration found in a parsed source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Symbol {
    id: SymbolId,
    name: String,
    start_line: u32,
    end_line: u32,
}

impl Symbol {
    /// Return the deterministic symbol identity.
    #[must_use]
    pub const fn id(&self) -> &SymbolId {
        &self.id
    }

    /// Return the source-level declaration name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the one-based inclusive starting line.
    #[must_use]
    pub const fn start_line(&self) -> u32 {
        self.start_line
    }

    /// Return the one-based inclusive ending line.
    #[must_use]
    pub const fn end_line(&self) -> u32 {
        self.end_line
    }
}

/// One conservative structural edge with file-scoped source location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuralEdge {
    kind: EdgeKind,
    source: String,
    target: String,
    line: u32,
}

impl StructuralEdge {
    /// Return the relationship kind.
    #[must_use]
    pub const fn kind(&self) -> EdgeKind {
        self.kind
    }

    /// Return the source entity identity.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Return the target symbol, module, or identifier.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Return the one-based source line.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.line
    }
}

/// Kind of nonfatal file-scoped ingestion diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    /// No packaged grammar was selected.
    UnknownLanguage,
    /// The selected parser rejected or recovered from malformed source.
    ParseError,
}

/// A visible nonfatal ingestion diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionDiagnostic {
    path: String,
    kind: DiagnosticKind,
    message: String,
}

impl IngestionDiagnostic {
    /// Return the repository-relative source path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the diagnostic category.
    #[must_use]
    pub const fn kind(&self) -> DiagnosticKind {
        self.kind
    }

    /// Return a bounded human-readable explanation.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Complete bounded extraction result for one source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileExtraction {
    language: Option<Language>,
    chunks: Vec<Chunk>,
    symbols: Vec<Symbol>,
    edges: Vec<StructuralEdge>,
    diagnostics: Vec<IngestionDiagnostic>,
}

impl FileExtraction {
    /// Return the selected packaged language mode, if any.
    #[must_use]
    pub const fn language(&self) -> Option<Language> {
        self.language
    }

    /// Return bounded lexical chunks.
    #[must_use]
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// Return named declarations.
    #[must_use]
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// Return conservative structural relationships.
    #[must_use]
    pub fn edges(&self) -> &[StructuralEdge] {
        &self.edges
    }

    /// Return visible nonfatal diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[IngestionDiagnostic] {
        &self.diagnostics
    }
}

/// Stateless, one-file-at-a-time packaged source extractor.
pub struct Extractor {
    detector: language::Detector,
    repository: crate::domain::RepositoryId,
}

impl Extractor {
    /// Build an extractor from repository language overrides.
    #[must_use]
    pub fn new(config: &RepositoryConfig) -> Self {
        Self {
            detector: language::Detector::new(config),
            repository: config.repository_id().clone(),
        }
    }

    /// Extract one source and release its syntax tree before returning.
    #[must_use]
    pub fn extract(&self, source: &SourceFile) -> FileExtraction {
        let chunks = chunk::chunks(source.id(), source.contents());
        let Some(language) = self.detector.detect(source.path()) else {
            return fallback(source, None, chunks, DiagnosticKind::UnknownLanguage);
        };
        match parser::parse(&self.repository, source, language) {
            Some((symbols, edges)) => FileExtraction {
                language: Some(language),
                chunks,
                symbols,
                edges,
                diagnostics: Vec::new(),
            },
            None => fallback(source, Some(language), chunks, DiagnosticKind::ParseError),
        }
    }
}

fn fallback(
    source: &SourceFile,
    language: Option<Language>,
    chunks: Vec<Chunk>,
    kind: DiagnosticKind,
) -> FileExtraction {
    let message = match kind {
        DiagnosticKind::UnknownLanguage => "no packaged grammar; used text chunks",
        DiagnosticKind::ParseError => "source contained parse errors; used text chunks",
    };
    FileExtraction {
        language,
        chunks,
        symbols: Vec::new(),
        edges: Vec::new(),
        diagnostics: vec![IngestionDiagnostic {
            path: source.path().to_owned(),
            kind,
            message: message.to_owned(),
        }],
    }
}

fn edge(kind: EdgeKind, file: &FileId, target: String, line: u32) -> StructuralEdge {
    StructuralEdge {
        kind,
        source: file.as_str().to_owned(),
        target,
        line,
    }
}
