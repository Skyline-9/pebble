//! Generation-local `SQLite` graph reads and writes.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::IndexError;

const MAX_GRAPH_READ_ROWS: usize = 10_000;

/// Validated maximum number of rows returned by one graph read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphReadLimit(i64);

impl TryFrom<usize> for GraphReadLimit {
    type Error = IndexError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if !(1..=MAX_GRAPH_READ_ROWS).contains(&value) {
            return Err(IndexError::rebuild(
                "graph read limit must be between 1 and 10000",
            ));
        }
        let value =
            i64::try_from(value).map_err(|_| IndexError::rebuild("graph read limit is invalid"))?;
        Ok(Self(value))
    }
}

impl GraphReadLimit {
    pub(super) const fn sql(self) -> i64 {
        self.0
    }
}

/// One of the structural edge kinds stored in the evidence graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphEdgeKind {
    /// A file defines a symbol.
    Defines,
    /// Source references a symbol.
    References,
    /// Source imports an external module or entity.
    Imports,
    /// Source calls an entity.
    Calls,
    /// A source entity contains another entity.
    Contains,
}

impl GraphEdgeKind {
    /// Every supported graph edge kind.
    pub const ALL: [Self; 5] = [
        Self::Defines,
        Self::References,
        Self::Imports,
        Self::Calls,
        Self::Contains,
    ];

    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Defines => "defines",
            Self::References => "references",
            Self::Imports => "imports",
            Self::Calls => "calls",
            Self::Contains => "contains",
        }
    }
}

/// Entity-backed or unresolved external destination of a graph edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeTarget<'target> {
    /// A generation-local graph entity.
    Entity(&'target str),
    /// A target not resolved in this generation.
    External(&'target str),
}

/// One graph edge returned by a generation reader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphEdge {
    kind: GraphEdgeKind,
    source: String,
    target: String,
    target_is_external: bool,
    source_line: u32,
}

impl GraphEdge {
    /// Return the edge kind.
    #[must_use]
    pub const fn kind(&self) -> GraphEdgeKind {
        self.kind
    }

    /// Return the source entity ID.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Return the destination entity ID or external target.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Return whether the destination is unresolved external text.
    #[must_use]
    pub const fn target_is_external(&self) -> bool {
        self.target_is_external
    }

    /// Return the one-based source line.
    #[must_use]
    pub const fn source_line(&self) -> u32 {
        self.source_line
    }
}

/// Validated row totals for one generation-local graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphCounts {
    repositories: u64,
    revisions: u64,
    files: u64,
    symbols: u64,
    chunks: u64,
    edges: u64,
    diagnostics: u64,
    pub(super) entities: u64,
}

macro_rules! count_accessors {
    ($($name:ident),+ $(,)?) => {$(
        #[doc = concat!("Return the number of ", stringify!($name), " rows.")]
        #[must_use]
        pub const fn $name(&self) -> u64 { self.$name }
    )+};
}

impl GraphCounts {
    count_accessors!(
        repositories,
        revisions,
        files,
        symbols,
        chunks,
        edges,
        diagnostics
    );

    pub(super) fn load(connection: &Connection, generation: &str) -> Result<Self, IndexError> {
        Ok(Self {
            repositories: count(connection, "repositories", generation)?,
            revisions: count(connection, "revisions", generation)?,
            files: count(connection, "files", generation)?,
            symbols: count(connection, "symbols", generation)?,
            chunks: count(connection, "chunks", generation)?,
            edges: count(connection, "edges", generation)?,
            diagnostics: count(connection, "diagnostics", generation)?,
            entities: count(connection, "entities", generation)?,
        })
    }
}

/// Read-only query handle pinned to one generation graph.
pub struct GraphReader {
    path: PathBuf,
    pub(super) generation: String,
    pub(super) connection: Connection,
}

impl GraphReader {
    pub(super) fn open(path: &Path, generation: &str) -> Result<Self, IndexError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        connection.pragma_update(None, "query_only", true)?;
        Ok(Self {
            path: path.to_owned(),
            generation: generation.to_owned(),
            connection,
        })
    }

    /// Return validated row counts.
    ///
    /// # Errors
    ///
    /// Returns an error when the graph cannot be queried.
    pub fn counts(&self) -> Result<GraphCounts, IndexError> {
        GraphCounts::load(&self.connection, &self.generation)
    }

    /// Return the metadata value associated with a key.
    ///
    /// # Errors
    ///
    /// Returns an error when the graph cannot be queried.
    pub fn metadata(&self, key: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE generation_id = ?1 AND key = ?2",
                params![self.generation, key],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Return a bounded set of edges of one kind in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns an error when graph rows cannot be read.
    pub fn edges(
        &self,
        kind: GraphEdgeKind,
        limit: GraphReadLimit,
    ) -> Result<Vec<GraphEdge>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT source_id, target_entity_id, external_target, source_line
             FROM edges WHERE generation_id = ?1 AND edge_kind = ?2
             ORDER BY source_id, source_line, COALESCE(target_entity_id, external_target)
             LIMIT ?3",
        )?;
        let rows =
            statement.query_map(params![self.generation, kind.as_str(), limit.0], |row| {
                let entity: Option<String> = row.get(1)?;
                let external: Option<String> = row.get(2)?;
                Ok(GraphEdge {
                    kind,
                    source: row.get(0)?,
                    target: entity.clone().or(external).unwrap_or_default(),
                    target_is_external: entity.is_none(),
                    source_line: row.get(3)?,
                })
            })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Return whether `SQLite` opened the main database read-only.
    ///
    /// # Errors
    ///
    /// Returns an error when the database mode cannot be inspected.
    pub fn is_read_only(&self) -> Result<bool, IndexError> {
        Ok(self.connection.is_readonly("main")?)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn validate(&self) -> Result<(), IndexError> {
        super::schema::validate(&self.connection, &self.generation)
    }
}

fn count(connection: &Connection, table: &str, generation: &str) -> Result<u64, IndexError> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE generation_id = ?1");
    let value = connection.query_row(&sql, [generation], |row| row.get::<_, i64>(0))?;
    u64::try_from(value).map_err(|_| IndexError::rebuild("negative graph row count"))
}
