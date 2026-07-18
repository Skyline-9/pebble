//! Transactional writes for a generation-local graph.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};

use crate::domain::{ChunkId, FileId, GenerationId, RepositoryId, SymbolId, WorktreeRevision};

use super::{EdgeTarget, GraphEdgeKind, GraphTransaction, IndexError, schema};

/// Transactional writer for a generation under construction.
pub struct GraphWriter {
    path: PathBuf,
    generation: GenerationId,
    connection: Connection,
}

impl GraphWriter {
    pub(super) fn create(path: PathBuf, generation: GenerationId) -> Result<Self, IndexError> {
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        schema::create(&connection, generation.as_str())?;
        Ok(Self {
            path,
            generation,
            connection,
        })
    }

    pub(super) fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Execute an explicit all-or-nothing graph transaction.
    ///
    /// # Errors
    ///
    /// Returns an error and rolls back when an operation or commit fails.
    pub fn transaction<F>(&self, operation: F) -> Result<(), IndexError>
    where
        F: FnOnce(&GraphTransaction<'_>) -> Result<(), IndexError>,
    {
        let transaction = self.connection.unchecked_transaction()?;
        operation(&GraphTransaction::new(
            self.generation.as_str(),
            &transaction,
        ))?;
        transaction.commit()?;
        Ok(())
    }

    /// Insert a repository idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when the row violates the graph schema.
    pub fn insert_repository(&self, id: &RepositoryId, name: &str) -> Result<(), IndexError> {
        self.transaction(|writer| writer.insert_repository(id, name))
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
        self.transaction(|writer| writer.insert_revision(repository, revision))
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
        self.transaction(|writer| {
            writer.insert_file(id, repository, revision, path, language, digest)
        })
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
        self.transaction(|writer| writer.insert_symbol(id, file, name, kind, start_line, end_line))
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
        self.transaction(|writer| {
            writer.insert_chunk(id, file, start_line, end_line, content, digest)
        })
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
        self.transaction(|writer| writer.insert_edge(kind, source, target, source_line))
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
        self.transaction(|writer| writer.insert_diagnostic(path, code, message))
    }

    /// Set generation metadata idempotently.
    ///
    /// # Errors
    ///
    /// Returns an error when the metadata cannot be written.
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), IndexError> {
        self.transaction(|writer| writer.set_metadata(key, value))
    }

    pub(super) fn flush(self) -> Result<PathBuf, IndexError> {
        self.connection.cache_flush()?;
        self.connection
            .close()
            .map_err(|(_, error)| IndexError::Sqlite(error))?;
        std::fs::OpenOptions::new()
            .read(true)
            .open(&self.path)?
            .sync_all()?;
        Ok(self.path)
    }
}
