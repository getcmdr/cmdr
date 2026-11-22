//! Mount points the OS hands over as TEXT, crossed to the native path they are in one place.
//!
//! The mount table (`getmntinfo`'s `f_mntonname`, `/proc/self/mountinfo`), NSWorkspace's mount
//! notifications, GVFS's folder names, and the `LocationInfo` rows built from them all carry a
//! mount point as a `String`, and the per-OS lookups over that table (`get_mount_point`, the
//! cloud-drive match, the disk-image check) take the path they place as the same text. That text is the OS's own spelling, so it names the native path
//! as is: ❌ it's never app text, which crosses at `commands::wire_path::native` (or
//! `AppPath::to_native`). One named crossing keeps the native reading greppable until the
//! discovery code carries `PathBuf`s from where it reads them (milestone 10f's text exits).

use std::path::Path;

/// `text`, a mount point (or a path under one) the OS reported as text, as the native path it
/// names.
pub(crate) fn os_mount_path(text: &str) -> &Path {
    Path::new(text)
}
