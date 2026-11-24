//! Which text editors this OS lists for F4, and how it hands a file to one. A platform seam
//! (`docs/guides/platform-seams.md`).

use super::TextEditorList;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// Windows' editor rules, pure so they're tested on every OS (`windows.rs` asks the Shell).
#[cfg(any(target_os = "windows", test))]
mod windows_rules;

const _: fn(&str) -> TextEditorList = list_text_editors;

/// The text editors this OS lists, the system default, and which one the stored setting names
/// (`chosen_id`, canonical). Blocks; the command bounds it.
pub use imp::list_text_editors;

/// macOS: F4's launch for a native file, through `open` (`-t`, `-b`, or `-a`).
#[cfg(target_os = "macos")]
pub use imp::open_in_editor;

/// Windows: F4's launch for the wire text `path`, on a Shell thread under its own deadline.
#[cfg(target_os = "windows")]
pub use imp::open_in_editor;
