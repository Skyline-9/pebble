//! Sealed, append-only flat-vector generation file format.
//!
//! Rows are fixed width: an eight-byte row ordinal followed by `dimension`
//! little-endian `f32` values. Pebble entity identities are content-derived
//! strings rather than integers, so a companion newline-delimited sidecar
//! records the stable entity ID for each row in the same order. Both files
//! are written once by [`VectorFileWriter`](crate::vectors::format::VectorFileWriter) and never mutated afterward. All
//! I/O uses plain buffered `std::fs`/`std::io` reads and writes; there is no
//! unsafe code and no memory mapping.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Length in bytes of a model fingerprint.
pub const FINGERPRINT_LEN: usize = 32;
/// Largest supported embedding dimension.
pub const MAX_DIMENSION: usize = 4_096;
/// Largest supported row count for one sealed vector generation.
pub const MAX_ROWS: u64 = 200_000;
/// Largest supported byte length of one stored entity ID.
pub const MAX_ENTITY_ID_LEN: usize = 512;

const MAGIC: [u8; 8] = *b"PBLVEC01";
const FORMAT_VERSION: u32 = 1;
const HEADER_LEN: u64 = 8 + 4 + FINGERPRINT_LEN as u64 + 4 + 8;
const ROW_COUNT_OFFSET: u64 = 8 + 4 + FINGERPRINT_LEN as u64 + 4;

/// Streaming writer for one sealed flat-vector generation.
pub struct VectorFileWriter {
    vector_file: File,
    ids_file: File,
    dimension: usize,
    fingerprint: [u8; FINGERPRINT_LEN],
    row_count: u64,
}

impl VectorFileWriter {
    /// Create a fresh sealed vector generation at `vector_path` with a
    /// companion entity-ID sidecar at `ids_path`. Neither path may already
    /// exist.
    ///
    /// # Errors
    ///
    /// Returns an error when the dimension is out of bounds or either file
    /// cannot be created.
    pub fn create(
        vector_path: &Path,
        ids_path: &Path,
        dimension: usize,
        fingerprint: [u8; FINGERPRINT_LEN],
    ) -> io::Result<Self> {
        if dimension == 0 || dimension > MAX_DIMENSION {
            return Err(invalid_data("vector dimension is out of bounds"));
        }
        let dimension_bytes = u32::try_from(dimension)
            .map_err(|_| invalid_data("vector dimension exceeds the header field width"))?
            .to_le_bytes();
        let mut vector_file = create_new(vector_path)?;
        vector_file.write_all(&MAGIC)?;
        vector_file.write_all(&FORMAT_VERSION.to_le_bytes())?;
        vector_file.write_all(&fingerprint)?;
        vector_file.write_all(&dimension_bytes)?;
        vector_file.write_all(&0_u64.to_le_bytes())?;
        let ids_file = create_new(ids_path)?;
        Ok(Self {
            vector_file,
            ids_file,
            dimension,
            fingerprint,
            row_count: 0,
        })
    }

    /// Return the embedding dimension fixed at creation.
    #[must_use]
    pub const fn dimension(&self) -> usize {
        self.dimension
    }

    /// Return the model fingerprint fixed at creation.
    #[must_use]
    pub const fn fingerprint(&self) -> [u8; FINGERPRINT_LEN] {
        self.fingerprint
    }

    /// Append one entity's embedding row.
    ///
    /// # Errors
    ///
    /// Returns an error when the entity ID is invalid, the vector length
    /// does not match the fixed dimension, the row bound is exceeded, or a
    /// write fails.
    pub fn write_row(&mut self, entity_id: &str, vector: &[f32]) -> io::Result<()> {
        validate_entity_id(entity_id)?;
        if vector.len() != self.dimension {
            return Err(invalid_data(
                "embedding length does not match the fixed dimension",
            ));
        }
        if self.row_count >= MAX_ROWS {
            return Err(invalid_data("vector generation row bound exceeded"));
        }
        self.vector_file.write_all(&self.row_count.to_le_bytes())?;
        for value in vector {
            self.vector_file.write_all(&value.to_le_bytes())?;
        }
        self.ids_file.write_all(entity_id.as_bytes())?;
        self.ids_file.write_all(b"\n")?;
        self.row_count += 1;
        Ok(())
    }

    /// Finalize the row count, then flush and durably sync both sealed
    /// files.
    ///
    /// # Errors
    ///
    /// Returns an error when the header cannot be rewritten or either file
    /// cannot be synced.
    pub fn finish(mut self) -> io::Result<()> {
        self.vector_file.seek(SeekFrom::Start(ROW_COUNT_OFFSET))?;
        self.vector_file.write_all(&self.row_count.to_le_bytes())?;
        self.vector_file.sync_all()?;
        self.ids_file.sync_all()?;
        Ok(())
    }
}

/// Read-only handle over one sealed, fully validated flat-vector generation.
pub struct VectorFileReader {
    file: RefCell<File>,
    dimension: usize,
    row_count: u64,
    entity_ids: Vec<String>,
}

impl VectorFileReader {
    /// Open and fully validate a sealed vector generation and its entity
    /// sidecar.
    ///
    /// # Errors
    ///
    /// Returns an error when either file is missing, a symbolic link,
    /// truncated, malformed, size-inconsistent with its header, or built
    /// with a model fingerprint other than `expected_fingerprint`.
    pub fn open(
        vector_path: &Path,
        ids_path: &Path,
        expected_fingerprint: [u8; FINGERPRINT_LEN],
    ) -> io::Result<Self> {
        reject_symlink(vector_path)?;
        reject_symlink(ids_path)?;
        let mut file = File::open(vector_path)?;
        let header_len = usize::try_from(HEADER_LEN)
            .map_err(|_| invalid_data("vector generation header length is invalid"))?;
        let mut header = vec![0_u8; header_len];
        file.read_exact(&mut header)
            .map_err(|_| invalid_data("vector generation header is truncated"))?;
        if header[0..8] != MAGIC {
            return Err(invalid_data("vector generation magic bytes are invalid"));
        }
        if u32::from_le_bytes(array4(&header[8..12])) != FORMAT_VERSION {
            return Err(invalid_data(
                "vector generation format version is unsupported",
            ));
        }
        let mut fingerprint = [0_u8; FINGERPRINT_LEN];
        fingerprint.copy_from_slice(&header[12..12 + FINGERPRINT_LEN]);
        if fingerprint != expected_fingerprint {
            return Err(invalid_data(
                "vector generation model fingerprint does not match",
            ));
        }
        let dimension_offset = 12 + FINGERPRINT_LEN;
        let dimension_value =
            u32::from_le_bytes(array4(&header[dimension_offset..dimension_offset + 4]));
        let dimension = usize::try_from(dimension_value)
            .map_err(|_| invalid_data("vector generation dimension is invalid"))?;
        if dimension == 0 || dimension > MAX_DIMENSION {
            return Err(invalid_data("vector generation dimension is out of bounds"));
        }
        let row_count_offset = dimension_offset + 4;
        let row_count = u64::from_le_bytes(array8(&header[row_count_offset..row_count_offset + 8]));
        if row_count > MAX_ROWS {
            return Err(invalid_data("vector generation row count is out of bounds"));
        }
        let row_length = row_byte_length(dimension)?;
        let expected_length = HEADER_LEN
            .checked_add(
                row_count
                    .checked_mul(row_length)
                    .ok_or_else(|| invalid_data("vector generation file length overflows"))?,
            )
            .ok_or_else(|| invalid_data("vector generation file length overflows"))?;
        if file.metadata()?.len() != expected_length {
            return Err(invalid_data(
                "vector generation file size is inconsistent with its header",
            ));
        }
        let entity_ids = read_entity_ids(ids_path, row_count)?;
        Ok(Self {
            file: RefCell::new(file),
            dimension,
            row_count,
            entity_ids,
        })
    }

    /// Return the fixed embedding dimension.
    #[must_use]
    pub const fn dimension(&self) -> usize {
        self.dimension
    }

    /// Return the validated row count.
    #[must_use]
    pub const fn row_count(&self) -> u64 {
        self.row_count
    }

    /// Return the stable entity ID stored for one row ordinal.
    #[must_use]
    pub fn entity_id(&self, ordinal: u64) -> Option<&str> {
        usize::try_from(ordinal)
            .ok()
            .and_then(|index| self.entity_ids.get(index))
            .map(String::as_str)
    }

    /// Read one row's embedding by ordinal, using a bounded seek and an
    /// exact read.
    ///
    /// # Errors
    ///
    /// Returns an error when the ordinal is out of bounds, the row's stored
    /// ordinal is inconsistent with its position, or the read fails.
    pub fn read_row(&self, ordinal: u64) -> io::Result<Vec<f32>> {
        if ordinal >= self.row_count {
            return Err(invalid_data("vector row ordinal is out of bounds"));
        }
        let row_length = row_byte_length(self.dimension)?;
        let offset = HEADER_LEN
            .checked_add(
                ordinal
                    .checked_mul(row_length)
                    .ok_or_else(|| invalid_data("vector row offset overflows"))?,
            )
            .ok_or_else(|| invalid_data("vector row offset overflows"))?;
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(offset))?;
        let mut key_bytes = [0_u8; 8];
        file.read_exact(&mut key_bytes)?;
        if u64::from_le_bytes(key_bytes) != ordinal {
            return Err(invalid_data(
                "vector row ordinal does not match its stored key",
            ));
        }
        let mut vector = Vec::with_capacity(self.dimension);
        let mut float_bytes = [0_u8; 4];
        for _ in 0..self.dimension {
            file.read_exact(&mut float_bytes)?;
            vector.push(f32::from_le_bytes(float_bytes));
        }
        Ok(vector)
    }
}

fn row_byte_length(dimension: usize) -> io::Result<u64> {
    let dimension = u64::try_from(dimension)
        .map_err(|_| invalid_data("vector generation dimension is invalid"))?;
    dimension
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(8))
        .ok_or_else(|| invalid_data("vector generation row length overflows"))
}

fn read_entity_ids(ids_path: &Path, row_count: u64) -> io::Result<Vec<String>> {
    reject_symlink(ids_path)?;
    let file = File::open(ids_path)?;
    let mut ids = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        validate_entity_id(&line)?;
        ids.push(line);
        if u64::try_from(ids.len()).unwrap_or(u64::MAX) > MAX_ROWS {
            return Err(invalid_data("vector entity sidecar row bound exceeded"));
        }
    }
    let actual = u64::try_from(ids.len())
        .map_err(|_| invalid_data("vector entity sidecar row count is invalid"))?;
    if actual != row_count {
        return Err(invalid_data(
            "vector entity sidecar row count does not match its header",
        ));
    }
    Ok(ids)
}

fn validate_entity_id(entity_id: &str) -> io::Result<()> {
    if entity_id.is_empty()
        || entity_id.len() > MAX_ENTITY_ID_LEN
        || !entity_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid_data("vector entity ID is invalid"));
    }
    Ok(())
}

fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    no_follow(&mut options);
    options.open(path)
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid_data(
            "vector generation path may not be a symbolic link",
        ));
    }
    Ok(())
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

const fn array4(bytes: &[u8]) -> [u8; 4] {
    let mut array = [0_u8; 4];
    array.copy_from_slice(bytes);
    array
}

const fn array8(bytes: &[u8]) -> [u8; 8] {
    let mut array = [0_u8; 8];
    array.copy_from_slice(bytes);
    array
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const fn no_follow_flag() -> i32 {
    0x2_0000
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
const fn no_follow_flag() -> i32 {
    0x100
}

#[cfg(unix)]
fn no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(no_follow_flag());
}

#[cfg(not(unix))]
fn no_follow(_options: &mut OpenOptions) {}
