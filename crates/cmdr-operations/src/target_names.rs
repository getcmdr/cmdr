//! The names top-level sources take at a transfer's destination when they
//! aren't their own: a rename that runs as a move.
//!
//! A rename is one cheap call on most volumes, but not on an object store,
//! where it copies every byte and deletes the source (`Volume::rename_work`).
//! Such a rename goes through the move engine, whose destination is a FOLDER;
//! this map is what makes "move `/a/foo` into `/a` as `bar`" expressible. It's
//! carried on the operation's state (`WriteOperationState::target_names`) and
//! read in exactly one place, the async transfer driver, where every volume
//! engine builds its top-level destination paths.
//!
//! ❗ A name, never a path: [`TargetNames::new`] refuses a name that isn't one
//! plain name by the app path's rules (`AppName::from_listed_text`), so a map
//! can't send a source anywhere but into the destination folder it was given.
//! A POSIX server's `a\b` is one name on every host (on Windows `std::path`'s
//! components split it and refused it).

use std::collections::HashMap;

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf};

/// The NAME each renamed top-level source takes at the destination, by source
/// path, as the app text it was sent.
// DEFAULT-OK: an empty map is the ordinary transfer, every source keeping its
// own name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TargetNames(HashMap<AppPathBuf, String>);

/// A target name that isn't one plain name (empty, `.`, `..`, holding a `/`
/// or a NUL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotAName(pub(crate) String);

impl TargetNames {
    /// The map for `renames`, refusing any name that isn't one plain name.
    pub(crate) fn new(renames: impl IntoIterator<Item = (AppPathBuf, String)>) -> Result<Self, NotAName> {
        let mut names = HashMap::new();
        for (source, name) in renames {
            if AppName::from_listed_text(&name).is_err() {
                return Err(NotAName(name));
            }
            names.insert(source, name);
        }
        Ok(Self(names))
    }

    /// The target name `source` was given, `None` when it keeps its own.
    pub(crate) fn chosen_for(&self, source: &AppPath) -> Option<&str> {
        self.0.get(source).map(String::as_str)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmdr_fs::app_path;

    fn renamed(name: &str) -> Result<TargetNames, NotAName> {
        TargetNames::new([(app_path!("/a/foo").to_owned(), name.to_string())])
    }

    #[test]
    fn a_renamed_source_takes_its_target_name_and_the_rest_keep_their_own() {
        let names = renamed("bar").expect("one name");
        assert_eq!(names.chosen_for(app_path!("/a/foo")), Some("bar"));
        assert_eq!(names.chosen_for(app_path!("/a/other")), None);
        assert!(TargetNames::default().is_empty());
    }

    /// ❗ A name can't smuggle in a path: the source would land outside the
    /// folder the move was given.
    #[test]
    fn a_target_name_that_isnt_one_plain_name_is_refused() {
        for bad in ["", ".", "..", "x/y", "/abs", "../up", "/etc/x", "a\0b"] {
            assert_eq!(renamed(bad), Err(NotAName(bad.to_string())), "{bad:?}");
        }
    }

    /// A name holding `\`, `:`, or a trailing dot or space is ONE name on every host:
    /// a POSIX server holds it whole (Windows' `std::path` split `a\b` and refused it).
    #[test]
    fn a_hostile_looking_name_is_one_name_on_every_host() {
        for one in ["a\\b", "foo\\..", "..\\bar", "a:b", "x.", "x ", "C:\\x"] {
            let names = renamed(one).expect("one name");
            assert_eq!(names.chosen_for(app_path!("/a/foo")), Some(one), "{one:?}");
        }
    }
}
