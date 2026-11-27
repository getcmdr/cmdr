//! Windows: nothing to raise. A Win32 handle has no per-process cap at this scale, and the CRT's
//! stdio descriptor limit (`_setmaxstdio`) covers `FILE*`s, which nothing in Cmdr opens.

use super::super::OpenFileLimit;

pub(crate) fn raise() -> OpenFileLimit {
    OpenFileLimit::NotApplicable
}
