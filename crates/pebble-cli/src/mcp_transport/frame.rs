use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::io::{AsyncRead, AsyncReadExt};

use super::MAX_FRAME_BYTES;

pub(super) struct FrameDecoder<R> {
    reader: R,
    pub(super) frame: Vec<u8>,
    chunk: [u8; 8 * 1024],
    start: usize,
    end: usize,
    oversized: Arc<AtomicBool>,
}

impl<R> FrameDecoder<R> {
    pub(super) fn new(reader: R, oversized: Arc<AtomicBool>) -> Self {
        Self {
            reader,
            frame: Vec::with_capacity(MAX_FRAME_BYTES),
            chunk: [0; 8 * 1024],
            start: 0,
            end: 0,
            oversized,
        }
    }
}

impl<R: AsyncRead + Unpin> FrameDecoder<R> {
    pub(super) async fn read_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            while self.start < self.end {
                let byte = self.chunk[self.start];
                self.start += 1;
                if self.frame.len() == MAX_FRAME_BYTES {
                    self.oversized.store(true, Ordering::Release);
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "MCP request exceeds 1 MiB",
                    ));
                }
                self.frame.push(byte);
                if byte == b'\n' {
                    self.frame.pop();
                    if self.frame.last() == Some(&b'\r') {
                        self.frame.pop();
                    }
                    return Ok(Some(std::mem::replace(
                        &mut self.frame,
                        Vec::with_capacity(MAX_FRAME_BYTES),
                    )));
                }
            }
            self.start = 0;
            self.end = self.reader.read(&mut self.chunk).await?;
            if self.end == 0 {
                return if self.frame.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(std::mem::replace(
                        &mut self.frame,
                        Vec::with_capacity(MAX_FRAME_BYTES),
                    )))
                };
            }
        }
    }
}
