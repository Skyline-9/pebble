//! Operations available inside one explicit graph transaction.

use rusqlite::{Connection, Transaction, params};

use crate::domain::{ChunkId, FileId, RepositoryId, SymbolId, WorktreeRevision};

use super::{EdgeTarget, GraphEdgeKind, IndexError};

/// One explicit `SQLite` graph transaction.
pub struct GraphTransaction<'transaction> {
    generation: &'transaction str,
    transaction: &'transaction Transaction<'transaction>,
}

impl<'transaction> GraphTransaction<'transaction> {
    pub(super) const fn new(
        generation: &'transaction str,
        transaction: &'transaction Transaction<'transaction>,
    ) -> Self {
        Self {
            generation,
            transaction,
        }
    }

    /// Insert a repository idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when the row violates the graph schema.
    pub fn insert_repository(&self, id: &RepositoryId, name: &str) -> Result<(), IndexError> {
        let changed = self.transaction.execute(
            "INSERT INTO repositories VALUES(?1, ?2, ?3)
             ON CONFLICT DO UPDATE SET repository_id = excluded.repository_id
             WHERE display_name = excluded.display_name",
            params![self.generation, id.as_str(), name],
        )?;
        ensure_idempotent(changed, "repository")
    }

    /// Insert a worktree revision idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when its repository is absent.
    pub fn insert_revision(
        &self,
        repository: &RepositoryId,
        revision: &WorktreeRevision,
    ) -> Result<(), IndexError> {
        let changed = self.transaction.execute(
            "INSERT INTO revisions VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT DO UPDATE SET revision = excluded.revision
             WHERE base_oid = excluded.base_oid
               AND dirty_digest IS excluded.dirty_digest",
            params![
                self.generation,
                repository.as_str(),
                revision.to_string(),
                revision.base_oid(),
                revision.dirty_digest()
            ],
        )?;
        ensure_idempotent(changed, "revision")
    }

    /// Insert a source file idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when its repository revision is absent.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_file(
        &self,
        id: &FileId,
        repository: &RepositoryId,
        revision: &WorktreeRevision,
        path: &str,
        language: &str,
        digest: &str,
    ) -> Result<(), IndexError> {
        insert_entity(self.transaction, self.generation, id.as_str(), "file")?;
        let changed = self.transaction.execute(
            "INSERT INTO files VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT DO UPDATE SET file_id = excluded.file_id
             WHERE file_id = excluded.file_id
               AND repository_id = excluded.repository_id
               AND revision = excluded.revision
               AND path = excluded.path
               AND language = excluded.language
               AND content_digest = excluded.content_digest",
            params![
                self.generation,
                id.as_str(),
                repository.as_str(),
                revision.to_string(),
                path,
                language,
                digest
            ],
        )?;
        ensure_idempotent(changed, "file")
    }

    /// Insert a symbol idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when its file is absent or its range is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_symbol(
        &self,
        id: &SymbolId,
        file: &FileId,
        name: &str,
        kind: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<(), IndexError> {
        insert_entity(self.transaction, self.generation, id.as_str(), "symbol")?;
        let changed = self.transaction.execute(
            "INSERT INTO symbols VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT DO UPDATE SET symbol_id = excluded.symbol_id
             WHERE file_id = excluded.file_id
               AND name = excluded.name
               AND symbol_kind = excluded.symbol_kind
               AND start_line = excluded.start_line
               AND end_line = excluded.end_line",
            params![
                self.generation,
                id.as_str(),
                file.as_str(),
                name,
                kind,
                start_line,
                end_line
            ],
        )?;
        ensure_idempotent(changed, "symbol")
    }

    /// Insert a text chunk idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when its file is absent or its range is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_chunk(
        &self,
        id: &ChunkId,
        file: &FileId,
        start_line: u32,
        end_line: u32,
        content: &str,
        digest: &str,
    ) -> Result<(), IndexError> {
        insert_entity(self.transaction, self.generation, id.as_str(), "chunk")?;
        let changed = self.transaction.execute(
            "INSERT INTO chunks VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT DO UPDATE SET chunk_id = excluded.chunk_id
             WHERE file_id = excluded.file_id
               AND start_line = excluded.start_line
               AND end_line = excluded.end_line
               AND content = excluded.content
               AND content_digest = excluded.content_digest",
            params![
                self.generation,
                id.as_str(),
                file.as_str(),
                start_line,
                end_line,
                content,
                digest
            ],
        )?;
        ensure_idempotent(changed, "chunk")
    }

    /// Insert one graph edge idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when a referenced local entity is absent.
    pub fn insert_edge(
        &self,
        kind: GraphEdgeKind,
        source: &str,
        target: EdgeTarget<'_>,
        source_line: u32,
    ) -> Result<(), IndexError> {
        let (entity, external) = match target {
            EdgeTarget::Entity(entity) => (Some(entity), None),
            EdgeTarget::External(external) => (None, Some(external)),
        };
        let changed = self.transaction.execute(
            "INSERT INTO edges(
                generation_id, edge_kind, source_id, target_entity_id,
                external_target, source_line
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT DO UPDATE SET source_id = excluded.source_id
             WHERE target_entity_id IS excluded.target_entity_id
               AND external_target IS excluded.external_target
               AND source_line = excluded.source_line",
            params![
                self.generation,
                kind.as_str(),
                source,
                entity,
                external,
                source_line
            ],
        )?;
        ensure_idempotent(changed, "edge")
    }

    /// Insert one nonfatal diagnostic idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when the diagnostic cannot be written.
    pub fn insert_diagnostic(
        &self,
        path: Option<&str>,
        code: &str,
        message: &str,
    ) -> Result<(), IndexError> {
        let changed = self.transaction.execute(
            "INSERT INTO diagnostics VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT DO UPDATE SET code = excluded.code
             WHERE path IS excluded.path AND message = excluded.message",
            params![self.generation, path, code, message],
        )?;
        ensure_idempotent(changed, "diagnostic")
    }

    /// Set generation metadata idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when the metadata cannot be written.
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), IndexError> {
        let changed = self.transaction.execute(
            "INSERT INTO metadata VALUES(?1, ?2, ?3)
             ON CONFLICT(generation_id, key) DO UPDATE SET key = excluded.key
             WHERE value = excluded.value",
            params![self.generation, key, value],
        )?;
        ensure_idempotent(changed, "metadata")
    }
}

fn insert_entity(
    connection: &Connection,
    generation: &str,
    id: &str,
    kind: &str,
) -> Result<(), IndexError> {
    let changed = connection.execute(
        "INSERT INTO entities VALUES(?1, ?2, ?3)
         ON CONFLICT DO UPDATE SET entity_id = excluded.entity_id
         WHERE entity_kind = excluded.entity_kind",
        params![generation, id, kind],
    )?;
    ensure_idempotent(changed, "entity")
}

const fn ensure_idempotent(changed: usize, entity: &'static str) -> Result<(), IndexError> {
    if changed == 0 {
        return Err(IndexError::Conflict(entity));
    }
    Ok(())
}
