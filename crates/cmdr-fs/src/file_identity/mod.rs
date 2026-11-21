//! Which node an item is, which device it's on, and how many names it has: the one
//! vocabulary every local identity question asks in, so no caller reads `st_dev` /
//! `st_ino` / `st_nlink` itself and no OS answers "don't know" as "no".
//!
//! ❗ [`Sameness::Unknown`] is not [`Sameness::Different`]. A caller guarding data (a copy
//! onto itself, a move that deletes its source) asks [`Sameness::may_be_same`] and takes
//! the cautious branch when the OS can't tell. `DETAILS.md` § "File identity".
//!
//! **The rule a transfer follows**, per destination name that's already taken:
//! - `Same` (the source itself, under another spelling): today's in-place handling, a copy
//!   duplicating to a ` (N)` name and a move doing nothing.
//! - `Different`: the ordinary conflict flow.
//! - `Unknown`: the ordinary conflict flow too, asking like any name clash, ❌ never "already
//!   in place" and never a silent skip; and an overwrite the person then picks must itself
//!   refuse on `Unknown`, since the two might be one file and the overwrite would destroy it.
//!
//! Today's callers still take the cautious-but-silent branch on `Unknown` (`may_be_same`: a
//! copy duplicates, a move counts the item in place, a compress onto an existing archive is
//! refused), which `unknown_identity_today_*` tests pin until the Windows local backend
//! brings the rule above with its refusal copy. The one exception is a copy's per-file
//! guard, which already refuses an overwrite it can't prove safe. Unknown never arises on
//! macOS or Linux.
//!
//! **Two ways to read an id.** [`FileId::of`] reads the `Metadata` a caller already holds,
//! which is free on Unix and always `None` on Windows: Windows keeps the id (volume serial
//! plus file id) behind an open handle, never in `Metadata`. [`FileId::at`] and
//! [`same_node_at`] read by PATH, opening a handle on Windows, so they answer there; a data
//! guard holding a path asks them. `DETAILS.md` § "File identity".

mod platform;

#[cfg(any(test, feature = "testing"))]
mod hidden;
#[cfg(any(test, feature = "testing"))]
pub use hidden::{HiddenIdentities, hide_identity_of};

use std::collections::HashSet;
use std::fs::Metadata;
use std::path::Path;

/// Which volume a node lives on, as this OS numbers volumes (`st_dev` on Unix). Only
/// ever compared with another read on the same machine in the same session: ❌ never
/// stored, since a device number can change across a reboot or a replug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(u64);

impl DeviceId {
    /// The OS's own number, for an API that takes one (FSEvents' per-device journal).
    pub fn raw(self) -> u64 {
        self.0
    }

    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// Which node an item is on this machine: its device plus its number there. The node
/// number has room for 128 bits, since a ReFS file id doesn't fit in 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    device: DeviceId,
    node: u128,
}

impl FileId {
    /// The node `metadata` describes (follows a link only if the read that produced
    /// `metadata` did). `None` where this OS's metadata doesn't carry one.
    pub fn of(metadata: &Metadata) -> Option<Self> {
        let id = platform::file_id(metadata)?;
        #[cfg(any(test, feature = "testing"))]
        if hidden::hides(id) {
            return None;
        }
        Some(id)
    }

    /// The node at `path`, read by path: the one reader that answers on Windows, where it
    /// opens a handle (`DETAILS.md` § "File identity"). `None` when `path` can't be read or
    /// the filesystem gives no usable id (a network redirector answering zero).
    pub fn at(path: &Path, follow: Follow) -> Option<Self> {
        Self::read_at(path, follow).ok().flatten()
    }

    /// [`FileId::at`] with the failed read kept: `Err` when `path` can't be read (its kind
    /// tells a missing path from an unreadable one), `Ok(None)` when it can but the
    /// filesystem gives no usable id. One read either way, so a guard that must tell "not
    /// there" from "can't tell" asks this ❌ rather than re-reading the path.
    pub fn read_at(path: &Path, follow: Follow) -> std::io::Result<Option<Self>> {
        let id = platform::file_id_at(path, follow)?;
        #[cfg(any(test, feature = "testing"))]
        if id.is_some_and(hidden::hides) {
            return Ok(None);
        }
        Ok(id)
    }

    /// The device the node lives on.
    pub fn device(self) -> DeviceId {
        self.device
    }

    /// An id read back from a store that kept its two numbers ([`DeviceId::raw`] and
    /// the node number). Only meaningful against ids read on this machine since its
    /// last boot, like any device number.
    pub fn from_stored(device: u64, node: u128) -> Self {
        Self::from_parts(DeviceId::from_raw(device), node)
    }

    /// The node number, for a store that keeps a 64-bit inode. `None` for a number that
    /// doesn't fit, so a wide id reads as "unknown" rather than as a different, truncated one.
    pub fn node_u64(self) -> Option<u64> {
        u64::try_from(self.node).ok()
    }

    pub(crate) fn from_parts(device: DeviceId, node: u128) -> Self {
        Self { device, node }
    }
}

/// Whether a read by path ([`FileId::at`]) follows a final symlink, like `fs::metadata`, or
/// reads the link itself, like `fs::symlink_metadata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    /// The node a final link points at.
    Links,
    /// The link itself.
    NotLinks,
}

/// The device `metadata`'s node lives on. `None` where this OS's metadata doesn't say.
pub fn device_of(metadata: &Metadata) -> Option<DeviceId> {
    #[cfg(any(test, feature = "testing"))]
    if platform::file_id(metadata).is_some_and(hidden::hides) {
        return None;
    }
    platform::device_of(metadata)
}

/// How many names the node has (`st_nlink`). `None` where this OS's metadata doesn't say,
/// which a hardlink dedup reads as "count it": over-counting a size beats losing one.
pub fn link_count(metadata: &Metadata) -> Option<u64> {
    #[cfg(any(test, feature = "testing"))]
    if platform::file_id(metadata).is_some_and(hidden::hides) {
        return None;
    }
    platform::link_count(metadata)
}

/// Whether a walk meets this file's bytes for the first time, for a total that counts
/// each node once (a hardlinked file isn't counted twice). `seen_nodes` holds the
/// 64-bit node numbers met so far. A file with one name (the overwhelmingly common
/// case) skips the set. A link count or node number the OS can't give counts the
/// bytes: over-counting a total beats losing some.
pub fn first_sighting(metadata: &Metadata, seen_nodes: &mut HashSet<u64>) -> bool {
    if link_count(metadata).is_none_or(|links| links <= 1) {
        return true;
    }
    FileId::of(metadata)
        .and_then(FileId::node_u64)
        .is_none_or(|node| seen_nodes.insert(node))
}

/// Whether two reads describe one thing, with "can't tell" kept apart from "no".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sameness {
    /// Both reads carry an identity, and it's the same.
    Same,
    /// Both reads carry an identity, and they differ.
    Different,
    /// At least one read carries no identity on this OS.
    Unknown,
}

impl Sameness {
    /// `true` unless the two are known to differ: the question a data guard asks, so a
    /// platform that can't tell takes the cautious branch.
    pub fn may_be_same(self) -> bool {
        !matches!(self, Self::Different)
    }

    fn of<T: PartialEq>(a: Option<T>, b: Option<T>) -> Self {
        match (a, b) {
            (Some(a), Some(b)) if a == b => Self::Same,
            (Some(_), Some(_)) => Self::Different,
            _ => Self::Unknown,
        }
    }
}

/// Whether `a` and `b` describe the same node.
pub fn same_node(a: &Metadata, b: &Metadata) -> Sameness {
    Sameness::of(FileId::of(a), FileId::of(b))
}

/// Whether the paths `a` and `b` name the same node, links followed ([`FileId::at`]).
/// `Unknown` when either can't be read or carries no id.
pub fn same_node_at(a: &Path, b: &Path) -> Sameness {
    Sameness::of(FileId::at(a, Follow::Links), FileId::at(b, Follow::Links))
}

/// Whether `a` and `b` live on the same device.
pub fn same_device(a: &Metadata, b: &Metadata) -> Sameness {
    Sameness::of(device_of(a), device_of(b))
}

/// The device the node at `path` lives on, links followed, read by path: the one device
/// reader that answers on Windows, where it opens a handle (the volume serial). `None` when
/// `path` can't be read or the OS gives no device.
pub fn device_at(path: &Path) -> Option<DeviceId> {
    #[cfg(any(test, feature = "testing"))]
    if platform::file_id_at(path, Follow::Links)
        .ok()
        .flatten()
        .is_some_and(hidden::hides)
    {
        return None;
    }
    platform::device_at(path)
}

/// Whether the paths `a` and `b` live on the same device, links followed ([`device_at`]).
/// `Unknown` when either can't be read or carries no device.
pub fn same_device_at(a: &Path, b: &Path) -> Sameness {
    Sameness::of(device_at(a), device_at(b))
}

#[cfg(test)]
mod tests;
