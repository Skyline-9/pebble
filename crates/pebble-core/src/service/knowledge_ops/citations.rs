//! Building the allow-list of citations a queued patch may reference.

use std::collections::HashSet;

use rusqlite::{Connection, OpenFlags, params};

use crate::index::GenerationReader;
use crate::knowledge::AllowedCitation;

use super::super::ServiceError;

/// Largest number of allow-listed citations collected from one generation.
const MAX_ALLOWED_CITATIONS: i64 = 200_000;

pub(super) fn collect_allowed_citations(
    reader: &GenerationReader,
) -> Result<HashSet<AllowedCitation>, ServiceError> {
    let connection = Connection::open_with_flags(
        reader.graph_path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(ServiceError::operational)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(ServiceError::operational)?;
    let mut statement = connection
        .prepare(
            "SELECT files.repository_id, files.path, symbols.name
             FROM symbols
             JOIN files ON files.generation_id = symbols.generation_id
               AND files.file_id = symbols.file_id
             WHERE symbols.generation_id = ?1
             LIMIT ?2",
        )
        .map_err(ServiceError::operational)?;
    let rows = statement
        .query_map(
            params![reader.id().as_str(), MAX_ALLOWED_CITATIONS],
            |row| {
                Ok(AllowedCitation {
                    repo: row.get::<_, String>(0)?,
                    path: row.get::<_, String>(1)?,
                    symbol: row.get::<_, String>(2)?,
                })
            },
        )
        .map_err(ServiceError::operational)?;
    rows.collect::<Result<HashSet<_>, _>>()
        .map_err(ServiceError::operational)
}
