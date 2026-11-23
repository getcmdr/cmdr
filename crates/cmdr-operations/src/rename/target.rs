//! Where a rename lands, built only through the door that refuses `.` and `..` as the new
//! name (#1229).
//!
//! ❗ The refusal has to read the TEXT a sender spelled: a typed app path drops a `.` name
//! (`/docs/.` reads as `/docs`), so a check on the typed target never fires, and a forced
//! rename of `/docs/a.txt` "to" `/docs/.` became a rename onto its own parent. There's no
//! other way to build a [`RenameTarget`], so every caller (the desktop's command, the browser
//! demo, a test) meets the refusal.

use cmdr_fs::app_path::{AppPath, AppPathBuf};

use super::super::mutation_error::MutationError;

/// A rename's destination: an app path whose last name, as its sender spelled it, is a real
/// name. Built only by [`RenameTarget::read`].
#[derive(Debug, Clone)]
pub struct RenameTarget(AppPathBuf);

impl RenameTarget {
    /// `text` (a pane's `parent/new name`) as a rename's destination, read through `read`, the
    /// host's own reading of path text (the desktop's `wire_path::start_path`, the demo's
    /// `demo_path`). A last name of `.` or `..` (a trailing `/` ignored, as the OS reads it)
    /// is refused as `NameNotAllowed { DotName }`, the rule's own words, BEFORE `read`
    /// normalizes it away.
    ///
    /// # Errors
    ///
    /// [`MutationError::NameNotAllowed`] for a dot name; whatever `read` refuses.
    pub fn read(
        text: &str,
        read: impl FnOnce(&str) -> Result<AppPathBuf, MutationError>,
    ) -> Result<Self, MutationError> {
        if ends_in_dot_name(text) {
            return Err(MutationError::NameNotAllowed {
                reason: cmdr_fs::validation::ValidationError::DotName,
            });
        }
        read(text).map(Self)
    }

    /// The destination's app path.
    pub fn as_app_path(&self) -> &AppPath {
        &self.0
    }

    /// The destination's app path, owned.
    pub fn into_app_path(self) -> AppPathBuf {
        self.0
    }
}

/// Whether `text`'s last name is `.` or `..`, a trailing `/` ignored.
fn ends_in_dot_name(text: &str) -> bool {
    matches!(text.trim_end_matches('/').rsplit('/').next(), Some("." | ".."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_wire(text: &str) -> Result<AppPathBuf, MutationError> {
        AppPathBuf::parse(text, cmdr_fs::app_path::PathFlavor::Posix).map_err(|_| MutationError::NameEmpty)
    }

    /// The text is judged before the reading drops its `.`: `/docs/.` would read as `/docs`.
    #[test]
    fn a_dot_or_dot_dot_new_name_is_refused_before_the_text_is_read() {
        for text in ["/docs/.", "/docs/..", "/docs/./", "/docs/..//", ".", ".."] {
            let refused = RenameTarget::read(text, |_| panic!("{text}: the reading never runs"));
            assert!(
                matches!(
                    refused,
                    Err(MutationError::NameNotAllowed {
                        reason: cmdr_fs::validation::ValidationError::DotName
                    })
                ),
                "{text}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_real_new_name_reads_through_the_hosts_door() {
        for (text, read) in [
            ("/docs/b.txt", "/docs/b.txt"),
            ("/docs/.hidden", "/docs/.hidden"),
            ("/docs/a..", "/docs/a.."),
        ] {
            let target = RenameTarget::read(text, as_wire).expect(text);
            assert_eq!(target.as_app_path().as_str(), read);
        }
    }
}
