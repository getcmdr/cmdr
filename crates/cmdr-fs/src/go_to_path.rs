//! The pure half of "Go to path" (⌘G): what a typed input names, classified over a
//! host's answer for what's at a path.
//!
//! Two hosts call [`resolve`]: the desktop's `go_to_path::resolve` over the local disk
//! and the volume registry (`apps/desktop/src-tauri/src/go_to_path/`, where the decisions
//! live) and the browser demo's engine over its in-memory volume
//! (`crates/cmdr-listing-wasm/src/go_to.rs`). The steps, so both answer alike: trim
//! (empty is [`GoToPathResolution::Invalid`]), resolve the text as a person typed it
//! ([`crate::typed_path`]: `~`, the pane's folder, and each `..` as the volume answers
//! it), then a folder, a file (its parent and name), or the nearest ancestor that exists.

use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::app_path::{AppPath, PathArithmetic};
use crate::typed_path::{TypedBy, TypedPath, TypedPathHost, TypedPathRefusal, resolve_typed_with};

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
        /// Whether a `..` in the input crossed a symlink, so `path` isn't what the text
        /// reads as by itself: the preview line shows `path` then.
        crossed_link: bool,
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
        /// As `Directory`'s.
        crossed_link: bool,
    },
    /// The resolved path doesn't exist. `ancestor_dir` is the nearest existing
    /// ancestor (worst case the root). Navigate there and fire an INFO toast.
    NearestAncestor {
        /// What the input named, as an app path.
        requested: String,
        /// The closest folder above it that exists.
        ancestor_dir: String,
    },
    /// The input was empty or leads nowhere (no app path, or a volume that couldn't say
    /// what a `..` means there in time).
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

/// What a host answers for [`resolve`]: the typed-path host (`~`, the flavor, and each
/// `..`), plus what's at a path.
pub trait GoToHost: TypedPathHost {
    /// What's at `path`, following links, or `None` when nothing is (or it can't be read).
    fn found(&self, path: &AppPath) -> impl Future<Output = Option<FoundKind>>;

    /// Whether `path` crosses into an archive the host browses as a folder. None by default.
    fn enters_archive(&self, path: &AppPath) -> impl Future<Output = bool> {
        let _ = path;
        async { false }
    }
}

/// Resolves a typed `input` against `base_dir` (the focused pane's folder, `None` when the
/// pane's path is no app path) over `host`.
pub async fn resolve(input: &str, base_dir: Option<&AppPath>, host: &impl GoToHost) -> GoToPathResolution {
    let typed = resolve_typed_with(input.trim(), base_dir, TypedBy::Person, host).await;
    answer(typed, host).await
}

/// What the dialog hears for a typed input the host already resolved (as a person typed
/// it, trimmed): a refusal is [`GoToPathResolution::Invalid`] (blank input among them),
/// anything else is classified over `host`.
pub async fn answer(typed: Result<TypedPath, TypedPathRefusal>, host: &impl GoToHost) -> GoToPathResolution {
    match typed {
        Ok(typed) => classify(typed, host).await,
        Err(refusal) => GoToPathResolution::Invalid {
            reason: refusal.to_string(),
        },
    }
}

/// A folder, a file, or the nearest existing ancestor.
async fn classify(typed: TypedPath, host: &impl GoToHost) -> GoToPathResolution {
    let TypedPath { path, crossed_link } = typed;
    if host.enters_archive(&path).await {
        return GoToPathResolution::Directory {
            path: path.into_string(),
            crossed_link,
        };
    }
    match host.found(&path).await {
        Some(FoundKind::Directory) => GoToPathResolution::Directory {
            path: path.into_string(),
            crossed_link,
        },
        Some(FoundKind::File) => {
            // A path that exists has a parent except for a root, which is a folder.
            let parent_dir = path.parent().unwrap_or(&path).as_str().to_string();
            let file_name = path.file_name().unwrap_or(path.as_str()).to_string();
            GoToPathResolution::File {
                path: path.into_string(),
                parent_dir,
                file_name,
                crossed_link,
            }
        }
        None => {
            let ancestor_dir = nearest_existing_ancestor(&path, host).await.as_str().to_string();
            GoToPathResolution::NearestAncestor {
                requested: path.into_string(),
                ancestor_dir,
            }
        }
    }
}

/// Walks up from `path` to the closest ancestor that exists; the root when none does.
async fn nearest_existing_ancestor<'a>(path: &'a AppPath, host: &impl GoToHost) -> &'a AppPath {
    let mut current = path;
    loop {
        if host.found(current).await.is_some() {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return current,
        }
    }
}

#[cfg(test)]
#[path = "go_to_path_test.rs"]
mod tests;
