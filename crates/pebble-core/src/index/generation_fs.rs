//! Filesystem boundary checks for immutable generation graphs.

use std::fs::{self, Metadata, OpenOptions};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use super::{IndexError, schema};

const GRAPH_FILE: &str = "graph.db";
const LEXICAL_DIRECTORY: &str = "lexical";

pub(super) struct RootBoundary {
    path: PathBuf,
    canonical: PathBuf,
    metadata: Metadata,
}

impl RootBoundary {
    pub(super) fn inspect(path: &Path) -> Result<Self, IndexError> {
        Ok(Self {
            path: path.to_owned(),
            canonical: canonical(path)?,
            metadata: real_directory(path, "generation root is invalid")?,
        })
    }

    pub(super) fn verify(&self) -> Result<(), IndexError> {
        let metadata = real_directory(&self.path, "generation root changed")?;
        if canonical(&self.path)? != self.canonical || !same_identity(&self.metadata, &metadata) {
            return Err(invalid("generation root identity changed"));
        }
        Ok(())
    }
}

pub(super) struct GraphBoundary {
    root: RootBoundary,
    directory: PathBuf,
    directory_canonical: PathBuf,
    directory_metadata: Metadata,
    graph: PathBuf,
    graph_metadata: Metadata,
    lexical: PathBuf,
    lexical_canonical: PathBuf,
    lexical_metadata: Metadata,
    lexical_directory: super::pinned_directory::PinnedDirectory,
    lexical_files: Vec<(PathBuf, Metadata)>,
}

impl GraphBoundary {
    pub(super) fn inspect(root: &Path, generation: &str) -> Result<Self, IndexError> {
        let directory = root.join(generation);
        let root = RootBoundary::inspect(root)?;
        let directory_metadata =
            real_directory(&directory, "selected generation is not a real directory")?;
        let directory_canonical = canonical(&directory)?;
        if directory_canonical.parent() != Some(root.canonical.as_path()) {
            return Err(invalid("selected generation escapes its root"));
        }
        let graph = directory.join(GRAPH_FILE);
        let graph_metadata = real_file(&graph, "generation graph is not a real file")?;
        if canonical(&graph)?.parent() != Some(directory_canonical.as_path()) {
            return Err(invalid("generation graph escapes its directory"));
        }
        let lexical = directory.join(LEXICAL_DIRECTORY);
        let lexical_metadata =
            real_directory(&lexical, "generation lexical index is not a real directory")?;
        let lexical_canonical = canonical(&lexical)?;
        if lexical_canonical.parent() != Some(directory_canonical.as_path()) {
            return Err(invalid("generation lexical index escapes its directory"));
        }
        let lexical_directory =
            super::pinned_directory::PinnedDirectory::open_expected(&lexical, &lexical_metadata)?;
        let lexical_files = lexical_files(&lexical, &lexical_canonical)?;
        reject_sidecars(&graph)?;
        Ok(Self {
            root,
            directory,
            directory_canonical,
            directory_metadata,
            graph,
            graph_metadata,
            lexical,
            lexical_canonical,
            lexical_metadata,
            lexical_directory,
            lexical_files,
        })
    }

    pub(super) fn validate(&self, generation: &str) -> Result<(), IndexError> {
        let connection = open_connection(&self.graph)?;
        schema::validate(&connection, generation).map_err(IndexError::into_rebuild)?;
        self.verify()
    }

    pub(super) fn verify(&self) -> Result<(), IndexError> {
        self.root.verify()?;
        let directory_metadata = real_directory(&self.directory, "selected generation changed")?;
        let graph_metadata = real_file(&self.graph, "generation graph changed")?;
        let lexical_metadata = real_directory(&self.lexical, "generation lexical index changed")?;
        if canonical(&self.directory)? != self.directory_canonical
            || canonical(&self.graph)?.parent() != Some(self.directory_canonical.as_path())
            || canonical(&self.lexical)? != self.lexical_canonical
            || !same_identity(&self.directory_metadata, &directory_metadata)
            || !same_identity(&self.graph_metadata, &graph_metadata)
            || !same_identity(&self.lexical_metadata, &lexical_metadata)
            || !same_files(
                &self.lexical_files,
                &lexical_files(&self.lexical, &self.lexical_canonical)?,
            )
        {
            return Err(invalid("generation filesystem identity changed"));
        }
        reject_sidecars(&self.graph)
    }

    pub(super) fn graph(&self) -> &Path {
        &self.graph
    }

    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(super) fn lexical(&self) -> &Path {
        &self.lexical
    }

    pub(super) fn lexical_directory(&self) -> super::pinned_directory::PinnedDirectory {
        self.lexical_directory.clone()
    }
}

pub(super) fn open_reader(
    boundary: &GraphBoundary,
    generation: &str,
) -> Result<super::GraphReader, IndexError> {
    let reader =
        super::GraphReader::open(boundary.graph(), generation).map_err(IndexError::into_rebuild)?;
    reader.validate().map_err(IndexError::into_rebuild)?;
    boundary.verify()?;
    Ok(reader)
}

fn open_connection(path: &Path) -> Result<Connection, IndexError> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(IndexError::into_rebuild)
}

pub(super) fn reject_graph_sidecars(graph: &Path) -> Result<(), IndexError> {
    reject_sidecars(graph)
}

fn real_directory(path: &Path, message: &'static str) -> Result<Metadata, IndexError> {
    let metadata = fs::symlink_metadata(path).map_err(IndexError::into_rebuild)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid(message));
    }
    Ok(metadata)
}

fn real_file(path: &Path, message: &'static str) -> Result<Metadata, IndexError> {
    let metadata = fs::symlink_metadata(path).map_err(IndexError::into_rebuild)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid(message));
    }
    Ok(metadata)
}

fn lexical_files(
    directory: &Path,
    canonical_directory: &Path,
) -> Result<Vec<(PathBuf, Metadata)>, IndexError> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(IndexError::into_rebuild)? {
        let entry = entry.map_err(IndexError::into_rebuild)?;
        let path = entry.path();
        let metadata = real_file(&path, "lexical index entry is not a real file")?;
        if canonical(&path)?.parent() != Some(canonical_directory) {
            return Err(invalid("lexical index entry escapes its directory"));
        }
        files.push((path, metadata));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn same_files(left: &[(PathBuf, Metadata)], right: &[(PathBuf, Metadata)]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|((left_path, left_meta), (right_path, right_meta))| {
                left_path == right_path && same_identity(left_meta, right_meta)
            })
}

fn reject_sidecars(graph: &Path) -> Result<(), IndexError> {
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = graph.as_os_str().to_owned();
        sidecar.push(suffix);
        match fs::symlink_metadata(PathBuf::from(sidecar)) {
            Ok(_) => return Err(invalid("SQLite graph sidecars are not allowed")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(IndexError::into_rebuild(error)),
        }
    }
    Ok(())
}

fn canonical(path: &Path) -> Result<PathBuf, IndexError> {
    path.canonicalize().map_err(IndexError::into_rebuild)
}

fn invalid(message: &'static str) -> IndexError {
    IndexError::rebuild(message)
}

#[cfg(unix)]
fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino() && left.file_type() == right.file_type()
}

#[cfg(not(unix))]
fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    left.file_type() == right.file_type()
        && left.created().ok() == right.created().ok()
        && left.modified().ok() == right.modified().ok()
}

pub(super) fn open_regular_new(path: &Path) -> Result<fs::File, std::io::Error> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(super::generation::no_follow_flag());
    }
    options.open(path)
}
