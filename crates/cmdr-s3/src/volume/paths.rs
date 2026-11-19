//! The paths the app addresses a place with, down to the bucket and key a
//! request names.
//!
//! The translation is `cmdr_fs::volume::remote_paths`, shared with SFTP and
//! WebDAV: the app spelling carries the account's `s3://<key id>@<host>:<port>`
//! prefix, the server-side tree under it is `/<bucket>/<key…>`, and a place is
//! rooted at `/` (the account, listing buckets) or `/<bucket>`. A bare
//! server-absolute path is ❌ REFUSED rather than anchored; that module's header
//! has why.

use std::collections::HashMap;

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::VolumeError;
use cmdr_fs::volume::remote_paths::{RemoteAppRefusal, RemoteRoot};

use super::S3Volume;
use super::listing::{Child, FILE_SUFFIX, Settled};
use crate::params::S3ConnectionParams;
use crate::refusal::S3ConnectError;

/// A place's root in the spellings a volume needs: the translation, the
/// account's prefix, and the typed root `Volume::root` answers.
pub(super) struct Place {
    pub root: RemoteRoot,
    /// `s3://<key id>@<host>:<port>`, what every app path on the account starts with.
    pub prefix: String,
    /// `<prefix>/` or `<prefix>/<bucket>`, the same text the root always had.
    pub app: AppPathBuf,
}

impl Place {
    /// The place `params` names.
    ///
    /// # Errors
    ///
    /// A bucket (or a key id) holding text no app path holds yet (a NUL, an
    /// escape-range code point), which no S3 provider allows in a bucket name or
    /// issues as a key id: `NoSuchBucket` for the bucket, `KeysRejected` for the
    /// key id.
    pub(super) fn of(params: &S3ConnectionParams) -> Result<Self, S3ConnectError> {
        let prefix = cmdr_fs::volume::s3_app_root(params.host(), params.port(), params.access_key_id());
        let root = RemoteRoot::new(prefix.clone(), std::path::Path::new(&params.remote_root()));
        let app = match root.app() {
            Ok(app) => app.to_owned(),
            Err(_) if RemoteRoot::new(prefix.clone(), std::path::Path::new("/")).app().is_ok() => {
                return Err(S3ConnectError::NoSuchBucket);
            }
            Err(_) => return Err(S3ConnectError::KeysRejected),
        };
        Ok(Self { root, prefix, app })
    }
}

/// What a server-side path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target<'a> {
    /// `/`: the account, whose children are its buckets.
    Account,
    /// `/<bucket>`: a bucket's top.
    Bucket(&'a str),
    /// `/<bucket>/<key>`: an object, or a folder (a prefix) under the bucket.
    Key { bucket: &'a str, key: &'a str },
}

/// Splits a normalized server-side path (`RemoteRoot::to_remote_path`'s
/// answer: absolute, no trailing slash, no `.` or `..`).
pub(super) fn target_of(remote: &str) -> Target<'_> {
    let rest = remote.trim_start_matches('/');
    match rest.split_once('/') {
        _ if rest.is_empty() => Target::Account,
        Some((bucket, key)) if !key.is_empty() => Target::Key { bucket, key },
        Some((bucket, _)) => Target::Bucket(bucket),
        None => Target::Bucket(rest),
    }
}

/// Which of a name's two holders a path means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Holder {
    /// Whatever holds it, the folder first (the usual case: no file was
    /// listed beside a folder of its name).
    Either,
    /// The file shown as `<name> (file)`.
    File,
    /// The folder a file of its name was listed beside: ❗ never that file.
    Folder,
}

/// A path as a request names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Resolved {
    /// The server-side path the request names: for a `<name> (file)` row, the
    /// file's real key.
    pub remote: String,
    pub holder: Holder,
}

// ── A file beside a folder of its name ────────────────────────────────
//
// S3 lets the key `photos` sit beside keys under `photos/`. A pane row is one
// path, so the listing shows the file as `photos (file)` (`listing::settle`)
// and remembers its real path in `beside_folders`. Every path then goes
// through `resolve`: `…/photos (file)` names the key `photos`, and `…/photos`
// names ONLY the folder, so no operation on the folder row (delete, rename,
// stat) can reach the file. A file a real `photos (file)` key hides is
// remembered too, as `Hidden`: its folder's row still means only the folder,
// while `…/photos (file)` keeps naming that real key. Each listing of the
// folder refreshes what it holds, and deleting or renaming the file forgets
// it. ❗ What a folder-only caller relies on is `delete_directory`, not this
// map: a volume rebuilt under a live pane starts it empty.

/// How the last listing of its folder showed a file beside a folder of its
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shown {
    /// As `<name> (file)`, a row that names this file.
    AsFileRow,
    /// Not at all: a real `<name> (file)` key holds that row.
    Hidden,
}

impl S3Volume {
    /// The server-side path for `path`, or `NotFound` when `path` isn't on this
    /// place. A `<name> (file)` row's path gives its real key.
    pub(super) fn to_remote_path(&self, path: &AppPath) -> Result<String, VolumeError> {
        Ok(self.resolve(path)?.remote)
    }

    /// The server-side path for `path` and which holder of its name it means.
    pub(super) fn resolve(&self, path: &AppPath) -> Result<Resolved, VolumeError> {
        self.resolve_shown(self.root.to_remote(path), || path.as_str().to_owned())
    }

    /// [`Self::resolve`] from the server-side path a row shows (`None` off
    /// this place, `NotFound` naming `spelled()`).
    pub(super) fn resolve_shown(
        &self,
        shown: Option<String>,
        spelled: impl FnOnce() -> String,
    ) -> Result<Resolved, VolumeError> {
        let shown = shown.ok_or_else(|| VolumeError::NotFound(spelled()))?;
        let beside = self.beside_folders();
        if let Some(real) = shown.strip_suffix(FILE_SUFFIX)
            && beside.get(real) == Some(&Shown::AsFileRow)
        {
            return Ok(Resolved {
                remote: real.to_string(),
                holder: Holder::File,
            });
        }
        let holder = if beside.contains_key(&shown) {
            Holder::Folder
        } else {
            Holder::Either
        };
        Ok(Resolved { remote: shown, holder })
    }

    /// The app path a row at the server-side `remote` carries, as text: what
    /// `FileEntry.path` holds. `None` when a misbehaving server named something
    /// off this place's root.
    ///
    /// ❗ A server name the typed path can't hold yet (an escape-range code
    /// point, a NUL) keeps today's spelling, `<prefix><remote>`, ❌ never a
    /// dropped row: escapes aren't live (`RemoteAppRefusal::NotAppText`).
    pub(super) fn row_path(&self, remote: &str) -> Option<String> {
        match self.root.to_app(remote) {
            Ok(path) => Some(path.into_string()),
            Err(RemoteAppRefusal::OffRoot) => None,
            Err(RemoteAppRefusal::NotAppText(_)) => Some(format!("{}{remote}", self.prefix)),
        }
    }

    /// [`Self::row_path`] as a typed app path, for the listing patcher; `None`
    /// also for a server name the typed path can't hold yet.
    pub(super) fn app_path_of(&self, remote: &str) -> Option<AppPathBuf> {
        self.root.to_app(remote).ok()
    }

    /// The server-side path a pane shows the object at `remote` under:
    /// `<remote> (file)` when it was listed beside a folder of its name.
    pub(super) fn shown_remote(&self, remote: &str) -> String {
        if self.beside_folders().get(remote) == Some(&Shown::AsFileRow) {
            format!("{remote}{FILE_SUFFIX}")
        } else {
            remote.to_string()
        }
    }

    /// Records which files a listing of `parent` found beside a folder of
    /// their name, shown or hidden, replacing what the folder's last listing
    /// said.
    pub(super) fn note_beside_folders(&self, parent: &str, settled: &Settled) {
        let mut beside = self.beside_folders();
        beside.retain(|remote, _| parent_of(remote) != parent);
        for child in &settled.children {
            if let Child::Object {
                name,
                beside_folder: true,
                ..
            } = child
                && let Some(real) = name.strip_suffix(FILE_SUFFIX)
            {
                beside.insert(child_of(parent, real), Shown::AsFileRow);
            }
        }
        for name in &settled.hidden {
            beside.insert(child_of(parent, name), Shown::Hidden);
        }
    }

    /// The file at `remote` is gone (deleted, renamed away): its folder's path
    /// means whatever holds it again.
    pub(super) fn forget_beside_folder(&self, remote: &str) {
        self.beside_folders().remove(remote);
    }

    /// The map, whatever a panicking holder left: each key is a whole path.
    fn beside_folders(&self) -> std::sync::MutexGuard<'_, HashMap<String, Shown>> {
        self.inner
            .beside_folders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The server-side path of `remote`'s parent folder.
fn parent_of(remote: &str) -> &str {
    match remote.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((parent, _)) => parent,
        None => "",
    }
}

/// `parent/name` in server-side spelling.
pub(super) fn child_of(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
#[path = "paths_test.rs"]
mod paths_test;
