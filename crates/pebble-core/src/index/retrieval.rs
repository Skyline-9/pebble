//! Bounded retrieval-specific reads over the pinned graph connection.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};

use super::{GraphReadLimit, GraphReader, IndexError};

#[derive(Clone, Debug)]
pub struct RetrievalEntity {
    pub(crate) entity_id: String,
    pub(crate) repository: String,
    pub(crate) revision: String,
    pub(crate) path: String,
    pub(crate) language: String,
    pub(crate) symbol: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) kind: String,
}

impl GraphReader {
    pub(crate) fn retrieval_exact(
        &self,
        value: &str,
        limit: usize,
    ) -> Result<Vec<(String, Option<RetrievalEntity>)>, IndexError> {
        let limit = GraphReadLimit::try_from(limit)?;
        let mut statement = self.connection.prepare(
            "SELECT entity_id FROM (
                SELECT file_id AS entity_id, 0 AS ordering FROM files
                WHERE generation_id = ?1 AND path = ?2
                UNION ALL
                SELECT symbol_id AS entity_id, 1 AS ordering FROM symbols
                WHERE generation_id = ?1 AND name = ?2
             ) ORDER BY ordering, entity_id LIMIT ?3",
        )?;
        let ids = statement
            .query_map(params![self.generation, value, limit.sql()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| self.retrieval_resolve(&id).map(|resolved| (id, resolved)))
            .collect()
    }

    pub(crate) fn retrieval_neighbors(
        &self,
        seeds: &[String],
        limit: usize,
    ) -> Result<Vec<(String, Option<RetrievalEntity>)>, IndexError> {
        let limit = GraphReadLimit::try_from(limit)?;
        let seed_ids = seeds.iter().cloned().collect::<BTreeSet<_>>();
        let mut ids = BTreeSet::new();
        let mut frontier = seeds.to_vec();
        for _ in 0..2 {
            if frontier.is_empty() || ids.len() >= usize::try_from(limit.sql()).unwrap_or(0) {
                break;
            }
            frontier = self.neighbor_ids(frontier, limit, &seed_ids, &mut ids)?;
        }
        ids.into_iter()
            .filter(|id| !seed_ids.contains(id))
            .map(|id| self.retrieval_resolve(&id).map(|resolved| (id, resolved)))
            .collect()
    }

    fn neighbor_ids(
        &self,
        frontier: Vec<String>,
        limit: GraphReadLimit,
        excluded: &BTreeSet<String>,
        ids: &mut BTreeSet<String>,
    ) -> Result<Vec<String>, IndexError> {
        let mut next = Vec::new();
        for seed in frontier {
            let mut statement = self.connection.prepare(
                "SELECT neighbor FROM (
                    SELECT target_entity_id AS neighbor FROM edges
                    WHERE generation_id = ?1 AND source_id = ?2
                      AND target_entity_id IS NOT NULL
                    UNION
                    SELECT source_id AS neighbor FROM edges
                    WHERE generation_id = ?1 AND target_entity_id = ?2
                 ) WHERE neighbor IS NOT NULL ORDER BY neighbor LIMIT ?3",
            )?;
            let neighbors = statement
                .query_map(params![self.generation, seed, limit.sql()], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for neighbor in neighbors {
                if ids.len() >= usize::try_from(limit.sql()).unwrap_or(0) {
                    break;
                }
                if !excluded.contains(&neighbor) && ids.insert(neighbor.clone()) {
                    next.push(neighbor);
                }
            }
        }
        Ok(next)
    }

    pub(crate) fn retrieval_resolve(
        &self,
        entity_id: &str,
    ) -> Result<Option<RetrievalEntity>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT entity.entity_id, entity.entity_kind,
                    file.repository_id, file.revision, file.path, file.language,
                    symbol.name,
                    COALESCE(symbol.start_line, chunk.start_line, 1),
                    COALESCE(symbol.end_line, chunk.end_line, 1),
                    CASE
                      WHEN chunk.chunk_id IS NOT NULL THEN chunk.content
                      WHEN symbol.symbol_id IS NOT NULL THEN (
                        SELECT excerpt.content FROM chunks AS excerpt
                        WHERE excerpt.generation_id = entity.generation_id
                          AND excerpt.file_id = symbol.file_id
                          AND excerpt.start_line <= symbol.start_line
                          AND excerpt.end_line >= symbol.end_line
                        ORDER BY excerpt.end_line - excerpt.start_line, excerpt.chunk_id
                        LIMIT 1
                      )
                      ELSE NULL
                    END,
                    CASE
                      WHEN chunk.chunk_id IS NOT NULL THEN chunk.start_line
                      WHEN symbol.symbol_id IS NOT NULL THEN (
                        SELECT excerpt.start_line FROM chunks AS excerpt
                        WHERE excerpt.generation_id = entity.generation_id
                          AND excerpt.file_id = symbol.file_id
                          AND excerpt.start_line <= symbol.start_line
                          AND excerpt.end_line >= symbol.end_line
                        ORDER BY excerpt.end_line - excerpt.start_line, excerpt.chunk_id
                        LIMIT 1
                      )
                      ELSE 1
                    END
             FROM entities AS entity
             LEFT JOIN symbols AS symbol
               ON symbol.generation_id = entity.generation_id
              AND symbol.symbol_id = entity.entity_id
             LEFT JOIN chunks AS chunk
               ON chunk.generation_id = entity.generation_id
              AND chunk.chunk_id = entity.entity_id
             JOIN files AS file
               ON file.generation_id = entity.generation_id
              AND file.file_id = COALESCE(symbol.file_id, chunk.file_id, entity.entity_id)
             WHERE entity.generation_id = ?1 AND entity.entity_id = ?2",
        )?;
        statement
            .query_row(params![self.generation, entity_id], |row| {
                let kind = row.get::<_, String>(1)?;
                let start_line = row.get(7)?;
                let end_line = row.get(8)?;
                let content = row.get::<_, Option<String>>(9)?;
                let content_start = row.get::<_, Option<u32>>(10)?;
                Ok(RetrievalEntity {
                    entity_id: row.get(0)?,
                    kind: kind.clone(),
                    repository: row.get(2)?,
                    revision: row.get(3)?,
                    path: row.get(4)?,
                    language: row.get(5)?,
                    symbol: row.get(6)?,
                    start_line,
                    end_line,
                    content: content.and_then(|value| {
                        if kind == "symbol" {
                            content_start.map(|source_start| {
                                line_excerpt(&value, source_start, start_line, end_line)
                            })
                        } else {
                            Some(value)
                        }
                    }),
                })
            })
            .optional()
            .map_err(Into::into)
    }
}

fn line_excerpt(source: &str, source_start: u32, start: u32, end: u32) -> String {
    let skip = start.saturating_sub(source_start) as usize;
    let count = end.saturating_sub(start).saturating_add(1) as usize;
    source
        .split_inclusive('\n')
        .skip(skip)
        .take(count)
        .collect()
}
