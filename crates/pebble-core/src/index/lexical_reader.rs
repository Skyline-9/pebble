//! Read-only queries over one immutable lexical generation.

use std::collections::BTreeSet;
use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Query, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, TantivyDocument, Value};
use tantivy::{Index, IndexReader, ReloadPolicy, Term};

use super::IndexError;
use super::lexical::{Fields, register_tokenizer, schema};

const MAX_RESULTS: usize = 10_000;
const MAX_QUERY_BYTES: usize = 16 * 1024;
const MAX_QUERY_TERMS: usize = 256;

/// One stored lexical match from an immutable generation.
#[derive(Clone, Debug, PartialEq)]
pub struct LexicalHit {
    entity_id: String,
    repository: String,
    revision: String,
    path: String,
    language: String,
    symbol: Option<String>,
    body: String,
    start_line: u32,
    end_line: u32,
    kind: String,
    score: f32,
}

macro_rules! text_accessors {
    ($($name:ident),+ $(,)?) => {$(
        #[doc = concat!("Return the stored ", stringify!($name), ".")]
        #[must_use]
        pub fn $name(&self) -> &str { &self.$name }
    )+};
}

impl LexicalHit {
    text_accessors!(repository, revision, path, language, body, kind);

    /// Return the stable graph entity ID.
    #[must_use]
    pub fn entity_id(&self) -> &str {
        &self.entity_id
    }

    /// Return the exact symbol name for symbol documents.
    #[must_use]
    pub fn symbol(&self) -> Option<&str> {
        self.symbol.as_deref()
    }

    /// Return the one-based inclusive start line.
    #[must_use]
    pub const fn start_line(&self) -> u32 {
        self.start_line
    }

    /// Return the one-based inclusive end line.
    #[must_use]
    pub const fn end_line(&self) -> u32 {
        self.end_line
    }

    /// Return the Tantivy relevance score.
    #[must_use]
    pub const fn score(&self) -> f32 {
        self.score
    }
}

/// Read-only search handle pinned to one immutable lexical generation.
pub struct LexicalReader {
    pub(super) index: Index,
    pub(super) reader: IndexReader,
    pub(super) fields: Fields,
}

impl LexicalReader {
    /// Open and validate a sealed memory-mapped lexical index.
    ///
    /// # Errors
    ///
    /// Returns rebuild-required for an invalid schema or corrupt segment.
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        let directory = super::pinned_directory::PinnedDirectory::open(path)?;
        super::generation_races::run(
            super::generation_races::RacePoint::LexicalReaderOpen,
            path.parent().unwrap_or(path),
            path,
        );
        Self::open_pinned(directory)
    }

    pub(super) fn open_pinned(
        directory: super::pinned_directory::PinnedDirectory,
    ) -> Result<Self, IndexError> {
        let index = Index::open(directory).map_err(IndexError::into_rebuild)?;
        let (expected, fields) = schema();
        if index.schema() != expected {
            return Err(IndexError::rebuild("lexical schema is invalid"));
        }
        register_tokenizer(&index).map_err(IndexError::into_rebuild)?;
        if !index
            .validate_checksum()
            .map_err(IndexError::into_rebuild)?
            .is_empty()
        {
            return Err(IndexError::rebuild("lexical checksum validation failed"));
        }
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(IndexError::into_rebuild)?;
        Ok(Self {
            index,
            reader,
            fields,
        })
    }

    /// Search tokenized code or note text.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid limit or unreadable index data.
    pub fn search_text(&self, text: &str, limit: usize) -> Result<Vec<LexicalHit>, IndexError> {
        let terms = self.terms(text)?;
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        self.search(&BooleanQuery::new_multiterms_query(terms), limit)
    }

    /// Find documents with this exact normalized path.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid limit or unreadable index data.
    pub fn exact_path(&self, path: &str, limit: usize) -> Result<Vec<LexicalHit>, IndexError> {
        validate_query_size(path)?;
        self.exact(self.fields.path, path, limit)
    }

    /// Find symbol documents with this exact symbol name.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid limit or unreadable index data.
    pub fn exact_symbol(&self, symbol: &str, limit: usize) -> Result<Vec<LexicalHit>, IndexError> {
        validate_query_size(symbol)?;
        self.exact(self.fields.symbol, symbol, limit)
    }

    /// Find documents containing this exact case-folded code identifier.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid limit or unreadable index data.
    pub fn exact_identifier(
        &self,
        identifier: &str,
        limit: usize,
    ) -> Result<Vec<LexicalHit>, IndexError> {
        let terms = self.terms(identifier)?;
        if terms.len() != 1 {
            return Ok(Vec::new());
        }
        self.search(
            &TermQuery::new(terms[0].clone(), IndexRecordOption::WithFreqs),
            limit,
        )
    }

    /// Return the committed document count.
    #[must_use]
    pub fn document_count(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    fn terms(&self, text: &str) -> Result<Vec<Term>, IndexError> {
        validate_query_size(text)?;
        let mut analyzer = self.index.tokenizer_for_field(self.fields.body)?;
        let mut stream = analyzer.token_stream(text);
        let mut terms = BTreeSet::new();
        while stream.advance() {
            terms.insert(Term::from_field_text(
                self.fields.body,
                &stream.token().text,
            ));
            if terms.len() > MAX_QUERY_TERMS {
                return Err(IndexError::TooManyQueryTerms {
                    maximum: MAX_QUERY_TERMS,
                });
            }
        }
        Ok(terms.into_iter().collect())
    }

    fn exact(
        &self,
        field: Field,
        value: &str,
        limit: usize,
    ) -> Result<Vec<LexicalHit>, IndexError> {
        self.search(
            &TermQuery::new(
                Term::from_field_text(field, value),
                IndexRecordOption::Basic,
            ),
            limit,
        )
    }

    fn search(&self, query: &dyn Query, limit: usize) -> Result<Vec<LexicalHit>, IndexError> {
        if !(1..=MAX_RESULTS).contains(&limit) {
            return Err(IndexError::rebuild(
                "lexical result limit must be between 1 and 10000",
            ));
        }
        let searcher = self.reader.searcher();
        searcher
            .search(query, &TopDocs::with_limit(limit).order_by_score())?
            .into_iter()
            .map(|(score, address)| {
                let document: TantivyDocument = searcher.doc(address)?;
                hit(&document, self.fields, score)
            })
            .collect()
    }
}

const fn validate_query_size(query: &str) -> Result<(), IndexError> {
    if query.len() > MAX_QUERY_BYTES {
        return Err(IndexError::QueryTooLarge {
            actual: query.len(),
            maximum: MAX_QUERY_BYTES,
        });
    }
    Ok(())
}

pub(super) fn hit(
    document: &TantivyDocument,
    fields: Fields,
    score: f32,
) -> Result<LexicalHit, IndexError> {
    let text = |field| {
        document
            .get_first(field)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| IndexError::rebuild("lexical document field is missing"))
    };
    let line = |field| {
        document
            .get_first(field)
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| IndexError::rebuild("lexical document line is invalid"))
    };
    Ok(LexicalHit {
        entity_id: text(fields.entity)?,
        repository: text(fields.repository)?,
        revision: text(fields.revision)?,
        path: text(fields.path)?,
        language: text(fields.language)?,
        symbol: document
            .get_first(fields.symbol)
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        body: text(fields.body)?,
        start_line: line(fields.start)?,
        end_line: line(fields.end)?,
        kind: text(fields.kind)?,
        score,
    })
}
