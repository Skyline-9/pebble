//! Immutable generation build, seal, activation, and recovery lifecycle.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use crate::domain::GenerationId;
use crate::vectors::{FlatVectorIndex, TextEmbedder, format::VectorFileWriter};

use super::building_boundary::BuildingBoundary;
use super::generation_fs::{GraphBoundary, RootBoundary, open_reader, reject_graph_sidecars};
use super::{GraphReader, GraphWriter, IndexError, LexicalReader, LexicalWriter};

const GRAPH_FILE: &str = "graph.db";
const LEXICAL_DIRECTORY: &str = "lexical";
const VECTORS_DIRECTORY: &str = "vectors";
const VECTORS_FILE: &str = "generation.vec";
const VECTORS_IDS_FILE: &str = "generation.ids";
/// Mutable handle for a generation being built outside query visibility.
pub struct GenerationBuilder {
    root: PathBuf,
    id: GenerationId,
    building: PathBuf,
    graph: GraphWriter,
    lexical: LexicalWriter,
    vectors: Option<VectorFileWriter>,
}

impl GenerationBuilder {
    /// Create a fresh `<id>.building` generation and schema.
    ///
    /// # Errors
    ///
    /// Returns an error when the generation already exists or local I/O fails.
    pub fn create(root: &Path, id: GenerationId) -> Result<Self, IndexError> {
        Self::create_with_embedder(root, id, None)
    }

    /// Create a fresh `<id>.building` generation and schema, optionally
    /// including a sealed vector generation streamed from `embedder`.
    ///
    /// # Errors
    ///
    /// Returns an error when the generation already exists or local I/O fails.
    pub fn create_with_embedder(
        root: &Path,
        id: GenerationId,
        embedder: Option<&dyn TextEmbedder>,
    ) -> Result<Self, IndexError> {
        fs::create_dir_all(root)?;
        let root_boundary = RootBoundary::inspect(root)?;
        let building = root.join(format!("{}.building", id.as_str()));
        if let Err(error) = fs::create_dir(&building) {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                return Err(IndexError::IncompleteBuild {
                    generation: id.as_str().to_owned(),
                });
            }
            return Err(error.into());
        }
        sync_directory(root)?;
        super::generation_races::run(
            super::generation_races::RacePoint::BuildingCreated,
            root,
            &building,
        );
        root_boundary.verify()?;
        let building_boundary = BuildingBoundary::inspect(root, &building)?;
        let graph = GraphWriter::create(building.join(GRAPH_FILE), id.clone())?;
        building_boundary.verify()?;
        let lexical_path = building.join(LEXICAL_DIRECTORY);
        fs::create_dir(&lexical_path)?;
        super::generation_races::run(
            super::generation_races::RacePoint::LexicalDirectoryCreated,
            &building,
            &lexical_path,
        );
        let lexical_boundary = building_boundary.pin_lexical(&lexical_path)?;
        let lexical = LexicalWriter::create_pinned(&lexical_path, lexical_boundary.directory())?;
        super::generation_races::run(
            super::generation_races::RacePoint::LexicalWriterCreated,
            &building,
            &lexical_path,
        );
        lexical_boundary.verify()?;
        building_boundary.verify()?;
        let vectors = match embedder {
            Some(embedder) => {
                let vectors_path = building.join(VECTORS_DIRECTORY);
                fs::create_dir(&vectors_path)?;
                building_boundary.verify()?;
                let writer = VectorFileWriter::create(
                    &vectors_path.join(VECTORS_FILE),
                    &vectors_path.join(VECTORS_IDS_FILE),
                    embedder.dimension(),
                    embedder.fingerprint(),
                )?;
                Some(writer)
            }
            None => None,
        };
        building_boundary.verify()?;
        root_boundary.verify()?;
        Ok(Self {
            root: root.to_owned(),
            id,
            building,
            graph,
            lexical,
            vectors,
        })
    }

    /// Return the transactional `SQLite` graph writer.
    #[must_use]
    pub const fn graph(&self) -> &GraphWriter {
        &self.graph
    }

    /// Return the generation's `SQLite` path while it is under construction.
    #[must_use]
    pub fn graph_path(&self) -> &Path {
        self.graph.path()
    }

    /// Return the bounded Tantivy writer for this building generation.
    #[must_use]
    pub const fn lexical(&mut self) -> &mut LexicalWriter {
        &mut self.lexical
    }

    /// Return the bounded vector-generation writer for this building
    /// generation, when an embedder was supplied at creation.
    #[must_use]
    pub const fn vectors(&mut self) -> Option<&mut VectorFileWriter> {
        self.vectors.as_mut()
    }

    /// Flush, validate, and rename this generation without activating it.
    ///
    /// # Errors
    ///
    /// Returns rebuild-required for schema, integrity, ownership, or row-count
    /// validation failures. `CURRENT` remains unchanged on every failure.
    pub fn seal(self) -> Result<SealedGeneration, IndexError> {
        let Self {
            root,
            id,
            building,
            graph,
            lexical,
            vectors,
        } = self;
        let root_boundary = RootBoundary::inspect(&root)?;
        let lexical = lexical.finish()?;
        let graph_path = graph.flush()?;
        reject_graph_sidecars(&graph_path)?;
        let building_name = format!("{}.building", id.as_str());
        let boundary = GraphBoundary::inspect(&root, &building_name)?;
        boundary.validate(id.as_str())?;
        let graph = open_reader(&boundary, id.as_str())?;
        if graph.metadata("compiler")?.as_deref() == Some("pebble-core") {
            super::cross_index::validate(&graph, &lexical)?;
        }
        if let Some(vectors) = vectors {
            let fingerprint = vectors.fingerprint();
            vectors.finish()?;
            let vectors_path = building.join(VECTORS_DIRECTORY);
            FlatVectorIndex::open(
                &vectors_path.join(VECTORS_FILE),
                &vectors_path.join(VECTORS_IDS_FILE),
                fingerprint,
            )?;
        }
        sync_directory(&building)?;
        boundary.verify()?;
        root_boundary.verify()?;
        let sealed = root.join(id.as_str());
        fs::rename(&building, &sealed)?;
        sync_directory(&root)?;
        root_boundary.verify()?;
        Ok(SealedGeneration { root, id })
    }
}

/// Validated immutable generation that is not necessarily current.
pub struct SealedGeneration {
    root: PathBuf,
    id: GenerationId,
}

impl SealedGeneration {
    /// Atomically switch `CURRENT` to this validated generation.
    ///
    /// # Errors
    ///
    /// Returns an error when the durable temporary write, rename, or reader
    /// open fails.
    pub fn activate(self) -> Result<GenerationReader, IndexError> {
        let reader = GenerationReader::open(&self.root, self.id)?;
        let _lock = super::current::lock(&self.root)?;
        let previous = super::current::snapshot(&self.root);
        reader.verify()?;
        let bytes = format!("{}\n", reader.id()).into_bytes();
        super::current::publish(&self.root, &bytes, &previous)?;
        super::generation_races::run(
            super::generation_races::RacePoint::GenerationPublished,
            &self.root,
            reader.directory(),
        );
        if let Err(error) = reader.verify() {
            super::current::restore(&self.root, &previous)?;
            return Err(error);
        }
        Ok(reader)
    }
}

/// Read-only graph handle pinned to one immutable generation.
pub struct GenerationReader {
    id: GenerationId,
    directory: PathBuf,
    graph: GraphReader,
    lexical: LexicalReader,
    boundary: GraphBoundary,
    vectors: Option<(PathBuf, PathBuf)>,
}

impl GenerationReader {
    /// Recover startup state and open the generation named by `CURRENT`.
    ///
    /// # Errors
    ///
    /// Returns rebuild-required when `CURRENT` or the selected generation is
    /// missing, malformed, corrupt, or inconsistent.
    pub fn open_current(root: &Path) -> Result<Self, IndexError> {
        let bytes = super::current::read(root)?;
        let id = parse_current(&bytes)?;
        Self::open(root, id)
    }

    /// Open one sealed generation read-only.
    ///
    /// # Errors
    ///
    /// Returns rebuild-required when its graph is missing or invalid.
    pub fn open(root: &Path, id: GenerationId) -> Result<Self, IndexError> {
        let boundary = GraphBoundary::inspect(root, id.as_str())?;
        boundary.validate(id.as_str())?;
        let graph = open_reader(&boundary, id.as_str())?;
        super::generation_races::run(
            super::generation_races::RacePoint::LexicalReaderOpen,
            boundary.directory(),
            boundary.lexical(),
        );
        let lexical = LexicalReader::open_pinned(boundary.lexical_directory())
            .map_err(IndexError::into_rebuild)?;
        if graph.metadata("compiler")?.as_deref() == Some("pebble-core") {
            super::cross_index::validate(&graph, &lexical)?;
        }
        boundary.verify()?;
        let vectors = detect_vectors(boundary.directory());
        Ok(Self {
            id,
            directory: boundary.directory().to_owned(),
            graph,
            lexical,
            boundary,
            vectors,
        })
    }

    /// Return the pinned generation ID.
    #[must_use]
    pub const fn id(&self) -> &GenerationId {
        &self.id
    }

    /// Return the read-only graph.
    #[must_use]
    pub const fn graph(&self) -> &GraphReader {
        &self.graph
    }

    /// Return the sealed `SQLite` graph path.
    #[must_use]
    pub fn graph_path(&self) -> &Path {
        self.graph.path()
    }

    /// Return the read-only lexical index pinned to this generation.
    #[must_use]
    pub const fn lexical(&self) -> &LexicalReader {
        &self.lexical
    }

    /// Return the pinned generation directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Return the sealed vector generation's file and entity-sidecar paths,
    /// when a vector index was built for this generation.
    #[must_use]
    pub fn vectors_paths(&self) -> Option<(&Path, &Path)> {
        self.vectors
            .as_ref()
            .map(|(vector, ids)| (vector.as_path(), ids.as_path()))
    }

    pub(super) fn verify(&self) -> Result<(), IndexError> {
        self.boundary.verify()
    }
}

fn detect_vectors(directory: &Path) -> Option<(PathBuf, PathBuf)> {
    let vector_path = directory.join(VECTORS_DIRECTORY).join(VECTORS_FILE);
    let ids_path = directory.join(VECTORS_DIRECTORY).join(VECTORS_IDS_FILE);
    if is_regular_file(&vector_path) && is_regular_file(&ids_path) {
        Some((vector_path, ids_path))
    } else {
        None
    }
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
}

pub(super) fn parse_current(bytes: &[u8]) -> Result<GenerationId, IndexError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| IndexError::rebuild("CURRENT is not UTF-8"))?;
    let value = text
        .strip_suffix('\n')
        .filter(|value| !value.contains('\n'))
        .ok_or_else(|| IndexError::rebuild("CURRENT has invalid framing"))?;
    GenerationId::try_from(value.to_owned())
        .map_err(|_| IndexError::rebuild("CURRENT has an invalid generation ID"))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) const fn no_follow_flag() -> i32 {
    0x2_0000
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
pub(super) const fn no_follow_flag() -> i32 {
    0x100
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), IndexError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), IndexError> {
    Ok(())
}
