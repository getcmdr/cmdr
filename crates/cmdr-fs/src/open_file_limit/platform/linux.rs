//! Linux: the shared POSIX raise (`unix.rs`), held only to the hard limit (which `fs.nr_open`
//! caps in turn), so no per-process ceiling of its own.

use super::super::OpenFileLimit;

pub(crate) fn raise() -> OpenFileLimit {
    super::unix::raise_within(None)
}
