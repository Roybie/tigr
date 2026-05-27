//! Streaming file handle — the resource behind `IO.open` and the
//! handle-based reads/writes added alongside `IO.read_file` &c.
//!
//! Modelled on [`crate::vm::socket::SocketInner`] at a much smaller
//! scale: an `Arc`-shared, `Send + Sync` handle that lives outside the
//! GC heap, with interior mutability per field. The same over-read
//! buffer pattern serves `read_line` / `read_until` / `read_exact`.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A shared, `Send` file handle. Cloning bumps the `Arc` refcount.
pub type FileHandle = Arc<FileInner>;

/// How the file was opened. Fixed at construction; reads on a write
/// handle (and vice-versa) raise [`FileError::WrongMode`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    Read,
    Write,
    Append,
}

impl FileMode {
    pub fn parse(s: &str) -> Option<FileMode> {
        match s {
            "r" => Some(FileMode::Read),
            "w" => Some(FileMode::Write),
            "a" => Some(FileMode::Append),
            _ => None,
        }
    }

    fn is_read(self) -> bool {
        matches!(self, FileMode::Read)
    }

    fn is_write(self) -> bool {
        matches!(self, FileMode::Write | FileMode::Append)
    }
}

/// An operation failure, mapped to a structured tigr error by the IO
/// module. Mirrors the role of [`crate::vm::socket::NetError`].
pub enum FileError {
    /// The handle was `close`d.
    Closed,
    /// Op is not legal for this handle's mode (read on write-only, etc).
    WrongMode(&'static str),
    /// `read_exact` ran out of bytes before reading `n`.
    Eof(String),
    /// `read_line` decoded invalid UTF-8.
    Decode(String),
    /// `open` got an unknown mode string.
    InvalidMode(String),
    /// Any other OS-level I/O error.
    Io(io::Error),
}

impl From<io::Error> for FileError {
    fn from(e: io::Error) -> Self {
        FileError::Io(e)
    }
}

/// Per-syscall read size for the buffer-fill helper.
const CHUNK: usize = 8192;
/// Cap on a single `read_chunk` syscall — a huge `n` must not allocate
/// gigabytes up front. A short read is valid; the caller loops.
const MAX_DIRECT_READ: usize = 65536;

pub struct FileInner {
    mode: FileMode,
    file: Mutex<File>,
    /// Surplus bytes pulled past what `read_until` / `read_line` needed.
    /// Only used in read mode; always drained before another syscall.
    read_buf: Mutex<Vec<u8>>,
    closed: AtomicBool,
    /// Display only — the path the handle was opened with.
    path: String,
    /// Stable id for legible display; identity is `Arc::ptr_eq`.
    id: u64,
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl FileInner {
    pub fn open(path: String, mode: FileMode) -> Result<FileHandle, FileError> {
        let mut opts = OpenOptions::new();
        match mode {
            FileMode::Read => {
                opts.read(true);
            }
            FileMode::Write => {
                opts.write(true).create(true).truncate(true);
            }
            FileMode::Append => {
                opts.append(true).create(true);
            }
        }
        let file = opts.open(&path)?;
        Ok(Arc::new(FileInner {
            mode,
            file: Mutex::new(file),
            read_buf: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            path,
            id: next_id(),
        }))
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn mode(&self) -> FileMode {
        self.mode
    }

    fn ensure_open(&self) -> Result<(), FileError> {
        if self.closed.load(Ordering::Acquire) {
            Err(FileError::Closed)
        } else {
            Ok(())
        }
    }

    fn ensure_read(&self, label: &'static str) -> Result<(), FileError> {
        self.ensure_open()?;
        if !self.mode.is_read() {
            return Err(FileError::WrongMode(label));
        }
        Ok(())
    }

    fn ensure_write(&self, label: &'static str) -> Result<(), FileError> {
        self.ensure_open()?;
        if !self.mode.is_write() {
            return Err(FileError::WrongMode(label));
        }
        Ok(())
    }

    /// Drain up to `n` bytes from the surplus buffer; an empty vec means
    /// the buffer is empty.
    fn drain_buffered(&self, n: usize) -> Vec<u8> {
        let mut buf = self.read_buf.lock().unwrap();
        let k = n.min(buf.len());
        buf.drain(..k).collect()
    }

    /// One blocking `read` syscall; the buffer is *not* touched here.
    fn raw_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut guard = self.file.lock().unwrap();
        guard.read(buf)
    }

    /// Append one syscall's worth of bytes to `read_buf`; returns the
    /// count added (0 = EOF). The buffer lock is not held across the
    /// syscall.
    fn recv_more(&self) -> Result<usize, FileError> {
        let mut tmp = vec![0u8; CHUNK];
        let got = self.raw_read(&mut tmp)?;
        if got > 0 {
            self.read_buf.lock().unwrap().extend_from_slice(&tmp[..got]);
        }
        Ok(got)
    }

    /// Read up to `n` bytes. An empty result means EOF. The buffer is
    /// drained first; otherwise a single syscall runs (capped — a short
    /// read is valid, the caller loops).
    pub fn read_chunk(&self, n: usize) -> Result<Vec<u8>, FileError> {
        self.ensure_read("read expects a file opened for reading")?;
        if n == 0 {
            return Ok(Vec::new());
        }
        let buffered = self.drain_buffered(n);
        if !buffered.is_empty() {
            return Ok(buffered);
        }
        let mut tmp = vec![0u8; n.min(MAX_DIRECT_READ)];
        let got = self.raw_read(&mut tmp)?;
        tmp.truncate(got);
        Ok(tmp)
    }

    /// Read exactly `n` bytes. A premature EOF raises [`FileError::Eof`].
    pub fn read_exact(&self, n: usize) -> Result<Vec<u8>, FileError> {
        self.ensure_read("read_exact expects a file opened for reading")?;
        let mut out: Vec<u8> = Vec::with_capacity(n);
        // Drain the buffer first.
        let buffered = self.drain_buffered(n);
        out.extend_from_slice(&buffered);
        while out.len() < n {
            let remaining = n - out.len();
            let mut tmp = vec![0u8; remaining.min(MAX_DIRECT_READ)];
            let got = self.raw_read(&mut tmp)?;
            if got == 0 {
                return Err(FileError::Eof(format!(
                    "read_exact: wanted {n} bytes, got {} before EOF",
                    out.len()
                )));
            }
            out.extend_from_slice(&tmp[..got]);
        }
        Ok(out)
    }

    /// Read up to and including the next `delim` byte. `None` means a
    /// clean EOF with nothing buffered; trailing bytes with no delimiter
    /// at EOF are returned as a final unterminated chunk.
    pub fn read_until(&self, delim: u8) -> Result<Option<Vec<u8>>, FileError> {
        self.ensure_read("read_until expects a file opened for reading")?;
        loop {
            {
                let mut buf = self.read_buf.lock().unwrap();
                if let Some(pos) = buf.iter().position(|&b| b == delim) {
                    return Ok(Some(buf.drain(..=pos).collect()));
                }
            }
            if self.recv_more()? == 0 {
                let mut buf = self.read_buf.lock().unwrap();
                return Ok(if buf.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut *buf))
                });
            }
        }
    }

    /// Read every remaining byte. The buffer is drained first, then the
    /// underlying file is read to EOF.
    pub fn read_all(&self) -> Result<Vec<u8>, FileError> {
        self.ensure_read("read_all expects a file opened for reading")?;
        let mut out = std::mem::take(&mut *self.read_buf.lock().unwrap());
        let mut guard = self.file.lock().unwrap();
        guard.read_to_end(&mut out)?;
        Ok(out)
    }

    /// Read one `\n`-terminated line, with trailing `\r\n` / `\n`
    /// stripped, decoded as UTF-8. `None` at clean EOF; invalid UTF-8
    /// raises [`FileError::Decode`].
    pub fn read_line(&self) -> Result<Option<String>, FileError> {
        let line = self.read_until(b'\n')?;
        let Some(mut line) = line else { return Ok(None) };
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        match String::from_utf8(line) {
            Ok(s) => Ok(Some(s)),
            Err(e) => Err(FileError::Decode(format!(
                "read_line: invalid UTF-8 at byte {}",
                e.utf8_error().valid_up_to()
            ))),
        }
    }

    /// Write every byte. Returns the byte count (always `data.len()` on
    /// success; the return value is for parity with `Net.write`).
    pub fn write_all(&self, data: &[u8]) -> Result<usize, FileError> {
        self.ensure_write("write expects a file opened for writing")?;
        let mut guard = self.file.lock().unwrap();
        guard.write_all(data)?;
        guard.flush()?;
        Ok(data.len())
    }

    /// Seek to absolute `pos` from the start of the file. Drops any
    /// over-read surplus so subsequent reads observe the new position.
    pub fn seek(&self, pos: i64) -> Result<(), FileError> {
        self.ensure_open()?;
        if pos < 0 {
            return Err(FileError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("seek: negative position {pos}"),
            )));
        }
        // Drop buffered surplus *before* the seek — a read after seek
        // must see the new region, not stale bytes from the old one.
        self.read_buf.lock().unwrap().clear();
        let mut guard = self.file.lock().unwrap();
        guard.seek(SeekFrom::Start(pos as u64))?;
        Ok(())
    }

    /// Report the current logical read/write position: the file's own
    /// cursor minus any bytes pre-buffered by a previous `read_until` /
    /// `read_line` that the user has not yet consumed.
    pub fn tell(&self) -> Result<i64, FileError> {
        self.ensure_open()?;
        let buffered = self.read_buf.lock().unwrap().len() as i64;
        let mut guard = self.file.lock().unwrap();
        let pos = guard.stream_position()?;
        Ok(pos as i64 - buffered)
    }

    /// Close the handle. Idempotent. The OS file descriptor is closed
    /// when the last `Arc` clone drops (RAII), so this only flips the
    /// flag — subsequent ops then see `Closed`.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `File` may cross actor threads (a `spawn`ed worker reading a
    /// file the parent opened), so its handle must be `Send + Sync`.
    #[test]
    fn file_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FileInner>();
        assert_send_sync::<FileHandle>();
        assert_send_sync::<FileError>();
    }
}
