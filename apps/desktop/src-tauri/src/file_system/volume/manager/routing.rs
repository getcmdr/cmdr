//! Which volume serves a path: [`VolumeManager::resolve`] and what it answers.
//!
//! The registry proper (hold volumes by ID) is the parent `manager` module.
//! This is the dispatcher above the three ROUTES that can swap the volume out
//! from under a path: the git portal (`git_routing.rs`), restic snapshots
//! (`restic_routing.rs`), and archives (`archive_routing.rs`). Each mints a
//! volume on demand, registers it, and caps how many stay registered; each
//! hands the caller's path back verbatim. The dispatcher's one decision is the
//! ORDER: the leftmost confirmed boundary wins. One route nests: an archive
//! inside a restic snapshot reads through the snapshot's view.
//!
//! It's another inherent `impl VolumeManager` block (a type's impl can span
//! files within a crate), so every method stays at `VolumeManager::…`.

use super::super::{ResolvedVolume, RoutedKind};
use super::VolumeManager;
use super::restic_routing::ResticAnswer;
use cmdr_fs::app_path::AppPath;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Whether `path` could be served by a ROUTE rather than by the volume that
/// physically holds it. Pure string work over path segments: no `stat`, no
/// network, and deliberately permissive, because
/// [`VolumeManager::resolve`] is the authoritative answer.
///
/// True for a non-empty archive-inner path, anything inside a repo's virtual
/// `.git` trees while the portal is switched on, and any path at or under a
/// `Browse snapshots` component (a real non-repo folder of that name too: the
/// restic half is a candidate the hub confirms). ❗ The `.zip` FILE itself is NOT
/// one of them — it's an ordinary file, and a copy, a move, and the viewer all
/// have to treat it as bytes on disk.
///
/// Call sites use it as the cheap gate in front of an `await`ed `resolve`, so an
/// ordinary local or remote path pays nothing; whoever then acts on the answer
/// reads [`ResolvedVolume::routed`], never this.
///
/// ❌ Not the same question as
/// [`Volume::routes_over_a_parent`](super::super::Volume::routes_over_a_parent),
/// which asks whether a volume that already EXISTS is one a route minted (what
/// keeps it out of the mount lookup). This one is about a path, and is asked
/// before any volume has been resolved. Keeping them apart is what lets this one
/// stay lexical.
pub fn path_routes_over_its_parent(path: &Path) -> bool {
    let inside_an_archive = cmdr_fs::archive_format::archive_boundary_candidate(path)
        .is_some_and(|(_zip, inner)| !inner.as_os_str().is_empty());
    inside_an_archive
        || crate::file_system::git::wiring::portal_serves(path)
        || cmdr_fs::restic_view::restic_route(path).is_some()
}

/// What a path naming an archive inside a restic snapshot ITSELF (no inner
/// path) resolves to: the snapshot's file, for every caller acting on it, or the
/// archive's root, for a listing entering it ([`VolumeManager::resolve_to_list`]).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArchiveRoot {
    File,
    Folder,
}

/// A place in a path where a route could take over, by the component it sits
/// at. Lexical; each route confirms its own.
enum Boundary {
    /// A `.git/<category>`, under the worktree root that holds it (the portal
    /// decides with no I/O).
    GitPortal(PathBuf),
    /// A `Browse snapshots` component, under the repo folder that holds it.
    Restic(PathBuf),
    /// The first archive-named component.
    Archive,
}

/// [`path_routes_over_its_parent`] for an app path, read off the same `std::path`
/// spelling the route candidates read ([`lexical_spelling`]), so its answer is the
/// `std::path` gate's on the same text, byte for byte.
pub fn app_path_routes_over_its_parent(path: &AppPath) -> bool {
    path_routes_over_its_parent(&lexical_spelling(path))
}

/// `path` as the `std::path` value the git and restic route candidates read
/// (`cmdr_git::portal_route`, `cmdr_fs::restic_view`) and the git portal's
/// overlay rows (`Portal::category_rows`), which still speak
/// `std::path`: the one crossing on the routing seam. The roots those candidates
/// answer are what the portal and the snapshot view are built over, as before.
/// Goes when both candidates take an app path (`cmdr-git`'s surface is capped,
/// so that's David's call; `docs/specs/app-path-plan.md` § 10c's record).
pub(crate) fn lexical_spelling(path: &AppPath) -> Cow<'_, Path> {
    path.as_std_legacy()
}

/// Every route boundary in `path`, leftmost first: the order
/// [`VolumeManager::resolve`] tries them in. The archive's place is read off
/// the same `std::path` spelling as the other two, so the order counts one
/// set of components.
fn boundaries(path: &AppPath) -> Vec<Boundary> {
    let lexical = lexical_spelling(path);
    let mut found: Vec<(usize, Boundary)> = Vec::new();
    if let Some(worktree_root) = cmdr_git::portal_route(&lexical) {
        found.push((worktree_root.components().count(), Boundary::GitPortal(worktree_root)));
    }
    for repo_root in cmdr_fs::restic_view::restic_route_candidates(&lexical) {
        found.push((repo_root.components().count(), Boundary::Restic(repo_root)));
    }
    if let Some((archive, _inner)) = cmdr_fs::archive_format::archive_boundary_candidate(&lexical) {
        found.push((archive.components().count().saturating_sub(1), Boundary::Archive));
    }
    // Two boundaries can't share a component: each is a different name.
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, boundary)| boundary).collect()
}

impl VolumeManager {
    /// Path-aware volume lookup: routes a path that reaches into a repo's
    /// virtual `.git` trees to its read-only `GitPortalVolume`, one at or below
    /// a restic repo's `Browse snapshots` row to its read-only `ResticVolume`,
    /// and one that crosses a `.zip` boundary to its `ArchiveVolume`,
    /// registering each on demand. Everything else is a plain
    /// [`get`](Self::get) with the path unchanged.
    ///
    /// **The leftmost CONFIRMED boundary wins.** Routes nest both ways: a `.zip`
    /// or a `.git/branches` inside a restic snapshot is backed-up data, not a
    /// file on disk, and a restic repo inside a git snapshot is the portal's.
    /// So the boundaries are tried left to right, and one that doesn't confirm
    /// (a real folder named `photos.zip`, a non-repo folder named
    /// `Browse snapshots`, the portal switched off) hands over to the next one,
    /// ❌ never ends routing: a repo inside a real folder named `backups.zip/`
    /// still browses.
    ///
    /// This is `async` for two confirms: a `.zip` boundary on a REMOTE parent
    /// costs a `get_metadata` plus a four-byte `read_range`
    /// ([`resolve_archive`](Self::resolve_archive)), and a restic repo the hub
    /// hasn't met costs one listing of its folder, remembered either way
    /// ([`resolve_restic`](Self::resolve_restic)).
    ///
    /// Adopt this at every site that did `get(volume_id)` then
    /// `volume.method(path)`. The sync-only
    /// [`resolve_local_only`](Self::resolve_local_only) exists for the one
    /// caller that can't `.await` (the write-op fresh-listing oracle).
    pub async fn resolve(&self, volume_id: &str, path: &AppPath) -> ResolvedVolume {
        self.resolve_as(volume_id, path, ArchiveRoot::File).await
    }

    /// [`resolve`](Self::resolve) for a LISTING, the one place an archive's root
    /// is a folder to enter rather than a file to act on. Differs only for an
    /// archive inside a restic snapshot: there the lexical gate every other
    /// caller asks ([`path_routes_over_its_parent`]) can't keep the `.zip` file a
    /// file (everything under `Browse snapshots` routes), so `resolve` keeps its
    /// root on the view, and only a listing enters it. A plain archive's root
    /// routes the same either way, since the gate already answers for it.
    pub async fn resolve_to_list(&self, volume_id: &str, path: &AppPath) -> ResolvedVolume {
        self.resolve_as(volume_id, path, ArchiveRoot::Folder).await
    }

    async fn resolve_as(&self, volume_id: &str, path: &AppPath, archive_root: ArchiveRoot) -> ResolvedVolume {
        let mut undecided = false;
        for boundary in boundaries(path) {
            let routed = match boundary {
                Boundary::GitPortal(worktree_root) => self.resolve_git_portal(volume_id, worktree_root, path),
                Boundary::Restic(repo_root) => match self.resolve_restic(volume_id, &repo_root, path).await {
                    ResticAnswer::Routed(routed) => Some(
                        self.inside_the_snapshot(volume_id, &repo_root, routed, path, archive_root)
                            .await,
                    ),
                    ResticAnswer::PassOn => None,
                    ResticAnswer::Undecided => {
                        undecided = true;
                        None
                    }
                },
                Boundary::Archive => {
                    Some(self.resolve_archive(volume_id, path).await).filter(ResolvedVolume::is_routed)
                }
            };
            if let Some(routed) = routed {
                return routed;
            }
        }
        if undecided {
            // Reads go to the drive; a write refuses rather than write into what
            // may be a repo's read-only view.
            return ResolvedVolume::undecided(self.get(volume_id), path, RoutedKind::Restic);
        }
        ResolvedVolume::passthrough_at(self.get(volume_id), path)
    }

    /// What serves a path the restic view `routed` took: an archive inside the
    /// snapshot when the path crosses into one the view confirms (#631), else the
    /// view. The one nesting a route does: an archive reads through any `Volume`,
    /// so it reads through a snapshot's too.
    async fn inside_the_snapshot(
        &self,
        volume_id: &str,
        repo_root: &Path,
        routed: ResolvedVolume,
        path: &AppPath,
        archive_root: ArchiveRoot,
    ) -> ResolvedVolume {
        let Some(view) = routed.volume.clone() else {
            return routed;
        };
        let view_id = super::restic_routing::restic_volume_id(volume_id, repo_root);
        self.resolve_archive_in_snapshot(&view_id, view, path, archive_root)
            .await
            .unwrap_or(routed)
    }

    /// Sync sibling of [`resolve`](Self::resolve), with the same leftmost order,
    /// that does no network or remote I/O. It confirms **only local** archive
    /// boundaries (a remote `.zip` path passes through, its confirm needs async
    /// I/O, and so does an archive inside a snapshot, which the view answers
    /// for), routes restic only on the hub's memo (❌ no confirm listing: a
    /// `read_dir` on a wedged OS mount would block a scan walker), and routes git
    /// identically, since that's lexical.
    ///
    /// The ONE caller is the write-op fresh-listing oracle
    /// (`Listings::authoritative_listing`), which runs on sync recursive scan
    /// walkers. That oracle declines a remote archive path and an unrouted path
    /// under a `Browse snapshots` component on its own, so a passthrough here
    /// only costs a re-read.
    pub fn resolve_local_only(&self, volume_id: &str, path: &AppPath) -> ResolvedVolume {
        for boundary in boundaries(path) {
            let routed = match boundary {
                Boundary::GitPortal(worktree_root) => self.resolve_git_portal(volume_id, worktree_root, path),
                Boundary::Restic(repo_root) => self.resolve_restic_from_memo(volume_id, &repo_root, path),
                Boundary::Archive => {
                    Some(self.resolve_local_archive(volume_id, path)).filter(ResolvedVolume::is_routed)
                }
            };
            if let Some(routed) = routed {
                return routed;
            }
        }
        ResolvedVolume::passthrough_at(self.get(volume_id), path)
    }
}

/// Records `id` as the most recently resolved entry in a routed-volume LRU and
/// hands back the IDs that fell off the end, for the caller to unregister
/// OUTSIDE the LRU lock (so the LRU and volumes locks are never held at once).
///
/// Shared by the archive and git-portal routes: both mint volumes on demand and
/// both must stay bounded, and a second copy of this would drift.
pub(super) fn touch_routed_lru(lru: &mut VecDeque<String>, id: &str, cap: usize) -> Vec<String> {
    lru.retain(|existing| existing != id);
    lru.push_back(id.to_string());
    let mut evicted = Vec::new();
    while lru.len() > cap {
        if let Some(old) = lru.pop_front() {
            evicted.push(old);
        }
    }
    evicted
}
