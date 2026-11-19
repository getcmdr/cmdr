//! The pure half of "Go to path" (⌘G): what a typed input names, classified over a
//! host's answer for what's at a path.
//!
//! Two hosts call [`resolve_with`]: the desktop's `go_to_path::resolve` over the local
//! disk (`apps/desktop/src-tauri/src/go_to_path/`, where the decisions live: lexical
//! `.`/`..`, no `canonicalize`, nearest ancestor) and the browser demo's engine over its
//! in-memory volume (`crates/cmdr-listing-wasm/src/go_to.rs`). The steps, so both answer
//! alike: trim (empty is [`GoToPathResolution::Invalid`]), expand `~`, join onto the
//! pane's folder, normalize lexically, then a folder, a file (its parent and name), or
//! the nearest ancestor that exists.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The outcome of resolving a typed "go to path" input. The variant is the contract the
/// frontend branches on; ❌ never classify by string-matching `reason`.
///
/// `rename_all_fields = "camelCase"` is REQUIRED: without it, struct-variant fields ship
/// snake_case through tauri-specta and read `undefined` on the TS side (`ipc-enum-camelcase`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum GoToPathResolution {
    /// The resolved path is an existing directory. Navigate into it.
    Directory {
        /// The folder, as an app path.
        path: String,
    },
    /// The resolved path is an existing file. `path` is the canonical normalized
    /// full path (recorded into recents); `parent_dir` / `file_name` drive the
    /// navigate-to-parent + select.
    File {
        /// The file, as an app path.
        path: String,
        /// Its folder, as an app path.
        parent_dir: String,
        /// Its name.
        file_name: String,
    },
    /// The resolved path doesn't exist. `ancestor_dir` is the nearest existing
    /// ancestor (worst case `/`). Navigate there and fire an INFO toast.
    NearestAncestor {
        /// What the input named, as an app path.
        requested: String,
        /// The closest folder above it that exists.
        ancestor_dir: String,
    },
    /// Defensive: the input was empty or couldn't be turned into a path.
    Invalid {
        /// Why, for the log only.
        reason: String,
    },
}

/// What's at a path, as far as a go-to cares: a folder, or anything else that exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundKind {
    /// A folder (a link to one counts as one).
    Directory,
    /// A file, or any other non-folder.
    File,
}

/// What a host answers for [`resolve_with`].
pub trait GoToHost {
    /// `input` with a leading `~` expanded to the home folder; any other input unchanged.
    fn expand_tilde(&self, input: &str) -> String;
    /// What's at `path`, following links, or `None` when nothing is (or it can't be read).
    fn found(&self, path: &Path) -> Option<FoundKind>;
    /// The app-path spelling of `path`, the one form a pane holds.
    fn app_spelling(&self, path: &Path) -> String;
    /// Whether `path` crosses into an archive the host browses as a folder. None by default.
    fn enters_archive(&self, _path: &Path) -> bool {
        false
    }
}

/// Lexically normalizes `.` and `..` components without touching the disk.
///
/// `.` is dropped; `..` pops the previous normal component, or is kept verbatim when
/// there's nothing to pop (a leading `..` on a relative path, or a `..` right after the
/// root). The root prefix (`/`) is preserved.
///
/// ❌ Not a general-purpose normalizer, so it stays in the crate: across a symlink on POSIX,
/// `/a/link/..` means the link TARGET's parent, which text alone can't know. The typed-path plan
/// (`docs/specs/app-path-plan.md` § "Typed and pasted `..`", #1225) replaces this step for every
/// `..` but a relative input's leading ones with a volume-aware one, asked through [`GoToHost`]
/// (a `parent_for_dot_dot`-style method whose default is today's lexical parent), so both hosts
/// keep answering alike.
pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let popped = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if popped {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves a typed `input` against `base_dir` (the focused pane's folder, as the host's
/// own path) over `host`.
pub fn resolve_with(input: &str, base_dir: &Path, host: &impl GoToHost) -> GoToPathResolution {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return GoToPathResolution::Invalid {
            reason: "Type a path to go to.".to_string(),
        };
    }

    // `join` keeps an absolute input whole and puts a relative one under the pane's folder.
    // Not `is_absolute` first: it's always `false` in the browser build, where `join` still
    // replaces the base with a rooted input.
    let expanded = host.expand_tilde(trimmed);
    let normalized = lexical_normalize(&base_dir.join(&expanded));
    let normalized_str = host.app_spelling(&normalized);

    if host.enters_archive(&normalized) {
        return GoToPathResolution::Directory { path: normalized_str };
    }

    match host.found(&normalized) {
        Some(FoundKind::Directory) => GoToPathResolution::Directory { path: normalized_str },
        Some(FoundKind::File) => {
            // A path that exists has a parent except for the root, which is a folder.
            let parent_dir = normalized
                .parent()
                .map_or_else(|| "/".to_string(), |parent| host.app_spelling(parent));
            let file_name = normalized
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| normalized_str.clone());
            GoToPathResolution::File {
                path: normalized_str,
                parent_dir,
                file_name,
            }
        }
        None => GoToPathResolution::NearestAncestor {
            requested: normalized_str,
            ancestor_dir: host.app_spelling(&nearest_existing_ancestor(&normalized, host)),
        },
    }
}

/// Walks up from `path` to the closest ancestor that exists; the topmost one when none does.
fn nearest_existing_ancestor(path: &Path, host: &impl GoToHost) -> PathBuf {
    let mut current = path;
    loop {
        if host.found(current).is_some() {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return current.to_path_buf(),
        }
    }
}

#[cfg(test)]
#[path = "go_to_path_test.rs"]
mod tests;
