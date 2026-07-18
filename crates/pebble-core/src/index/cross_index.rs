//! Bounded exact correspondence validation for graph and lexical entities.

use std::ops::Bound;

use rusqlite::params;
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, Query, RangeQuery};
use tantivy::schema::{TantivyDocument, Value};
use tantivy::{Order, Term};

use super::{GraphReader, IndexError, LexicalReader};

const VALIDATION_PAGE_ROWS: usize = 256;

#[derive(Debug, Eq, PartialEq)]
struct EntityRecord {
    entity_id: String,
    repository: String,
    revision: String,
    path: String,
    kind: String,
    start_line: u32,
    end_line: u32,
    language: String,
    symbol: Option<String>,
}

pub(super) fn validate(graph: &GraphReader, lexical: &LexicalReader) -> Result<(), IndexError> {
    let mut after = None;
    loop {
        let graph_page = graph.entity_records_after(after.as_deref())?;
        let lexical_page = lexical.entity_records_after(after.as_deref())?;
        if graph_page != lexical_page {
            return Err(IndexError::rebuild(
                "graph and lexical entity metadata differ",
            ));
        }
        let Some(last) = graph_page.last() else {
            return Ok(());
        };
        after = Some(last.entity_id.clone());
    }
}

impl GraphReader {
    fn entity_records_after(&self, after: Option<&str>) -> Result<Vec<EntityRecord>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT entity_id, repository_id, revision, path, kind,
                    start_line, end_line, language, symbol
             FROM (
                 SELECT chunk.chunk_id AS entity_id, file.repository_id, file.revision,
                        file.path, 'chunk' AS kind, chunk.start_line, chunk.end_line,
                        file.language, NULL AS symbol
                 FROM chunks AS chunk
                 JOIN files AS file
                   ON file.generation_id = chunk.generation_id
                  AND file.file_id = chunk.file_id
                 WHERE chunk.generation_id = ?1
                 UNION ALL
                 SELECT symbol.symbol_id, file.repository_id, file.revision,
                        file.path, 'symbol', symbol.start_line, symbol.end_line,
                        file.language, symbol.name
                 FROM symbols AS symbol
                 JOIN files AS file
                   ON file.generation_id = symbol.generation_id
                  AND file.file_id = symbol.file_id
                 WHERE symbol.generation_id = ?1
             )
             WHERE (?2 IS NULL OR entity_id > ?2)
             ORDER BY entity_id
             LIMIT ?3",
        )?;
        let limit = i64::try_from(VALIDATION_PAGE_ROWS)
            .map_err(|_| IndexError::rebuild("cross-index page limit is invalid"))?;
        let rows = statement.query_map(params![self.generation, after, limit], |row| {
            Ok(EntityRecord {
                entity_id: row.get(0)?,
                repository: row.get(1)?,
                revision: row.get(2)?,
                path: row.get(3)?,
                kind: row.get(4)?,
                start_line: row.get(5)?,
                end_line: row.get(6)?,
                language: row.get(7)?,
                symbol: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

impl LexicalReader {
    fn entity_records_after(&self, after: Option<&str>) -> Result<Vec<EntityRecord>, IndexError> {
        let searcher = self.reader.searcher();
        let query: Box<dyn Query> = after.map_or_else(
            || Box::new(AllQuery) as Box<dyn Query>,
            |entity| {
                Box::new(RangeQuery::new(
                    Bound::Excluded(Term::from_field_text(self.fields.entity, entity)),
                    Bound::Unbounded,
                ))
            },
        );
        let documents = searcher.search(
            query.as_ref(),
            &TopDocs::with_limit(VALIDATION_PAGE_ROWS)
                .order_by_string_fast_field("entity_id", Order::Asc),
        )?;
        documents
            .into_iter()
            .map(|(_, address)| {
                let document: TantivyDocument = searcher.doc(address)?;
                lexical_record(&document, self)
            })
            .collect()
    }
}

fn lexical_record(
    document: &TantivyDocument,
    reader: &LexicalReader,
) -> Result<EntityRecord, IndexError> {
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
    Ok(EntityRecord {
        entity_id: text(reader.fields.entity)?,
        repository: text(reader.fields.repository)?,
        revision: text(reader.fields.revision)?,
        path: text(reader.fields.path)?,
        kind: text(reader.fields.kind)?,
        start_line: line(reader.fields.start)?,
        end_line: line(reader.fields.end)?,
        language: text(reader.fields.language)?,
        symbol: document
            .get_first(reader.fields.symbol)
            .and_then(|value| value.as_str())
            .map(str::to_owned),
    })
}
