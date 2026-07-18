//! Tantivy directory I/O rooted at an open directory capability.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tantivy::HasLen;
use tantivy::directory::error::{DeleteError, OpenReadError, OpenWriteError};
use tantivy::directory::{
    AntiCallToken, Directory, FileHandle, OwnedBytes, TerminatingWrite, WatchCallback, WatchHandle,
    WritePtr,
};

use super::IndexError;
use super::pinned_directory_platform::{descriptor_alias, no_follow, read_exact_at, same_identity};

#[derive(Clone)]
pub(super) struct PinnedDirectory {
    inner: Arc<Inner>,
}

struct Inner {
    directory: File,
    alias: PathBuf,
}

impl PinnedDirectory {
    pub(super) fn open(path: &Path) -> Result<Self, IndexError> {
        let expected = fs::symlink_metadata(path).map_err(IndexError::into_rebuild)?;
        if expected.file_type().is_symlink() || !expected.is_dir() {
            return Err(IndexError::rebuild(
                "lexical capability target is not a real directory",
            ));
        }
        Self::open_expected(path, &expected)
    }

    pub(super) fn open_expected(path: &Path, expected: &fs::Metadata) -> Result<Self, IndexError> {
        let directory = File::open(path).map_err(IndexError::into_rebuild)?;
        let opened = directory.metadata().map_err(IndexError::into_rebuild)?;
        if !opened.is_dir() || !same_identity(expected, &opened) {
            return Err(IndexError::rebuild(
                "lexical directory changed while acquiring its capability",
            ));
        }
        let alias = descriptor_alias(&directory, &opened, path)?;
        Ok(Self {
            inner: Arc::new(Inner { directory, alias }),
        })
    }

    fn resolve(path: &Path, root: &Path) -> io::Result<PathBuf> {
        let mut components = path.components();
        let Some(Component::Normal(name)) = components.next() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Tantivy path is not one plain relative filename",
            ));
        };
        if components.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Tantivy path contains nested or parent components",
            ));
        }
        Ok(root.join(name))
    }

    fn path(&self, relative: &Path) -> io::Result<PathBuf> {
        Self::resolve(relative, &self.inner.alias)
    }

    fn open_read_file(&self, relative: &Path) -> io::Result<File> {
        let full = self.path(relative)?;
        let mut options = OpenOptions::new();
        options.read(true);
        no_follow(&mut options);
        let file = options.open(full)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Tantivy entry is not a regular file",
            ));
        }
        Ok(file)
    }
}

impl fmt::Debug for PinnedDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PinnedDirectory")
            .field("alias", &self.inner.alias)
            .finish_non_exhaustive()
    }
}

impl Directory for PinnedDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let file = self
            .open_read_file(path)
            .map_err(|error| read_error(error, path))?;
        let handle = PinnedFile::new(file).map_err(|error| read_error(error, path))?;
        Ok(Arc::new(handle))
    }

    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        let full = self.path(path).map_err(|error| delete_error(error, path))?;
        fs::remove_file(full).map_err(|error| delete_error(error, path))
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        let full = self.path(path).map_err(|error| read_error(error, path))?;
        match fs::symlink_metadata(full) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(read_error(error, path)),
        }
    }

    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        let full = self.path(path).map_err(|error| write_error(error, path))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        no_follow(&mut options);
        let file = options
            .open(full)
            .map_err(|error| write_error(error, path))?;
        Ok(io::BufWriter::new(Box::new(DurableWriter(file))))
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        let mut file = self
            .open_read_file(path)
            .map_err(|error| read_error(error, path))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| read_error(error, path))?;
        Ok(bytes)
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let destination = self.path(path)?;
        let temporary = self.temporary_path()?;
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            no_follow(&mut options);
            let mut file = options.open(&temporary)?;
            file.write_all(data)?;
            file.sync_data()?;
            fs::rename(&temporary, destination)?;
            self.sync_directory()
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    fn sync_directory(&self) -> io::Result<()> {
        self.inner.directory.sync_all()
    }

    fn watch(&self, _callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        Ok(WatchHandle::empty())
    }
}

impl PinnedDirectory {
    fn temporary_path(&self) -> io::Result<PathBuf> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let name = format!(".pebble-tantivy-{}-{sequence}.tmp", std::process::id());
            let path = self.inner.alias.join(name);
            if !path.try_exists()? {
                return Ok(path);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "unable to allocate a Tantivy atomic-write temporary",
        ))
    }
}

struct DurableWriter(File);

impl Write for DurableWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl TerminatingWrite for DurableWriter {
    fn terminate_ref(&mut self, _token: AntiCallToken) -> io::Result<()> {
        self.0.flush()?;
        self.0.sync_data()
    }
}

#[derive(Debug)]
struct PinnedFile {
    file: File,
    length: usize,
}

impl PinnedFile {
    fn new(file: File) -> io::Result<Self> {
        let length = usize::try_from(file.metadata()?.len())
            .map_err(|_| io::Error::other("Tantivy file exceeds addressable memory"))?;
        Ok(Self { file, length })
    }
}

impl HasLen for PinnedFile {
    fn len(&self) -> usize {
        self.length
    }
}

impl FileHandle for PinnedFile {
    fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
        let reversed = range.start > range.end;
        let beyond_file = range.end > self.length;
        if reversed || beyond_file {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Tantivy read range exceeds its pinned file",
            ));
        }
        let mut bytes = vec![0; range.len()];
        read_exact_at(&self.file, &mut bytes, range.start)?;
        Ok(OwnedBytes::new(bytes))
    }
}

fn read_error(error: io::Error, path: &Path) -> OpenReadError {
    if error.kind() == io::ErrorKind::NotFound {
        OpenReadError::FileDoesNotExist(path.to_owned())
    } else {
        OpenReadError::wrap_io_error(error, path.to_owned())
    }
}

fn write_error(error: io::Error, path: &Path) -> OpenWriteError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        OpenWriteError::FileAlreadyExists(path.to_owned())
    } else {
        OpenWriteError::wrap_io_error(error, path.to_owned())
    }
}

fn delete_error(error: io::Error, path: &Path) -> DeleteError {
    if error.kind() == io::ErrorKind::NotFound {
        DeleteError::FileDoesNotExist(path.to_owned())
    } else {
        DeleteError::IoError {
            io_error: Arc::new(error),
            filepath: path.to_owned(),
        }
    }
}
