//! Deterministic one-snapshot repository compilation.

use std::path::{Path, PathBuf};

use ulid::Ulid;

use crate::domain::{GenerationId, RepositoryId, WorktreeRevision};
use crate::ingestion::{DiagnosticKind, EdgeKind, Extractor, FileExtraction, StructuralEdge};
use crate::repository::{RepositoryConfig, RepositorySnapshot, SkipReason, SourceFile, SystemGit};
use crate::vectors::TextEmbedder;
use crate::vectors::format::VectorFileWriter;

use super::{
    EdgeTarget, GenerationBuilder, GenerationReader, GraphEdgeKind, GraphTransaction, IndexError,
};

/// Optional deterministic compiler fault used to verify crash isolation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerFault {
    /// Compile without injecting a fault.
    None,
    /// Stop after all `SQLite` graph records and before Tantivy completion.
    AfterGraph,
}

/// Stateless compiler from one stable repository snapshot to one generation.
pub struct RepositoryCompiler {
    generations: PathBuf,
}

impl RepositoryCompiler {
    /// Bind a compiler to one repository's generation root.
    #[must_use]
    pub fn new(generations: &Path) -> Self {
        Self {
            generations: generations.to_owned(),
        }
    }

    /// Compile and atomically activate one complete generation.
    ///
    /// # Errors
    ///
    /// Returns an error for snapshot, extraction storage, validation, or activation failures.
    pub fn compile(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
        generation: GenerationId,
    ) -> Result<GenerationReader, IndexError> {
        self.compile_with_fault(repository, config, generation, CompilerFault::None)
    }

    /// Compile one complete generation, additionally building a sealed
    /// vector generation streamed from `embedder`.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`Self::compile`], plus embedding I/O
    /// failures surfaced through [`IndexError::Io`].
    pub fn compile_with_embedder(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
        generation: GenerationId,
        embedder: &dyn TextEmbedder,
    ) -> Result<GenerationReader, IndexError> {
        self.compile_inner(
            repository,
            config,
            generation,
            CompilerFault::None,
            Some(embedder),
        )
    }

    /// Compile using a newly allocated generation ID.
    ///
    /// An existing inert `<id>.building` path is never removed or reused.
    /// Instead, this method retries with a fresh unpredictable ID.
    ///
    /// # Errors
    ///
    /// Returns an error for generation allocation, snapshot, extraction,
    /// storage, validation, or activation failures.
    pub fn compile_fresh(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
    ) -> Result<GenerationReader, IndexError> {
        self.compile_fresh_inner(repository, config, None)
    }

    /// Compile a fresh generation using a newly allocated generation ID,
    /// additionally building a sealed vector generation streamed from
    /// `embedder`.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`Self::compile_fresh`], plus embedding
    /// I/O failures surfaced through [`IndexError::Io`].
    pub fn compile_fresh_with_embedder(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
        embedder: &dyn TextEmbedder,
    ) -> Result<GenerationReader, IndexError> {
        self.compile_fresh_inner(repository, config, Some(embedder))
    }

    fn compile_fresh_inner(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
        embedder: Option<&dyn TextEmbedder>,
    ) -> Result<GenerationReader, IndexError> {
        const MAX_ALLOCATION_ATTEMPTS: usize = 16;
        let mut last_generation = String::new();
        for _ in 0..MAX_ALLOCATION_ATTEMPTS {
            let generation = GenerationId::try_from(Ulid::new().to_string())?;
            generation.as_str().clone_into(&mut last_generation);
            match self.compile_inner(
                repository,
                config,
                generation,
                CompilerFault::None,
                embedder,
            ) {
                Err(IndexError::IncompleteBuild { .. }) => {}
                result => return result,
            }
        }
        Err(IndexError::IncompleteBuild {
            generation: last_generation,
        })
    }

    /// Compile with a deterministic pre-activation fault point.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::InjectedFault`] at the selected point, otherwise
    /// the same failures as [`Self::compile`].
    pub fn compile_with_fault(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
        generation: GenerationId,
        fault: CompilerFault,
    ) -> Result<GenerationReader, IndexError> {
        self.compile_inner(repository, config, generation, fault, None)
    }

    fn compile_inner(
        &self,
        repository: &Path,
        config: &RepositoryConfig,
        generation: GenerationId,
        fault: CompilerFault,
        embedder: Option<&dyn TextEmbedder>,
    ) -> Result<GenerationReader, IndexError> {
        let git = SystemGit::discover()?;
        let mut snapshot = RepositorySnapshot::open(repository, config, &git)?;
        let revision = snapshot.revision().clone();
        let repository_id = config.repository_id();
        let extractor = Extractor::new(config);
        let mut builder =
            GenerationBuilder::create_with_embedder(&self.generations, generation, embedder)?;
        initialize_graph(builder.graph(), repository_id, &revision)?;
        let mut previous_path = None;

        for source in snapshot.by_ref() {
            let source = source?;
            if previous_path
                .as_deref()
                .is_some_and(|path| path >= source.path())
            {
                return Err(IndexError::rebuild(
                    "repository snapshot path order is not strictly increasing",
                ));
            }
            previous_path = Some(source.path().to_owned());
            let extraction = extractor.extract(&source);
            builder.graph().transaction(|graph| {
                write_graph_file(graph, repository_id, &revision, &source, &extraction)
            })?;
            write_lexical_file(
                builder.lexical(),
                repository_id,
                &revision,
                &source,
                &extraction,
            )?;
            if let Some(embedder) = embedder
                && let Some(vectors) = builder.vectors()
            {
                write_vector_file(vectors, embedder, &source, &extraction)?;
            }
        }
        for diagnostic in snapshot.diagnostics() {
            builder.graph().insert_diagnostic(
                Some(diagnostic.path()),
                "snapshot_skip",
                skip_message(diagnostic.reason()),
            )?;
        }
        if fault == CompilerFault::AfterGraph {
            return Err(IndexError::InjectedFault("after_graph"));
        }
        builder.seal()?.activate()
    }
}

fn initialize_graph(
    graph: &super::GraphWriter,
    repository: &RepositoryId,
    revision: &WorktreeRevision,
) -> Result<(), IndexError> {
    graph.transaction(|writer| {
        writer.insert_repository(repository, repository.as_str())?;
        writer.insert_revision(repository, revision)?;
        writer.set_metadata("compiler", "pebble-core")?;
        writer.set_metadata("revision", &revision.to_string())
    })
}

fn write_graph_file(
    graph: &GraphTransaction<'_>,
    repository: &RepositoryId,
    revision: &WorktreeRevision,
    source: &SourceFile,
    extraction: &FileExtraction,
) -> Result<(), IndexError> {
    let language = language_name(extraction);
    graph.insert_file(
        source.id(),
        repository,
        revision,
        source.path(),
        language,
        source.content_hash(),
    )?;
    for symbol in extraction.symbols() {
        graph.insert_symbol(
            symbol.id(),
            source.id(),
            symbol.name(),
            "declaration",
            symbol.start_line(),
            symbol.end_line(),
        )?;
    }
    for chunk in extraction.chunks() {
        graph.insert_chunk(
            chunk.id(),
            source.id(),
            chunk.start_line(),
            chunk.end_line(),
            chunk.text(),
            blake3::hash(chunk.text().as_bytes()).to_hex().as_str(),
        )?;
        graph.insert_edge(
            GraphEdgeKind::Contains,
            source.id().as_str(),
            EdgeTarget::Entity(chunk.id().as_str()),
            chunk.start_line(),
        )?;
    }
    for edge in extraction.edges() {
        write_edge(graph, edge)?;
    }
    for diagnostic in extraction.diagnostics() {
        let code = match diagnostic.kind() {
            DiagnosticKind::UnknownLanguage => "unknown_language",
            DiagnosticKind::ParseError => "parse_error",
        };
        graph.insert_diagnostic(Some(diagnostic.path()), code, diagnostic.message())?;
    }
    Ok(())
}

fn write_edge(graph: &GraphTransaction<'_>, edge: &StructuralEdge) -> Result<(), IndexError> {
    let (kind, target) = match edge.kind() {
        EdgeKind::Defines => (GraphEdgeKind::Defines, EdgeTarget::Entity(edge.target())),
        EdgeKind::Imports => (GraphEdgeKind::Imports, EdgeTarget::External(edge.target())),
        EdgeKind::Calls => (GraphEdgeKind::Calls, EdgeTarget::External(edge.target())),
        EdgeKind::References => (
            GraphEdgeKind::References,
            EdgeTarget::External(edge.target()),
        ),
    };
    graph.insert_edge(kind, edge.source(), target, edge.line())
}

fn write_lexical_file(
    lexical: &mut super::LexicalWriter,
    repository: &RepositoryId,
    revision: &WorktreeRevision,
    source: &SourceFile,
    extraction: &FileExtraction,
) -> Result<(), IndexError> {
    let language = language_name(extraction);
    for chunk in extraction.chunks() {
        lexical.add_chunk(
            chunk.id(),
            repository,
            revision,
            source.path(),
            language,
            chunk.text(),
            chunk.start_line(),
            chunk.end_line(),
        )?;
    }
    for symbol in extraction.symbols() {
        let body = line_excerpt(source.contents(), symbol.start_line(), symbol.end_line());
        lexical.add_symbol(
            symbol.id(),
            repository,
            revision,
            source.path(),
            language,
            symbol.name(),
            &body,
            symbol.start_line(),
            symbol.end_line(),
        )?;
    }
    Ok(())
}

fn write_vector_file(
    vectors: &mut VectorFileWriter,
    embedder: &dyn TextEmbedder,
    source: &SourceFile,
    extraction: &FileExtraction,
) -> Result<(), IndexError> {
    for chunk in extraction.chunks() {
        let embedding = embedder.embed_one(chunk.text())?;
        vectors.write_row(chunk.id().as_str(), &embedding)?;
    }
    for symbol in extraction.symbols() {
        let body = line_excerpt(source.contents(), symbol.start_line(), symbol.end_line());
        let embedding = embedder.embed_one(&body)?;
        vectors.write_row(symbol.id().as_str(), &embedding)?;
    }
    Ok(())
}

fn language_name(extraction: &FileExtraction) -> &'static str {
    extraction
        .language()
        .map_or("text", crate::ingestion::Language::name)
}

fn line_excerpt(source: &str, start: u32, end: u32) -> String {
    let count = end.saturating_sub(start).saturating_add(1);
    source
        .lines()
        .skip(start.saturating_sub(1) as usize)
        .take(count as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

const fn skip_message(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::SymbolicLink => "symbolic link was skipped",
        SkipReason::TooLarge => "source exceeded the size limit",
        SkipReason::Binary => "binary source was skipped",
        SkipReason::InvalidUtf8 => "source was not valid UTF-8",
    }
}
