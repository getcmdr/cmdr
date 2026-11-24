//! A shortcut (`.lnk`) is drawn from another file: its icon location, else its target's icon.
//! Drawing one whose either place is on `\\host\…` opens an SMB connection on listing (and can
//! offer the user's credentials), so the link is read WITHOUT resolving (`IShellLinkW` through
//! `IPersistFile::Load`, `GetIconLocation`, `GetPath` with `SLGP_RAWPATH`, ❌ never `Resolve`)
//! and asked for its own icon only when both places pass [`super::place::may_read`].

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ};
use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH, ShellLink};
use windows::core::{HSTRING, Interface};

/// Longest path either field holds; a longer one comes back cut, which then names no place.
const FIELD_LEN: usize = 1_024;

/// Whether native `path` names a shortcut, by its extension.
pub(super) fn is_shortcut(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
}

/// Whether the shortcut at `lnk` (itself already a local place) points only at local places:
/// its icon location and its target, each where set. An unreadable link answers no.
pub(super) fn points_only_here(lnk: &Path) -> bool {
    let Some((icon, target)) = read_link(lnk) else {
        return false;
    };
    [icon, target]
        .into_iter()
        .flatten()
        .all(|place| super::place::may_read(&PathBuf::from(expand_env_vars(&place, |name| std::env::var(name).ok()))))
}

/// The link's icon location and target path as stored, each `None` when unset (a link to a
/// Shell item like This PC has no target path).
fn read_link(lnk: &Path) -> Option<(Option<String>, Option<String>)> {
    crate::platform::apartment::ensure_on_this_thread();
    // SAFETY: creating the Shell's own in-process `ShellLink` object; no outer object.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let file: IPersistFile = link.cast().ok()?;
    // SAFETY: the path is a live NUL-terminated `HSTRING`; `Load` only reads the `.lnk` file.
    unsafe { file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ) }.ok()?;

    let mut icon = vec![0u16; FIELD_LEN];
    let mut index = 0i32;
    // SAFETY: `icon` is a live buffer the call writes at most its length into.
    unsafe { link.GetIconLocation(&mut icon, &raw mut index) }.ok()?;
    let mut target = vec![0u16; FIELD_LEN];
    // SAFETY: as above; a null find-data pointer is documented as "not wanted", and
    // `SLGP_RAWPATH` answers the stored text without resolving anything.
    unsafe { link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0.unsigned_abs()) }.ok()?;
    Some((text_of(&icon), text_of(&target)))
}

/// The NUL-terminated text in `buffer`, `None` when empty.
fn text_of(buffer: &[u16]) -> Option<String> {
    let len = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
}

/// `text` with each `%NAME%` replaced by `lookup(NAME)`, the way a link stores
/// `%SystemRoot%\system32\shell32.dll`. A name `lookup` doesn't know stays as written, which
/// then names no drive and reads as not local.
pub(super) fn expand_env_vars(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => match lookup(&after[..end]) {
                Some(value) => {
                    out.push_str(&value);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            },
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
