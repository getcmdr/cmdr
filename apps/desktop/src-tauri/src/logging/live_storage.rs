//! `LiveStorage`: the file chain's sink, following the log-storage cap while the app runs.
//!
//! The fern tree is built once, at startup, and always carries the file chain. This sink sits
//! under `CoalescingWriter` and reads the live keep-count (`advanced.maxLogStorageMb`, as
//! `ceil(cap_mb / 50)`) on every write:
//!
//! - **0**: storage is off. The rotator is dropped, closing `cmdr.log`, and lines go nowhere.
//! - **Off → on**: the next line opens `cmdr.log` again (appending, so it picks up where the file
//!   left off).
//! - **Any other change**: the rotator keeps that many rotated files from its next rotation on.
//!   Lowering the cap deletes the excess right away through `eager_prune`; this sink only stops
//!   new rotations from bringing them back.
//!
//! One atomic load per write is the whole cost, so a cap change needs no dispatch rebuild (which
//! would drop records mid-swap) and no restart.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::rotating_file::RotatingFile;

pub(crate) struct LiveStorage {
    path: PathBuf,
    max_bytes: u64,
    /// The live keep-count: total files kept, the live one included. `0` = storage off.
    keep_count: &'static AtomicUsize,
    /// `None` while storage is off, and until the first write after it's turned on.
    file: Option<RotatingFile>,
    /// The keep-count `file` was last set up for.
    applied: usize,
}

impl LiveStorage {
    pub(crate) fn new(path: PathBuf, max_bytes: u64, keep_count: &'static AtomicUsize) -> Self {
        Self {
            path,
            max_bytes,
            keep_count,
            file: None,
            applied: 0,
        }
    }

    /// The rotator for the current cap, or `None` when storage is off.
    fn current(&mut self) -> Option<&mut RotatingFile> {
        let keep = self.keep_count.load(Ordering::Relaxed);
        if keep == 0 {
            if let Some(mut file) = self.file.take() {
                // Storage is off: nothing is left to report a failed last flush to.
                let _ = file.flush();
            }
            self.applied = 0;
            return None;
        }
        let rotated = rotated_keep(keep);
        match self.file.as_mut() {
            Some(file) if keep != self.applied => file.set_rotated_keep(rotated),
            Some(_) => {}
            None => {
                if let Some(dir) = self.path.parent() {
                    // A failure surfaces as the open's own error on the write below.
                    let _ = fs::create_dir_all(dir);
                }
                self.file = Some(RotatingFile::new(self.path.clone(), rotated, self.max_bytes));
            }
        }
        self.applied = keep;
        self.file.as_mut()
    }
}

/// Rotated files kept beside the live one for a total keep-count: the live file plus N-1 copies,
/// at least one, so a rotation never just deletes the only file.
pub(crate) fn rotated_keep(keep_count: usize) -> usize {
    keep_count.saturating_sub(1).max(1)
}

impl Write for LiveStorage {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.current() {
            Some(file) => file.write(buf),
            // Storage is off: the line is accepted and dropped, as with no file chain at all.
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.current() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "live_storage_tests.rs"]
mod tests;
