//! Where a caller's path lands under a Windows local volume's root, by the app path's own
//! arithmetic. Pure, so every OS table-tests it (`anchor_test.rs`); only `LocalWindowsVolume`
//! calls it.

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, NotAnAppPath, PathArithmetic, PathKind};

/// The absolute app path `path` names under the volume rooted at `root`:
///
/// - A drive or UNC path names its own place (plan D1), as it is. A bridge value spelled with
///   `\` (`C:\x`, a native path a caller handed through the bridge) is the same place.
/// - A scheme path is kept, so the crossing refuses it (`NotLocal`), ❌ never anchored.
/// - A POSIX path (`/a/b`), a relative one (`a/b`, or `a\b` on Windows), and the root's aliases
///   (`""`, `.`, `/`) hang their names under `root`, as `root_anchored` hangs them.
///
/// # Errors
///
/// A `..` name ([`NotAnAppPath::DotName`]: by text it can name a different file), what the
/// type refuses of a bridge value (`C:x`, a device path), and a name a drive or UNC root can't
/// hold.
#[cfg_attr(unix, allow(dead_code, reason = "the Windows volume's; table-tested on every OS"))]
pub(crate) fn anchor_under(root: &AppPath, path: &AppPath) -> Result<AppPathBuf, NotAnAppPath> {
    match path.to_canonical() {
        Ok(canonical) => match canonical.kind() {
            PathKind::Drive | PathKind::Unc | PathKind::Scheme => Ok(canonical.into_owned()),
            PathKind::Posix => hang(root, canonical.names()),
        },
        Err(NotAnAppPath::Relative | NotAnAppPath::Empty) => hang(root, path.names()),
        Err(why) => Err(why),
    }
}

/// `root` plus `names`, one plain name at a time; `.` names stay where they are.
fn hang<'a>(root: &AppPath, names: impl Iterator<Item = &'a str>) -> Result<AppPathBuf, NotAnAppPath> {
    let mut at = root.to_canonical()?.into_owned();
    for name in names {
        match name {
            "" | "." => continue,
            ".." => return Err(NotAnAppPath::DotName),
            _ => {}
        }
        // A name a path's own reading yields is one plain name already, so only a NUL (or an
        // escape that isn't canonical, which a bridge value never spells) is left to refuse.
        let name = AppName::from_app_spelling(name).map_err(|_| NotAnAppPath::Nul)?;
        at = at.join(name)?;
    }
    Ok(at)
}

#[cfg(test)]
#[path = "anchor_test.rs"]
mod tests;
