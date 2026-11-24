//! Where each OS gets its icons from, behind one fn set. A platform seam
//! (`docs/guides/platform-seams.md`); the caches, the id scheme, and the sample paths above it
//! are portable.

use std::path::Path;

use image::DynamicImage;

#[cfg_attr(target_os = "macos", path = "macos/mod.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows/mod.rs")]
mod imp;

/// Extended-attribute reads, shared by the macOS and Linux arms.
#[cfg(unix)]
mod unix;

/// The 8 MB-stack threads real-place lookups run on, shared by the macOS and Windows arms.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod icon_threads;

/// Each path's icon as a data URL (`None` when the OS has none), keyed `path:{path}`.
pub(super) type PathIcons = Vec<(String, Option<String>)>;

const _: fn(&Path) -> Option<String> = fetch_icon_for_path;
const _: fn(Vec<String>) -> PathIcons = fetch_path_icons;
const _: fn(&str, bool) -> Option<String> = icon_for_id;
const _: fn(&str, bool) -> Option<DynamicImage> = fresh_extension_icon;
const _: fn(fn() -> bool) -> bool = with_object_pool;
const _: fn() -> bool = tag_tinting_disabled;
const _: fn(&Path, &str) -> Option<Vec<u8>> = extended_attribute;
const _: fn(&str) -> bool = may_read_own_icon;

/// Whether the place at app path `path` may be asked for its OWN icon: the ask stays on this
/// machine and leaves the file as it is (no network round trip, no cloud download, no place
/// that only exists inside an archive). When not, `get_icons` shows its type's icon
/// (`own_icons::stand_in_icon_id`) and skips the disk cache, whose mtime read would reach
/// the place too.
pub(super) use imp::may_read_own_icon;

/// The OS's icon for the file or folder at `path`, as a data URL, or `None` when the OS has
/// none for it.
pub(super) use imp::fetch_icon_for_path;

/// [`fetch_icon_for_path`] for each of `paths`, keyed `path:{path}`, in input order. Each arm
/// picks the threads: a real folder's lookup can descend into a cloud provider.
pub(super) use imp::fetch_path_icons;

/// An icon this OS can answer for an icon id directly, before the sample-path fallback, or
/// `None` to go to the fallback.
pub(super) use imp::icon_for_id;

/// The icon `ext`'s default app declares for its documents, read past the OS's icon cache, or
/// `None` where the OS has no such read (the caller then asks a sample path).
pub(super) use imp::fresh_extension_icon;

/// Runs `f` inside whatever per-call object pool this OS's icon APIs need drained (macOS
/// autoreleases), or just runs it.
pub(super) use imp::with_object_pool;

/// Whether the user turned off tag-based folder tinting. False where the OS has no tags.
pub(super) use imp::tag_tinting_disabled;

/// The extended attribute `name` on `path`, or `None` when it's absent, unreadable, or the OS
/// keeps no such attributes.
pub(super) use imp::extended_attribute;
