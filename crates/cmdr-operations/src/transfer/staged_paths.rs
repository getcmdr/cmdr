//! The two paths a staged write addresses on its volume, each spelled both
//! ways once: [`Temp`] (the `.cmdr-tmp-*` it writes) and [`Spelled`] (the final
//! name). A leaf: `staged_write` and `recovered_name` both name them, and
//! `recovered_name` naming `staged_write` would weld it to `volume::*` in a
//! module cycle (`scripts/check/checks/DETAILS.md` § "Rust module cycles").
//!
//! The app path is the value: every volume call addresses it, and a temp is
//! minted beside it by the app path's own split and join ([`Temp::mint`]), so a
//! POSIX server's `a\b` stays ONE name on Windows too. The `std::path` spelling
//! is derived once for the ledgers, the logs, and the errors, which only read it
//! as text, ❌ never split it.

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic, is_escape};
use cmdr_fs::staging::{StagingOwner, StagingTemp, aside_sibling_name_with_uuid, temp_sibling_name_with_uuid};
use cmdr_fs::volume::Volume;
use std::borrow::Cow;

use super::super::host::{AnnouncedOnVolume, EngineHost};
use std::path::{Path, PathBuf};

/// A staged write's `.cmdr-tmp-*`: the guard that owns it, and its spelling for the destination volume. Every volume call on the temp
/// addresses [`app`](Self::app); the ledgers and the logs read
/// [`path`](Self::path), so their text stays byte-exact.
pub(super) struct Temp {
    guard: StagingTemp,
    app: AppPathBuf,
}

impl Temp {
    /// The `.cmdr-tmp-*` sibling of `final_path`, carrying `uuid`, minted and
    /// registered as in flight for `owner`.
    ///
    /// A canonical final path holding no escape-range code point (nearly every
    /// one) is split and joined typed, so its folder and last name are the app
    /// path's: on macOS and Linux exactly `Path`'s, and on Windows a POSIX
    /// server's `a\b` stays one name, where `Path` split it into a folder `a`.
    /// Anything else (a bridge value a test or a native local root hands in, a
    /// name whose lossy `std::path` spelling the leftover always wore) is minted
    /// as it always was, through `std::path`.
    pub(super) fn mint(final_path: &AppPath, uuid: uuid::Uuid, owner: StagingOwner) -> Self {
        if let Some((app, name)) = typed_sibling(final_path, uuid) {
            let path = native_text(&app).into_owned();
            return Self {
                guard: StagingTemp::adopt_named(path, name, owner),
                app,
            };
        }
        Self::adopt(StagingTemp::mint_with_uuid(&native_text(final_path), uuid, owner))
    }

    /// A temp already minted on its `std::path` spelling, with its app spelling
    /// made now.
    pub(super) fn adopt(guard: StagingTemp) -> Self {
        let app = AppPath::legacy_unchecked(guard.path()).into_owned();
        Self { guard, app }
    }

    /// The temp as the ledgers and the logs spell it.
    pub(super) fn path(&self) -> &Path {
        self.guard.path()
    }

    /// The temp as its volume addresses it.
    pub(super) fn app(&self) -> &AppPath {
        &self.app
    }
}

/// `final_path`'s temp sibling and its last name, by the app path's own split
/// and join; `None` where [`Temp::mint`] keeps the `std::path` mint.
fn typed_sibling(final_path: &AppPath, uuid: uuid::Uuid) -> Option<(AppPathBuf, String)> {
    typed_scratch(final_path, |name| temp_sibling_name_with_uuid(name, uuid))
}

/// A displaced destination's aside (`volume/displaced_destination.rs`): the
/// `.cmdr-temp-*` sibling of `original` and its last name, by the app path's own
/// split and join, as [`typed_sibling`] names a temp; `None` where the caller keeps
/// the `std::path` mint (`StagingTemp::mint_aside`).
pub(super) fn typed_aside(original: &AppPath, uuid: uuid::Uuid) -> Option<(AppPathBuf, String)> {
    typed_scratch(original, |name| aside_sibling_name_with_uuid(name, uuid))
}

/// The scratch sibling `scratch_name` names beside `final_path`, typed, for a
/// canonical path holding no escape-range code point.
fn typed_scratch(final_path: &AppPath, scratch_name: impl FnOnce(&str) -> String) -> Option<(AppPathBuf, String)> {
    if !final_path.is_canonical() || final_path.as_str().chars().any(is_escape) {
        return None;
    }
    let (folder, name) = (final_path.parent()?, final_path.file_name()?);
    let scratch = scratch_name(name);
    let app = folder.join(AppName::from_app_spelling(&scratch).ok()?).ok()?;
    Some((app, scratch))
}

/// [`native_text`] for a sibling module (the aside mint): one named crossing.
pub(super) fn native_spelling(path: &AppPath) -> Cow<'_, Path> {
    native_text(path)
}

/// Tells the host the engine is about to write `path` on `volume`
/// (`EngineHost::announce_spelled_on`), handing it the `std::path` spelling its
/// local twin is anchored from: the volume engine's one way to announce a write
/// it holds as an app path.
pub(crate) fn announce_app_on<'a>(host: &EngineHost, volume: &dyn Volume, path: &'a AppPath) -> AnnouncedOnVolume<'a> {
    host.announce_spelled_on(volume, path, &native_text(path))
}

/// An app path in the `std::path` spelling the ledgers and logs read: the
/// bridge's way out, ONE named crossing for this module.
fn native_text(path: &AppPath) -> Cow<'_, Path> {
    path.as_std_legacy()
}

/// A path a staged write addresses on its volume, in both spellings made once:
/// the app one for every volume call, the `std::path` one for the ledgers, the
/// logs, and the errors (its text exits, byte-exact on a name that isn't UTF-8).
#[derive(Debug, Clone)]
pub(super) struct Spelled {
    path: PathBuf,
    app: AppPathBuf,
}

impl Spelled {
    pub(super) fn new(app: &AppPath) -> Self {
        Self {
            path: native_text(app).into_owned(),
            app: app.to_owned(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn app(&self) -> &AppPath {
        &self.app
    }
}
