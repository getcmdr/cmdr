//! A typed path's Windows 8.3 short names (`LONGNA~1`) spelled long, the way every listing
//! names the same folder (#1337). A pane opened at the short spelling would disagree with
//! everything keyed on the long one: its rows against its root, the index, watchers,
//! favorites, history, the other pane. `cmdr_fs::app_path::expand_short_names` states the
//! rule: an inbound crossing that isn't a listing's own child goes through it first.

use std::io;
use std::path::{Path, PathBuf};

/// `native` with the short names in its longest existing prefix spelled long by `expand`
/// (`GetLongPathNameW` on Windows, which refuses a path that isn't there), and the missing
/// tail kept as typed. `None` when it holds no `~` (no short name can be there), when no
/// prefix expands, or when the long spelling is the same text.
pub(super) fn long_spelling(native: &Path, expand: impl Fn(&Path) -> io::Result<PathBuf>) -> Option<PathBuf> {
    if !native.as_os_str().as_encoded_bytes().contains(&b'~') {
        return None;
    }
    let prefix = native
        .ancestors()
        .take_while(|prefix| !prefix.as_os_str().is_empty())
        .find_map(|prefix| expand(prefix).ok().map(|long| (prefix, long)));
    let (prefix, long) = prefix?;
    let tail = native.strip_prefix(prefix).ok()?;
    let spelled = if tail.as_os_str().is_empty() { long } else { long.join(tail) };
    (spelled != native).then_some(spelled)
}

#[cfg(test)]
#[path = "long_names_test.rs"]
mod tests;
