//! What a `..` after a name means on a volume with symlinks: the body behind
//! [`Volume::parent_for_dot_dot`](super::Volume::parent_for_dot_dot) for the two
//! backends whose OS resolves `..` physically (the local POSIX disk and SFTP).
//!
//! POSIX resolves `/a/link/..` to the link TARGET's parent, never to `/a`. So when
//! the path's last name is a symlink, [`parent_across_links`] reads that link and
//! takes the parent of where it points, walking the target's own names the same
//! way (a relative target `../real/dir` is read from the link's folder, and its
//! `..` asks again), and following a target that is itself a link, up to
//! [`MAX_LINK_HOPS`]. Anything else answers the lexical parent: a name that isn't a
//! link, and a name that doesn't exist (so a typed path with a missing folder still
//! reaches its nearest existing ancestor). Every other part of the path stays as
//! given: only a `..` that crosses a link changes the text.
//!
//! Design and the cells: `docs/specs/app-path-plan.md` § "Typed and pasted `..`".

use std::ffi::OsStr;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::app_path::{AppNameBuf, AppPath, AppPathBuf, PathArithmetic};
use crate::os_error::OsErrorClass;

use super::VolumeError;

/// How many links one `..` may follow before it's refused as a loop: Linux's
/// `MAXSYMLINKS`, which macOS's `MAXSYMLINKS` (32) sits under.
pub const MAX_LINK_HOPS: u32 = 40;

/// A boxed, sendable future, the shape a [`LinkReader`] answers in.
pub type LinkFuture<'a> = Pin<Box<dyn Future<Output = Result<Option<PathBuf>, VolumeError>> + Send + 'a>>;

/// The link reads a backend's `..` needs, over the backend's WHOLE namespace (a
/// local disk is one tree, an SFTP server one server), since a link's target can
/// sit outside the volume's root.
pub trait LinkReader: Sync {
    /// The target of the link at `path`, as stored, `Ok(None)` when no link is
    /// there. A missing path answers `NotFound` or `Ok(None)`; both read as "no link".
    fn read_link<'a>(&'a self, path: &'a AppPath) -> LinkFuture<'a>;

    /// The app path of an absolute target's native text (with no `..` in it), or
    /// `None` when it names nothing this volume's app paths can spell.
    fn absolute_target(&self, target: &str) -> Option<AppPathBuf>;
}

/// The lexical parent: the folder holding `path`, or `path` itself at a root. The
/// trait's default, and the answer for any name that isn't a link.
pub fn lexical_parent(path: &AppPath) -> AppPathBuf {
    path.parent().unwrap_or(path).to_owned()
}

/// What `<path>/..` names on a backend whose OS resolves `..` physically: the
/// target's parent when `path`'s last name is a link, else the lexical parent.
///
/// # Errors
///
/// A read that fails for any reason but "nothing there" (a refusal, a dead
/// connection), a target this volume can't spell, or a chain of more than
/// [`MAX_LINK_HOPS`] links. ❌ Never a lexical guess in their place.
pub async fn parent_across_links(reader: &dyn LinkReader, path: &AppPath) -> Result<AppPathBuf, VolumeError> {
    let mut hops = 0;
    parent_of(reader, path.to_owned(), &mut hops).await
}

/// One `..` on `path`, sharing the hop budget with the walk that asked.
fn parent_of<'a>(
    reader: &'a dyn LinkReader,
    path: AppPathBuf,
    hops: &'a mut u32,
) -> Pin<Box<dyn Future<Output = Result<AppPathBuf, VolumeError>> + Send + 'a>> {
    Box::pin(async move {
        let mut current = path;
        loop {
            if current.is_root() {
                return Ok(current);
            }
            let Some(target) = link_at(reader, &current).await? else {
                return Ok(lexical_parent(&current));
            };
            *hops += 1;
            if *hops > MAX_LINK_HOPS {
                return Err(link_loop());
            }
            current = target_path(reader, &current, &target, hops).await?;
        }
    })
}

/// The link at `path`, or `None` when no link is there (nothing at all, or a path
/// running through a file).
async fn link_at(reader: &dyn LinkReader, path: &AppPath) -> Result<Option<PathBuf>, VolumeError> {
    match reader.read_link(path).await {
        Ok(target) => Ok(target),
        Err(VolumeError::NotFound(_)) => Ok(None),
        Err(err) if err.os_class() == Some(OsErrorClass::NotADirectory) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Where the link at `link` points, as an app path with every `..` in the target
/// asked the same way.
async fn target_path(
    reader: &dyn LinkReader,
    link: &AppPath,
    target: &Path,
    hops: &mut u32,
) -> Result<AppPathBuf, VolumeError> {
    // Plain Unicode text only, as every crossing makes until escapes go live (#1225's 10f): a
    // target that isn't is refused, ❌ never spelled lossily. Split on `/` alone, on every host:
    // a link is only followed on a POSIX disk or an SFTP server, where a `\` is part of a name.
    let text = target.to_str().ok_or_else(|| unspellable(target))?;
    let parts: Vec<&str> = text.split('/').collect();
    let (mut at, rest) = if text.starts_with('/') {
        // The leading run with no `..`, spelled as the backend spells it; the rest walks.
        let split = parts.iter().position(|part| *part == "..").unwrap_or(parts.len());
        let head_len = parts[..split]
            .iter()
            .map(|part| part.len() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        let start = reader
            .absolute_target(&text[..head_len.max(1)])
            .ok_or_else(|| unspellable(target))?;
        (start, &parts[split..])
    } else {
        (lexical_parent(link), &parts[..])
    };
    for part in rest {
        match *part {
            "" | "." => {}
            ".." => at = parent_of(reader, at, hops).await?,
            part => {
                let child = AppNameBuf::from_native_os_unicode(OsStr::new(part)).map_err(|_| unspellable(target))?;
                at = at.join(child.as_app_name()).map_err(|_| unspellable(target))?;
            }
        }
    }
    Ok(at)
}

fn link_loop() -> VolumeError {
    VolumeError::IoError {
        message: "too many levels of symbolic links".into(),
        raw_os_error: None,
    }
}

/// A target this volume's app paths can't spell (an absolute one outside an SFTP
/// volume's root, a name no app path holds). Refused: a lexical guess would name a
/// different folder.
fn unspellable(target: &Path) -> VolumeError {
    VolumeError::IoError {
        message: format!("a link target this volume can't follow: {}", target.display()).into(),
        raw_os_error: None,
    }
}

#[cfg(test)]
#[path = "dot_dot_test.rs"]
mod tests;
