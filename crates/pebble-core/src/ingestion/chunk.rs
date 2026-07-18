//! Deterministic bounded source chunking.

use crate::domain::{ChunkId, FileId};

/// Maximum UTF-8 byte length of one source chunk.
pub const MAX_CHUNK_BYTES: usize = 8 * 1024;

/// One bounded, line-addressable source excerpt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chunk {
    id: ChunkId,
    start_line: u32,
    end_line: u32,
    text: String,
}

impl Chunk {
    /// Return the deterministic chunk identity.
    #[must_use]
    pub const fn id(&self) -> &ChunkId {
        &self.id
    }

    /// Return the one-based inclusive starting line.
    #[must_use]
    pub const fn start_line(&self) -> u32 {
        self.start_line
    }

    /// Return the one-based inclusive ending line.
    #[must_use]
    pub const fn end_line(&self) -> u32 {
        self.end_line
    }

    /// Return the exact source excerpt.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

pub(super) fn chunks(file: &FileId, source: &str) -> Vec<Chunk> {
    let mut output = Vec::new();
    let mut pending = String::new();
    let mut start_line = 1_u32;
    let mut line_number = 1_u32;
    for line in source.split_inclusive('\n') {
        if line.trim().is_empty() {
            flush(
                file,
                &mut output,
                &mut pending,
                start_line,
                line_number.saturating_sub(1),
            );
        } else if pending.len().saturating_add(line.len()) <= MAX_CHUNK_BYTES {
            if pending.is_empty() {
                start_line = line_number;
            }
            pending.push_str(line);
        } else {
            flush(
                file,
                &mut output,
                &mut pending,
                start_line,
                line_number.saturating_sub(1),
            );
            split_line(file, &mut output, line, line_number);
        }
        line_number = line_number.saturating_add(1);
    }
    flush(
        file,
        &mut output,
        &mut pending,
        start_line,
        line_number.saturating_sub(1),
    );
    if output.is_empty() {
        output.push(make_chunk(file, 1, 1, 0, ""));
    }
    output
}

fn split_line(file: &FileId, output: &mut Vec<Chunk>, mut line: &str, line_number: u32) {
    while !line.is_empty() {
        let end = boundary(line, MAX_CHUNK_BYTES);
        let (part, remainder) = line.split_at(end);
        output.push(make_chunk(
            file,
            line_number,
            line_number,
            output.len(),
            part,
        ));
        line = remainder;
    }
}

const fn boundary(text: &str, maximum: usize) -> usize {
    if text.len() <= maximum {
        return text.len();
    }
    let mut end = maximum;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    end
}

fn flush(
    file: &FileId,
    output: &mut Vec<Chunk>,
    pending: &mut String,
    start_line: u32,
    end_line: u32,
) {
    if !pending.is_empty() {
        let ordinal = output.len();
        output.push(make_chunk(
            file,
            start_line,
            end_line.max(start_line),
            ordinal,
            pending,
        ));
        pending.clear();
    }
}

fn make_chunk(file: &FileId, start_line: u32, end_line: u32, ordinal: usize, text: &str) -> Chunk {
    let digest = blake3::hash(text.as_bytes()).to_hex();
    Chunk {
        id: ChunkId::derive(file, start_line, end_line, ordinal, digest.as_str()),
        start_line,
        end_line,
        text: text.to_owned(),
    }
}
