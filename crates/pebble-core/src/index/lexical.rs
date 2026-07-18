//! Immutable, bounded Tantivy lexical generation storage.

use std::path::{Path, PathBuf};

use tantivy::schema::{
    FAST, Field, IndexRecordOption, STORED, STRING, Schema, TantivyDocument, TextFieldIndexing,
    TextOptions,
};
use tantivy::tokenizer::{LowerCaser, RegexTokenizer, RemoveLongFilter, TextAnalyzer};
use tantivy::{Index, IndexSettings, IndexWriter, TantivyError};

use crate::domain::{ChunkId, RepositoryId, SymbolId, WorktreeRevision};

use super::IndexError;

const CODE_TOKENIZER: &str = "pebble_code";
const WRITER_MEMORY_BYTES: usize = 15_000_000;

#[derive(Clone, Copy)]
pub(super) struct Fields {
    pub(super) entity: Field,
    pub(super) repository: Field,
    pub(super) revision: Field,
    pub(super) path: Field,
    pub(super) language: Field,
    pub(super) symbol: Field,
    pub(super) body: Field,
    pub(super) start: Field,
    pub(super) end: Field,
    pub(super) kind: Field,
}

/// Bounded writer for one immutable lexical generation.
pub struct LexicalWriter {
    untrusted_name: PathBuf,
    directory: super::pinned_directory::PinnedDirectory,
    fields: Fields,
    writer: IndexWriter,
}

impl LexicalWriter {
    /// Create a fresh lexical index in an existing empty directory.
    ///
    /// # Errors
    ///
    /// Returns an error when Tantivy cannot create its index or bounded writer.
    pub fn create(path: &Path) -> Result<Self, IndexError> {
        let directory = super::pinned_directory::PinnedDirectory::open(path)?;
        Self::create_pinned(path, directory)
    }

    pub(super) fn create_pinned(
        path: &Path,
        directory: super::pinned_directory::PinnedDirectory,
    ) -> Result<Self, IndexError> {
        let (schema, fields) = schema();
        if Index::exists(&directory).map_err(IndexError::into_rebuild)? {
            return Err(TantivyError::IndexAlreadyExists.into());
        }
        let index = Index::create(directory.clone(), schema, IndexSettings::default())?;
        register_tokenizer(&index)?;
        let writer = index.writer_with_num_threads(1, WRITER_MEMORY_BYTES)?;
        Ok(Self {
            untrusted_name: path.to_owned(),
            directory,
            fields,
            writer,
        })
    }

    /// Add one stable chunk document.
    ///
    /// # Errors
    ///
    /// Returns an error when Tantivy rejects the document.
    #[allow(clippy::too_many_arguments)]
    pub fn add_chunk(
        &mut self,
        id: &ChunkId,
        repository: &RepositoryId,
        revision: &WorktreeRevision,
        path: &str,
        language: &str,
        body: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<(), IndexError> {
        self.add(Record {
            entity: id.as_str(),
            repository,
            revision,
            path,
            language,
            symbol: None,
            body,
            start_line,
            end_line,
            kind: "chunk",
        })
    }

    /// Add one stable symbol document.
    ///
    /// # Errors
    ///
    /// Returns an error when Tantivy rejects the document.
    #[allow(clippy::too_many_arguments)]
    pub fn add_symbol(
        &mut self,
        id: &SymbolId,
        repository: &RepositoryId,
        revision: &WorktreeRevision,
        path: &str,
        language: &str,
        symbol: &str,
        body: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<(), IndexError> {
        self.add(Record {
            entity: id.as_str(),
            repository,
            revision,
            path,
            language,
            symbol: Some(symbol),
            body,
            start_line,
            end_line,
            kind: "symbol",
        })
    }

    /// Commit exactly once and reopen the immutable index.
    ///
    /// # Errors
    ///
    /// Returns an error when commit, merge completion, or validation fails.
    pub fn finish(mut self) -> Result<super::LexicalReader, IndexError> {
        super::generation_races::run(
            super::generation_races::RacePoint::LexicalCommit,
            self.untrusted_name.parent().unwrap_or(&self.untrusted_name),
            &self.untrusted_name,
        );
        self.writer.commit()?;
        self.writer.wait_merging_threads()?;
        super::LexicalReader::open_pinned(self.directory)
    }

    fn add(&self, record: Record<'_>) -> Result<(), IndexError> {
        let mut document = TantivyDocument::default();
        document.add_text(self.fields.entity, record.entity);
        document.add_text(self.fields.repository, record.repository.as_str());
        document.add_text(self.fields.revision, record.revision.to_string());
        document.add_text(self.fields.path, record.path);
        document.add_text(self.fields.language, record.language);
        if let Some(symbol) = record.symbol {
            document.add_text(self.fields.symbol, symbol);
        }
        document.add_text(self.fields.body, record.body);
        document.add_u64(self.fields.start, u64::from(record.start_line));
        document.add_u64(self.fields.end, u64::from(record.end_line));
        document.add_text(self.fields.kind, record.kind);
        self.writer.add_document(document)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Record<'record> {
    entity: &'record str,
    repository: &'record RepositoryId,
    revision: &'record WorktreeRevision,
    path: &'record str,
    language: &'record str,
    symbol: Option<&'record str>,
    body: &'record str,
    start_line: u32,
    end_line: u32,
    kind: &'static str,
}

pub(super) fn schema() -> (Schema, Fields) {
    let body = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(CODE_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    let mut builder = Schema::builder();
    let fields = Fields {
        entity: builder.add_text_field("entity_id", STRING | STORED | FAST),
        repository: builder.add_text_field("repository", STRING | STORED),
        revision: builder.add_text_field("revision", STRING | STORED),
        path: builder.add_text_field("path", STRING | STORED),
        language: builder.add_text_field("language", STRING | STORED),
        symbol: builder.add_text_field("symbol", STRING | STORED),
        body: builder.add_text_field("body", body),
        start: builder.add_u64_field("start_line", STORED),
        end: builder.add_u64_field("end_line", STORED),
        kind: builder.add_text_field("kind", STRING | STORED),
    };
    (builder.build(), fields)
}

pub(super) fn register_tokenizer(index: &Index) -> Result<(), IndexError> {
    let tokenizer = RegexTokenizer::new(r"[\p{L}\p{N}_]+")
        .map_err(|error| IndexError::rebuild(format!("code tokenizer is invalid: {error}")))?;
    let analyzer = TextAnalyzer::builder(tokenizer)
        .filter(RemoveLongFilter::limit(255))
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(CODE_TOKENIZER, analyzer);
    Ok(())
}
