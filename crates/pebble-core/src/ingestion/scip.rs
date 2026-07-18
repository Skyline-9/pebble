//! Bounded SCIP conversion into Pebble structural contracts.

use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use thiserror::Error;

use crate::domain::{FileId, RepositoryId, SymbolId};

use super::scip_wire::{self, Document};
use super::{EdgeKind, StructuralEdge, Symbol};

const DEFINITION_ROLE: i32 = 1;
const IMPORT_ROLE: i32 = 2;

const _: usize = size_of::<scip::types::Index>();
const _: usize = size_of::<scip::types::Document>();
const _: usize = size_of::<scip::types::Occurrence>();
const _: usize = size_of::<scip::types::SymbolInformation>();

/// A malformed or out-of-policy SCIP index.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ScipError {
    /// The encoded index exceeded 64 MiB.
    #[error("SCIP index exceeds the 64 MiB input limit")]
    InputTooLarge,
    /// A nested message or group exceeded depth 64.
    #[error("SCIP message nesting exceeds the depth limit")]
    DepthLimit,
    /// A bounded SCIP record collection exceeded its limit.
    #[error("SCIP {0} count exceeds its limit")]
    CountLimit(&'static str),
    /// A consumed UTF-8 string exceeded 1 MiB.
    #[error("SCIP string exceeds the 1 MiB limit")]
    StringTooLarge,
    /// A consumed string was not valid UTF-8.
    #[error("SCIP string is not valid UTF-8")]
    MalformedUtf8,
    /// Protobuf framing was truncated or impossible.
    #[error("SCIP protobuf wire framing is malformed")]
    MalformedWire,
    /// An occurrence range violated the SCIP position contract.
    #[error("SCIP occurrence range is invalid")]
    InvalidRange,
}

/// Symbols, structural edges, and unresolved cross-index symbol references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScipExtraction {
    symbols: Vec<Symbol>,
    edges: Vec<StructuralEdge>,
    unresolved_external_symbols: Vec<String>,
}

impl ScipExtraction {
    /// Return symbols defined by imported documents.
    #[must_use]
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// Return structural relationships imported from occurrences.
    #[must_use]
    pub fn edges(&self) -> &[StructuralEdge] {
        &self.edges
    }

    /// Return sorted external symbols referenced but not defined in this index.
    #[must_use]
    pub fn unresolved_external_symbols(&self) -> &[String] {
        &self.unresolved_external_symbols
    }
}

/// Stateless bounded importer for one repository's SCIP index.
pub struct ScipImporter {
    repository: RepositoryId,
}

impl ScipImporter {
    /// Create an importer whose generated identities belong to `repository`.
    #[must_use]
    pub const fn new(repository: RepositoryId) -> Self {
        Self { repository }
    }

    /// Decode and convert an encoded SCIP index.
    ///
    /// # Errors
    ///
    /// Returns an error for oversized input, malformed protobuf framing,
    /// excessive nesting or records, invalid UTF-8, or invalid source ranges.
    pub fn import(&self, bytes: &[u8]) -> Result<ScipExtraction, ScipError> {
        let index = scip_wire::decode(bytes)?;
        let identifiers = index
            .documents
            .iter()
            .flat_map(|document| {
                let definitions = document
                    .occurrences
                    .iter()
                    .filter(|occurrence| occurrence.roles & DEFINITION_ROLE != 0)
                    .map(|occurrence| occurrence.symbol.as_str())
                    .collect::<BTreeSet<_>>();
                document.symbols.iter().filter_map(move |information| {
                    let key = symbol_key(&document.path, &information.symbol);
                    definitions.contains(information.symbol.as_str()).then(|| {
                        (
                            key.clone(),
                            SymbolId::derive(&self.repository, &document.language, &key),
                        )
                    })
                })
            })
            .collect::<BTreeMap<_, _>>();
        let mut symbols = Vec::new();
        let mut edges = Vec::new();
        let mut defined = BTreeSet::new();
        let mut unresolved = BTreeSet::new();

        for document in &index.documents {
            let converted = convert_document(&self.repository, document, &identifiers)?;
            defined.extend(converted.defined);
            symbols.extend(converted.symbols);
            edges.extend(converted.edges);
        }
        for edge in &edges {
            if edge.kind != EdgeKind::Defines
                && !defined.contains(edge.target.as_str())
                && !edge.target.starts_with("local ")
            {
                unresolved.insert(edge.target.clone());
            }
        }
        Ok(ScipExtraction {
            symbols,
            edges,
            unresolved_external_symbols: unresolved.into_iter().collect(),
        })
    }
}

struct ConvertedDocument {
    symbols: Vec<Symbol>,
    edges: Vec<StructuralEdge>,
    defined: BTreeSet<String>,
}

fn convert_document(
    repository: &RepositoryId,
    document: &Document,
    all_identifiers: &BTreeMap<String, SymbolId>,
) -> Result<ConvertedDocument, ScipError> {
    let file = FileId::derive(repository, &document.path);
    let mut definitions = BTreeMap::new();
    for occurrence in &document.occurrences {
        let range = source_range(&occurrence.range)?;
        if occurrence.roles & DEFINITION_ROLE != 0 && !occurrence.symbol.is_empty() {
            definitions
                .entry(occurrence.symbol.as_str())
                .or_insert(range);
        }
    }

    let mut symbols = Vec::new();
    let mut defined = BTreeSet::new();
    for information in &document.symbols {
        if information.symbol.is_empty() {
            continue;
        }
        let Some(&(start_line, end_line)) = definitions.get(information.symbol.as_str()) else {
            continue;
        };
        let key = symbol_key(&document.path, &information.symbol);
        let id = all_identifiers
            .get(&key)
            .cloned()
            .unwrap_or_else(|| SymbolId::derive(repository, &document.language, &key));
        let name = if information.display_name.is_empty() {
            information.symbol.clone()
        } else {
            information.display_name.clone()
        };
        defined.insert(id.as_str().to_owned());
        symbols.push(Symbol {
            id,
            name,
            start_line,
            end_line,
        });
    }

    let mut edges = Vec::new();
    for occurrence in &document.occurrences {
        if occurrence.symbol.is_empty() {
            continue;
        }
        let (line, _) = source_range(&occurrence.range)?;
        let target = all_identifiers
            .get(&symbol_key(&document.path, &occurrence.symbol))
            .map_or_else(|| occurrence.symbol.clone(), |id| id.as_str().to_owned());
        let kind = if occurrence.roles & DEFINITION_ROLE != 0 {
            EdgeKind::Defines
        } else if occurrence.roles & IMPORT_ROLE != 0 {
            EdgeKind::Imports
        } else {
            EdgeKind::References
        };
        edges.push(StructuralEdge {
            kind,
            source: file.as_str().to_owned(),
            target,
            line,
        });
    }
    Ok(ConvertedDocument {
        symbols,
        edges,
        defined,
    })
}

fn symbol_key(path: &str, symbol: &str) -> String {
    if symbol.starts_with("local ") {
        format!("{path}:{symbol}")
    } else {
        symbol.to_owned()
    }
}

fn source_range(range: &[i32]) -> Result<(u32, u32), ScipError> {
    let (start_line, start_character, end_line, end_character) = match range {
        [line, start, end] => (*line, *start, *line, *end),
        [start_line, start, end_line, end] => (*start_line, *start, *end_line, *end),
        _ => return Err(ScipError::InvalidRange),
    };
    if start_line < 0
        || start_character < 0
        || end_line < 0
        || end_character < 0
        || start_line > end_line
        || (start_line == end_line && start_character > end_character)
    {
        return Err(ScipError::InvalidRange);
    }
    let ends_at_next_line_start = start_line < end_line && end_character == 0;
    let start = u32::try_from(start_line)
        .ok()
        .and_then(|line| line.checked_add(1))
        .ok_or(ScipError::InvalidRange)?;
    let end = u32::try_from(end_line)
        .ok()
        .and_then(|line| {
            if ends_at_next_line_start {
                Some(line)
            } else {
                line.checked_add(1)
            }
        })
        .ok_or(ScipError::InvalidRange)?;
    Ok((start, end))
}
