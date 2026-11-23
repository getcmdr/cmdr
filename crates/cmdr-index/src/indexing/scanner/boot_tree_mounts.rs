//! The filesystems mounted INSIDE the boot tree, which the boot-disk index stops at.
//!
//! A mount is its own drive: a disk image, an rclone or sshfs mount, pCloud's
//! `~/pCloud Drive`, an NFS share mounted into a home folder, Xcode's `DeviceFS`. So
//! it isn't in `root`'s index, search, or folder sizes, and reads under it route to
//! the mount's own volume id (`paths::routing`). The `/Volumes/` prefix already did
//! this for mounts in the usual place; this is the same rule for every other place.
//!
//! The boot disk's own volumes are not "inside": `/` itself, and every mount point
//! the prefix policy already excludes (`/System/Volumes/Data`, `/System/Volumes/VM`,
//! `/Volumes/X`, `/dev`) never enters the set, so `/System/Volumes/Data/Users/...`,
//! firmlink-normalized to `/Users/...`, stays `root`'s.
//!
//! Read from the host's mount table ([`VolumeProvider::mount_points`], non-blocking)
//! and cached for [`REFRESH_AFTER`]: whoever asks after that re-reads it, so every
//! gate sees a mount within a second of it appearing, with no watcher and no
//! background work while nothing asks. The one walk that can still enter a
//! just-mounted filesystem is one already listing its parent inside that second.
//!
//! [`VolumeProvider::mount_points`]: crate::indexing::host::volumes::VolumeProvider::mount_points

use crate::indexing::paths::app_text::native_text;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use cmdr_fs::firmlinks;
use cmdr_fs::ignore_poison::RwLockIgnorePoison;

use crate::indexing::host;
use crate::indexing::host::volumes::MountPoint;
use crate::indexing::paths::path_prefix::is_at_or_under;

/// How stale the cached table may get before the next ask re-reads it. A read is
/// ~8.5 µs for 17 mounts (`getfsstat(MNT_NOWAIT)`, macOS 26, 2026-09-30), so this
/// bounds the cost at a rounding error while keeping a new mount's window short.
const REFRESH_AFTER: Duration = Duration::from_secs(1);
const REFRESH_AFTER_NANOS: u64 = REFRESH_AFTER.as_secs() * 1_000_000_000;

/// One filesystem mounted inside the boot tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::indexing) struct BootTreeMount {
    /// Firmlink-normalized, the space every index path and every gate works in.
    pub(in crate::indexing) path: String,
    /// As the mount table spells it, which is what the host's volume registry was
    /// told, so it's what a registry lookup has to ask with.
    pub(in crate::indexing) raw: String,
}

/// The set, as of one read of the mount table.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::indexing) struct BootTreeMounts {
    mounts: Vec<BootTreeMount>,
}

impl BootTreeMounts {
    /// The mounts inside the boot tree, out of a whole mount table.
    ///
    /// `already_excluded` is the prefix policy's verdict on a normalized path:
    /// a mount it already keeps the boot scan out of is the boot disk's own
    /// (`/System/Volumes/Data`) or somebody else's that the prefixes handle
    /// (`/Volumes/X`), and either way it's not this set's job.
    ///
    /// Which of the rest it stops at is [`BOOT_TREE_CUT`]'s call: every one on
    /// macOS and Windows, only the ones that aren't a local disk on Linux.
    pub(in crate::indexing) fn from_mount_table(
        mount_points: impl IntoIterator<Item = MountPoint>,
        already_excluded: impl Fn(&str) -> bool,
    ) -> Self {
        Self::from_mount_table_cut(BOOT_TREE_CUT, mount_points, already_excluded)
    }

    /// [`Self::from_mount_table`] under `cut`, so a test pins each platform's rule on
    /// every host.
    pub(super) fn from_mount_table_cut(
        cut: BootTreeCut,
        mount_points: impl IntoIterator<Item = MountPoint>,
        already_excluded: impl Fn(&str) -> bool,
    ) -> Self {
        let mut mounts: Vec<BootTreeMount> = mount_points
            .into_iter()
            .filter(|mount| cut.cuts(mount))
            .filter_map(|mount| {
                let raw = native_text(&mount.path);
                let trimmed = raw.trim_end_matches('/');
                // `/`, the boot disk itself, and a Windows drive's root (`C:`, `D:`): a
                // tree of its own, ❌ never a mount inside one, or `C:` would cut all of `C:`.
                if trimmed.is_empty() || is_drive_root(trimmed) {
                    return None;
                }
                let path = firmlinks::normalize_path(trimmed);
                if path == "/" || already_excluded(&path) {
                    return None;
                }
                Some(BootTreeMount {
                    path,
                    raw: trimmed.to_string(),
                })
            })
            .collect();
        mounts.sort_by(|a, b| a.path.cmp(&b.path));
        mounts.dedup_by(|a, b| a.path == b.path);
        Self { mounts }
    }

    /// The innermost mount at or above `path` (a normalized absolute path), if any.
    pub(in crate::indexing) fn covering(&self, path: &str) -> Option<&BootTreeMount> {
        self.mounts
            .iter()
            .filter(|mount| is_at_or_under(path, &mount.path))
            .max_by_key(|mount| mount.path.len())
    }

    fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }
}

/// Which filesystems mounted inside its tree the boot-disk index stops at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BootTreeCut {
    /// Every one (macOS, Windows): a mount is its own drive, with its own index.
    Every,
    /// Only the ones that aren't a known local disk (Linux): an NFS or CIFS share,
    /// FUSE, sshfs. A local one stays the boot disk's, because cutting it would cut
    /// a separate `/home` partition (Fedora Workstation mounts `/home` as its own
    /// btrfs subvolume) that nothing else indexes: only `/mnt` and `/media` mounts
    /// get their own indexes.
    NotALocalDisk,
}

impl BootTreeCut {
    /// Whether the boot scan stops at `mount`.
    pub(super) fn cuts(self, mount: &MountPoint) -> bool {
        match self {
            Self::Every => true,
            Self::NotALocalDisk => !mount.is_local_disk(),
        }
    }

    /// The rule's name in the exclusion-policy fingerprint, so changing it rebuilds
    /// every boot index built under the old one.
    pub(super) fn fingerprint_tag(self) -> &'static str {
        match self {
            Self::Every => "cut_at_boot_tree_mounts",
            Self::NotALocalDisk => "cut_at_non_local_boot_tree_mounts",
        }
    }
}

impl BootTreeCut {
    /// The rule for `os` (`std::env::consts::OS`). Windows cuts every mount too: each
    /// drive has its own index, and a folder mount of `D:` inside `C:` (`mountvol`) is
    /// `D:`, an NTFS local disk the Linux rule would walk into, writing `D:`'s files into
    /// `C:`'s index at paths `C:` doesn't have (#1431).
    pub(super) const fn for_os(os: &str) -> Self {
        if matches!(os.as_bytes(), b"macos" | b"windows") {
            Self::Every
        } else {
            Self::NotALocalDisk
        }
    }
}

/// This platform's [`BootTreeCut`].
pub(super) const BOOT_TREE_CUT: BootTreeCut = BootTreeCut::for_os(std::env::consts::OS);

/// Whether `path` (no trailing `/`) is a bare drive root, `C:`.
fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// What the cache holds between reads.
struct Cached {
    mounts: Arc<BootTreeMounts>,
}

static CACHE: RwLock<Option<Cached>> = RwLock::new(None);
/// When the cache was last read, in nanoseconds since [`epoch`], and which table
/// generation it read. Both atomics so the common ask (fresh, nothing mounted)
/// touches no lock at all.
static READ_AT_NANOS: AtomicU64 = AtomicU64::new(0);
static READ_GENERATION: AtomicU64 = AtomicU64::new(u64::MAX);
/// Whether the cached set holds anything. Read without the lock on the hot path:
/// every child of every directory a walk lists asks.
static ANY: AtomicBool = AtomicBool::new(false);

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_nanos() -> u64 {
    // +1 so a read at the very first instant never looks like "never read".
    u64::try_from(epoch().elapsed().as_nanos())
        .unwrap_or(u64::MAX)
        .saturating_add(1)
}

/// Whether the cached read is recent enough and of the installed host's table.
fn is_fresh(now: u64) -> bool {
    let read_at = READ_AT_NANOS.load(Ordering::Acquire);
    read_at != 0
        && READ_GENERATION.load(Ordering::Acquire) == host::volumes::table_generation()
        && now.saturating_sub(read_at) < REFRESH_AFTER_NANOS
}

/// Re-read the mount table into the cache. A table that won't answer keeps the
/// previous set: ❌ never an empty one, which would hand every mount's rows back to
/// `root` for as long as the table stays unreadable.
fn refresh(now: u64) {
    let generation = host::volumes::table_generation();
    let read = host::volumes::current().mount_points();
    let mut cache = CACHE.write_ignore_poison();
    if let Some(points) = read {
        let mounts = BootTreeMounts::from_mount_table(points, super::exclusions::excluded_by_boot_prefixes);
        ANY.store(!mounts.is_empty(), Ordering::Release);
        *cache = Some(Cached {
            mounts: Arc::new(mounts),
        });
    } else {
        log::debug!("Boot-tree mounts: the mount table wouldn't answer, so the previous set stands");
    }
    READ_GENERATION.store(generation, Ordering::Release);
    READ_AT_NANOS.store(now, Ordering::Release);
}

/// The current set, re-read first if it's gone stale.
pub(in crate::indexing) fn current() -> Arc<BootTreeMounts> {
    let now = now_nanos();
    if !is_fresh(now) {
        refresh(now);
    }
    CACHE
        .read_ignore_poison()
        .as_ref()
        .map(|cached| Arc::clone(&cached.mounts))
        .unwrap_or_default()
}

/// Whether `path` (normalized, absolute) is a mount point inside the boot tree or
/// sits under one. The cheap question every gate asks: no lock at all while the set
/// is fresh and empty, which it is on most machines.
pub(in crate::indexing) fn is_inside_a_mount(path: &str) -> bool {
    let now = now_nanos();
    if !is_fresh(now) {
        refresh(now);
    }
    if !ANY.load(Ordering::Acquire) {
        return false;
    }
    CACHE
        .read_ignore_poison()
        .as_ref()
        .is_some_and(|cached| cached.mounts.covering(path).is_some())
}

#[cfg(test)]
#[path = "boot_tree_mounts_tests.rs"]
mod tests;
