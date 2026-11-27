//! Merging one directory tree into another, level by level.
//!
//! The tree walk half of the cross-volume engine: [`copy_directory_streaming`]
//! walks a source directory into a destination, resolving deep conflicts
//! inline as it discovers them. Its siblings own the other half: `strategy.rs`
//! dispatches one source, and `stream_pipe.rs` gets ONE file's bytes from A to
//! B (staging, the write itself, and the two cancel tiers). A directory child
//! is a level the walk descends into, a file child goes to `stream_pipe.rs`.
//! The walk keeps one frame per level on a heap stack, so a tree's depth never
//! costs stack (#1541).
//!
//! Shared vocabulary (`MergeCtx`, `CreatedPaths`, `FileWindow`) lives in
//! `merge_ctx.rs` because both halves, both drivers, and `sequential_extract.rs`
//! speak it.
//!
//! The walk DISCOVERS serially and COPIES concurrently: one walker descends the
//! source in tree order and resolves every conflict on itself, exactly as it
//! always did, while each file's byte copy joins the operation-wide
//! `merge_ctx.rs::FileWindow` and overlaps its siblings. `DETAILS.md`
//! § "One window for the whole operation".
//!
//! The merge invariant this file has to keep: a merge never deletes or
//! overwrites a destination file the source doesn't shadow, under every policy,
//! backend, and cancel/rollback/retry mid-merge. Assert it through
//! `safety_oracle.rs`, never fresh inline asserts. See `CLAUDE.md` § Merge and
//! conflicts, and `DETAILS.md` § "Scan-as-you-merge".

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use super::super::super::state::WriteOperationState;
use super::super::super::transfer_tree::{NodeId, TreeLevel, TreeOrder};
use super::super::dest_name_index::DestNameIndex;
use super::super::transfer_driver::SourceProgress;
use super::super::transfer_probe::{TaskPhase, set_task_phase};
use super::super::volume_outage::through_outages;
use super::conflict::{ResolvedConflict, resolve_volume_conflict};
use super::folder_metadata::SourceFolderDates;
use super::landing::{DestFolder, NewName, where_it_lands};
use super::link_cycle::{Descending, RootLinks, WalkPath, descend_into, skip_link_back_up, walk_root};
use super::listed_name::{ListedName, into_std, listed_path, shown_under, std_of};
use super::merge_ctx::{CreatedPaths, FileWindow, MergeCtx};
use super::merge_leaves::{
    LeafPool, LeafRow, RefusedChild, RefusedFolder, copy_leaf, copy_link_leaf, refuse_child, skip_if_folder_refused,
};
use super::preflight::SourceFileFacts;
use super::rename_merge::merges_as_a_directory;
use super::strategy::Replaces;
use super::strategy::{LandingName, staging_for};
use super::transfer_error::{AtPath, PathRole, PathedVolumeError};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{DirectoryCreation, Volume, VolumeError};

/// Copies (merges) a directory tree from source to destination, streaming each
/// file through `write_from_stream`. Cancellation is checked between entries.
///
/// The walk itself is [`Walk::run`]; this is the entry point that owns the
/// subtree's leaf pool and guarantees it is drained before returning.
///
/// ## Scan-as-you-merge
///
/// The merge discovers deep conflicts inline, level by level, with no upfront
/// recursive pre-scan. The trigger is the destination directory's existence:
///
/// - `create_directory` returns `Ok(())` ⇒ WE created this level fresh. Nothing
///   inside it can clash, so we skip the dest listing entirely and stream every
///   source child straight in.
/// - `create_directory` returns `AlreadyExists` ⇒ we're MERGING into the user's
///   pre-existing directory. We list the dest level ONCE and index it into the
///   `DestNameIndex` the top-level pre-check uses, then for each source child
///   the index reports as taken we dispatch through the conflict resolver (file
///   policy: Stop-wait, latch, conditional reduce, type mismatches) — EXCEPT
///   dir-vs-dir, which descends unconditionally (a folder landing on a folder
///   always merges, never prompts). A child the index reports free is copied
///   straight in. One listing per level, in-memory lookups after; the only
///   `get_metadata` is for a name the listing can't settle, which on an
///   ordinary tree is none of them (`landing.rs::where_it_lands`). A child the
///   destination holds under another Unicode spelling is a clash with THAT
///   entry, and the walk addresses it by its stored name from then on.
///
/// The `Ok` vs `AlreadyExists` split also drives rollback: `Ok` records the dir
/// in `created` (rollback may remove it once empty); `AlreadyExists` does NOT,
/// so rollback never touches the user's pre-existing directory — only the files
/// we wrote into it. This is what keeps a merge from destroying dest-only files.
///
/// When `merge` is `None`, there's no per-child conflict resolution: a clashing
/// dest file is overwritten blindly (the cross-volume move's copy phase, where
/// the dest is fresh staging, plus tests that never merge). `Some` is what the
/// volume copy / cross-volume move pipelines pass so deep clashes honor policy.
/// `None` also means no window, so those callers keep a strictly serial walk.
///
/// ## The window
///
/// Discovery stays serial and copying goes wide: this function walks the tree and
/// resolves every conflict on ONE walker, in tree order, then hands each
/// file's byte copy to the operation-wide [`FileWindow`] on `MergeCtx`. Prompt
/// order, the apply-to-all latch, and the order directories are created in are
/// therefore all exactly what they were when the whole walk was serial; only the
/// bytes overlap. PLAN MODE never streams a byte, so it never touches the window.
///
/// ## Depth
///
/// A tree's depth lives on the heap, in [`Walk::run`]'s stack of [`Frame`]s,
/// ❌ never in nested futures: a future per folder level nests a poll per level,
/// and that overflowed a copy worker's 2 MiB stack around 50 levels down
/// (#1541, `DETAILS.md` § "The walk keeps its depth on the heap").
#[allow(
    clippy::too_many_arguments,
    reason = "Mirrors copy_single_path's argument list plus the rollback ledger, merge context, and the sequential-extract plan sink; bundling into a struct adds ceremony without cleaning anything up."
)]
pub(super) async fn copy_directory_streaming(
    source_volume: &Arc<dyn Volume>,
    source_path: &AppPath,
    dest_volume: &Arc<dyn Volume>,
    dest_path: &AppPath,
    state: &Arc<WriteOperationState>,
    created: &CreatedPaths,
    progress: &Arc<SourceProgress>,
    merge: Option<&MergeCtx<'_>>,
    // `Some` ⇒ PLAN MODE for the one-pass sequential extractor: create the
    // destination directory structure and resolve every file's conflict as usual,
    // but instead of streaming each file's bytes, record its resolved destination
    // in the plan and leave the byte write to the caller's single decode pass.
    // `None` ⇒ normal streaming copy.
    plan: Option<&super::sequential_extract::ExtractPlan>,
    // The source folder's own dates, from the scan's stat of it
    // (`SourceHint`): the one folder no listing in the walk carries. Stamped on
    // `dest_path` once the subtree landed, if the walk created it.
    source_dates: SourceFolderDates,
) -> Result<u64, PathedVolumeError> {
    // ONE pool for this whole subtree, so a file at depth 5 shares the window
    // with a file at depth 1 instead of opening one of its own per level.
    let mut pool = LeafPool::new(
        merge.map_or_else(FileWindow::serial, |ctx| ctx.window.clone()),
        merge.and_then(|ctx| ctx.probe.clone()),
    );
    let walk = Walk {
        source_volume,
        dest_volume,
        state,
        created,
        progress,
        merge,
        plan,
        root: source_path,
    };
    // The walk holds its destination as an app path from here down, every
    // child joined typed (`listed_name.rs::ListedName`).
    let walked = walk
        .run(
            &mut pool,
            LevelAt {
                source: source_path.to_owned(),
                dest: dest_path.to_owned(),
                // The walk starts at the source's own row and looks every child
                // up under it, level by level.
                folder: progress.node(),
                path: walk_root(source_volume.as_ref(), source_path).await,
            },
        )
        .await;
    // Unconditional: nothing may still be writing to the destination when this
    // returns, whether the walk finished, failed, or hit a cancel.
    pool.drain().await;
    let root = match walked {
        // The walk's own error is the FIRST failure by construction (it stops
        // the moment a leaf reports one), so it outranks anything the drain
        // then collected from leaves that were already in flight.
        Err(e) => return Err(e),
        Ok(root) => root,
    };
    if let Some(e) = pool.first_error.take() {
        return Err(e);
    }
    // The last leaves' refused names, now that nothing is in flight.
    pool.settle_refused(merge, created, progress).await?;

    // Every leaf landed, so no write can bump a folder's date after this. A
    // failed or cancelled subtree returned above and dates nothing.
    let mut folders = std::mem::take(&mut pool.folders);
    if root == DirectoryCreation::Created {
        folders.note_filled(dest_path.to_owned(), std_of(source_path).into_owned(), source_dates);
    }
    match plan {
        // PLAN MODE wrote no file yet: the data pass dates the folders once its
        // decode lands every member.
        Some(plan) => plan.hold_folder_metadata(folders),
        None => folders.stamp(source_volume, dest_volume, state).await,
    }
    Ok(pool.bytes)
}

/// What every level of one walk reads: both volumes, the operation, its ledgers,
/// and how a deep clash is settled.
struct Walk<'a, 'm> {
    source_volume: &'a Arc<dyn Volume>,
    dest_volume: &'a Arc<dyn Volume>,
    state: &'a Arc<WriteOperationState>,
    created: &'a CreatedPaths,
    progress: &'a Arc<SourceProgress>,
    merge: Option<&'m MergeCtx<'m>>,
    plan: Option<&'m super::sequential_extract::ExtractPlan>,
    /// The source folder the walk started at, for the links at it and above it
    /// (`cmdr_fs::volume::link_walk::RootLinks`).
    root: &'m AppPath,
}

/// One folder level of the walk: one on the walk's stack per level it is inside.
struct Frame {
    at: LevelAt,
    /// `None` until the walk prepares the level (its create and listings), the
    /// next thing it does once the level is pushed.
    open: Option<Level>,
    /// What the level above does once this one ends; `None` for the walk's
    /// root, whose outcome is the walk's own.
    then: Option<OnEnd>,
}

/// Where a level sits: its folder on each side, and its row in the transfer tree.
struct LevelAt {
    /// In the source's spelling: what its listing reads, and what every child's
    /// own listed path (`listed_name.rs::listed_path`) hangs under.
    source: AppPathBuf,
    /// In the destination's spelling: what its stat, its creates, and every
    /// child's typed join read.
    dest: AppPathBuf,
    /// Observation only: whether it's `None`, or what the tree makes of a child,
    /// changes nothing below.
    folder: Option<NodeId>,
    /// The folders above it by node, and the links followed to reach it: what a
    /// child folder is checked against before the walk descends into it.
    path: WalkPath,
}

/// A prepared level: its sorted listing and how far the walker is through it,
/// plus what the destination already held there.
struct Level {
    /// The folder every child joins under, its canonical twin looked up ONCE per
    /// level: a top-level bridge value (`C:\x` on Windows, `/a/./b`) would
    /// otherwise be re-read and rebuilt for each child's join. The same place.
    join_dir: AppPathBuf,
    entries: Vec<FileEntry>,
    /// Per entry, whether an earlier entry of this listing has its name. A
    /// source can list one name twice (two same-named items on a phone or
    /// camera, a zip holding a path twice), and the second can't take the
    /// level's answer: the first is landing on that name (`DETAILS.md` § "A
    /// source listing one name twice").
    repeats: Vec<bool>,
    /// The next entry to walk.
    next: usize,
    /// The destination level by name, for a level that was there before
    /// (`None`: this walk made it, so nothing in it can clash).
    dest_index: Option<DestNameIndex>,
    /// THIS walk's `create_directory` made the level (proof it was empty).
    made_here: bool,
    /// The level's children by name, for each entry's tree row.
    tree: TreeLevel,
}

/// What a child folder's level hands its parent once the child's subtree ends,
/// beside the child's own [`LevelAt`]: what followed the call when each level
/// was a recursive call of its parent's ([`Walk::settle`]).
struct OnEnd {
    /// The folder's dates, from the parent's listing: noted if the walk created it.
    dates: SourceFolderDates,
    /// Whether a failure of the folder's own name is the refused-name question
    /// (`skip_if_folder_refused`). Not for a folder that took a same-named
    /// leaf's place: the destination already stores that name.
    asks_if_refused: bool,
}

/// What a level's walk came to, for now.
enum Advance {
    /// It reached a child folder, landing at `LevelAt::dest` (its own name, or
    /// the name a resolved type mismatch freed): walk that first, then come back.
    Descend(LevelAt, OnEnd),
    /// Every entry is walked.
    Finished,
}

impl<'a> Walk<'a, '_> {
    /// Walks the tree under `root`, depth first in tree order, keeping one
    /// [`Frame`] per level it is inside. Answers whether THIS walk created the
    /// root level (`Created`), which is what earns a folder its source's date.
    async fn run(&self, pool: &mut LeafPool<'a>, root: LevelAt) -> Result<DirectoryCreation, PathedVolumeError> {
        let mut inside = vec![Frame {
            at: root,
            open: None,
            then: None,
        }];
        let mut root_links = RootLinks::default();
        loop {
            let frame = inside
                .last_mut()
                .expect("the walk returns as its root level ends, so it is always inside one");
            let ended = match frame.open.as_mut() {
                None => match self.open_level(pool, &frame.at).await {
                    Ok(level) => {
                        frame.open = Some(level);
                        continue;
                    }
                    Err(e) => Err(e),
                },
                Some(level) => match self.advance(pool, &frame.at, level, &mut root_links).await {
                    Ok(Advance::Descend(at, then)) => {
                        inside.push(Frame {
                            at,
                            open: None,
                            then: Some(then),
                        });
                        continue;
                    }
                    Ok(Advance::Finished) => Ok(level.finish(&frame.at, self.progress)),
                    Err(e) => Err(e),
                },
            };
            // The top level ended. Its parent takes the outcome, and a failure
            // it passes on ends that level too, so climb until a level goes on.
            // The ended level's listing drops here, before its parent acts.
            let mut outcome = ended;
            loop {
                let Frame { at, then, .. } = inside.pop().expect("the level that ended is on top");
                let Some(then) = then else {
                    return outcome;
                };
                match self.settle(pool, at, then, outcome).await {
                    Ok(()) => break,
                    Err(e) => outcome = Err(e),
                }
            }
        }
    }

    /// Prepares the level at `at`: makes or finds its destination folder,
    /// indexes that folder if it was there before, and lists the source level
    /// in tree order.
    async fn open_level(&self, pool: &mut LeafPool<'a>, at: &LevelAt) -> Result<Level, PathedVolumeError> {
        let Walk {
            source_volume,
            dest_volume,
            state,
            created,
            progress,
            ..
        } = *self;
        let source_path: &AppPath = &at.source;
        let dest_path_app: &AppPath = &at.dest;
        // The same level for the bodies that still take `std::path` (the ledger,
        // the host's announce, the listing seam): a borrow for UTF-8.
        let dest_path_std = std_of(dest_path_app);
        let dest_path: &Path = &dest_path_std;
        let join_dir = match dest_path_app.to_canonical() {
            Ok(canonical) => canonical.into_owned(),
            Err(_) => dest_path_app.to_owned(),
        };
        state.scope().host().announce_on(&**dest_volume, dest_path);
        // Say what this task is doing before the first `.await` of the level. A
        // walk parks on listings, so a stack sample sees nothing and the dump would
        // otherwise still read `spawned` however long the tree takes. Each leaf
        // sets its own phase once it starts streaming, and the next level sets this
        // one back.
        set_task_phase(TaskPhase::Walking);

        // The source listing is needed at EVERY level, whatever the destination
        // turns out to hold, and it is independent of the whole destination chain
        // below. So the two run concurrently rather than in sequence: on a
        // cross-share merge each leg is a full network round trip, and paying them
        // one after the other doubled the walk's cost per directory (measured
        // ~1.2 s/dir SMB→SMB in a user's bundle, `ERR-AYVM4`, where the walk WAS the
        // transfer).
        //
        // ❗ Unless either side is a single-transport backend. MTP reports
        // `max_concurrent_ops() == 1` because it is one USB bulk transport, and the
        // device lock it takes is released before the PTP transaction runs, so two
        // overlapping calls would interleave transactions on one phone. The window
        // keeps every LEAF serial there for the same reason; these two listings sit
        // outside the window, so they have to honor it themselves.
        //
        // Nothing about ordering changes either way: every decision — prompt order,
        // the apply-to-all latch, the order directories are created in — is made
        // walking the entries (`advance`), after both legs have landed.
        let legs_may_overlap = source_volume.max_concurrent_ops() > 1 && dest_volume.max_concurrent_ops() > 1;

        // Ensure the destination directory exists, learn whether THIS level
        // pre-existed (a merge) or we created it fresh, and for a merge build the
        // name→entry map ONCE. A freshly-created level can't clash, so we never
        // list it.
        //
        // Every backend EXCEPT MTP surfaces "already exists" as
        // `VolumeError::AlreadyExists` (SMB needs smb2 ≥ 0.8.0 to typed-classify
        // STATUS_OBJECT_NAME_COLLISION). MTP's `create_directory` does NOT error on
        // a same-name dir — the MTP protocol allows same-name sibling objects, so a
        // blind `create_folder` would make a duplicate `photos` and the merge would
        // target the WRONG dir. So on MTP (and any backend whose `create_directory`
        // can't be trusted to error on collision) we pre-check existence with the
        // one listing the merge level pays anyway, and skip the create when present.
        // The level's prepare is a listing and an idempotent create, so a share
        // that drops under it is waited for and the prepare runs again: a create
        // that landed before the drop answers `AlreadyExists` the second time, which
        // reads as "there" (`../volume_outage.rs`, ❗ only for calls that are safe to
        // run twice).
        let level = pool
            .drive(through_outages(
                state,
                &[source_volume, dest_volume],
                move || async move {
                    let dest_prepare = async {
                        // THIS walk's `create_directory` made the level (proof it was empty),
                        // ❌ never the `NotSupported` "treat as fresh" case below.
                        let mut made_here = false;
                        let level_pre_existed = if backend_create_directory_detects_collisions(dest_volume) {
                            match dest_volume.create_directory(dest_path_app).await {
                                Ok(()) => {
                                    created.record_dir(dest_path_app.to_owned());
                                    // Made here, so it is the extract's to quarantine, now rather than
                                    // in a pass a cancel or failure would skip (`extract_quarantine.rs`).
                                    super::extract_quarantine::quarantine_created_folder(
                                        state,
                                        source_volume,
                                        dest_volume,
                                        dest_path_app,
                                    )
                                    .await;
                                    made_here = true;
                                    false
                                }
                                Err(VolumeError::AlreadyExists(_)) => true,
                                Err(VolumeError::NotSupported) => {
                                    // Backend can't create directories at all; assume
                                    // `write_from_stream` materializes parents on demand (LocalPosix
                                    // does via `create_dir_all` semantics). Treat as fresh.
                                    false
                                }
                                Err(e) => return Err(e),
                            }
                        } else {
                            // Untrusted-collision backend (MTP): pre-check existence.
                            if dest_volume.exists(dest_path_app).await {
                                true
                            } else {
                                match dest_volume.create_directory(dest_path_app).await {
                                    Ok(()) => {
                                        created.record_dir(dest_path_app.to_owned());
                                        // Made here, so it is the extract's to quarantine, now rather than
                                        // in a pass a cancel or failure would skip (`extract_quarantine.rs`).
                                        super::extract_quarantine::quarantine_created_folder(
                                            state,
                                            source_volume,
                                            dest_volume,
                                            dest_path_app,
                                        )
                                        .await;
                                        made_here = true;
                                        false
                                    }
                                    // A race created it between the check and the create; merge.
                                    Err(VolumeError::AlreadyExists(_)) => true,
                                    Err(VolumeError::NotSupported) => false,
                                    Err(e) => return Err(e),
                                }
                            }
                        };

                        // A level we created ourselves holds nothing, so there is no index to
                        // build and no name to look up: `None` is the answer for every child.
                        // A link the destination listing hid is marked, so nothing merges
                        // into it (`crate::hidden_links`); a source link is carried.
                        let dest_index = if level_pre_existed {
                            let listed =
                                crate::hidden_links::list_marking_hidden_links(dest_volume.as_ref(), dest_path);
                            Some(DestNameIndex::build(listed.await?))
                        } else {
                            None
                        };
                        Ok((dest_index, made_here))
                    };

                    let (dest_index, entries) = if legs_may_overlap {
                        tokio::join!(dest_prepare, source_volume.list_directory_as_source(source_path))
                    } else {
                        (
                            dest_prepare.await,
                            source_volume.list_directory_as_source(source_path).await,
                        )
                    };
                    // Each leg's failure is its own volume's: a share that won't create the
                    // nested folder is the destination's refusal, at the destination path.
                    Ok::<_, PathedVolumeError>((dest_index.at_destination(dest_path)?, entries.at_source(source_path)?))
                },
            ))
            .await?;
        let ((dest_index, made_here), mut entries) = level;
        // An OS junk file the person asked to leave out is no part of the transfer:
        // never copied, and never in the tree or the totals the scan already left
        // it out of. A move's sweep removes it with its folder (`source_sweep.rs`).
        entries.retain(|entry| !state.os_junk.leaves_out_str(&entry.name, merges_as_a_directory(entry)));
        // Tree order, whatever order the backend listed in (SMB and MTP don't
        // sort): the walker starts files, asks conflicts, and creates folders in
        // the order the progress tree shows. `DETAILS.md` § "Processing order".
        TreeOrder::active(state.scope().host()).sort(&mut entries, |entry| {
            (merges_as_a_directory(entry), entry.name.as_str())
        });
        // A move sweeps exactly the folders this walk listed; anything else it
        // finds in the source afterwards arrived later and stays.
        created.record_walked_source_dir(source_path);
        let tree = progress.tree().level(
            at.folder,
            entries.iter().map(|entry| {
                (
                    entry.name.as_str(),
                    merges_as_a_directory(entry),
                    entry.size.unwrap_or(0),
                )
            }),
        );
        let repeats = {
            let mut seen: HashSet<&str> = HashSet::with_capacity(entries.len());
            entries.iter().map(|entry| !seen.insert(entry.name.as_str())).collect()
        };
        Ok(Level {
            join_dir,
            entries,
            repeats,
            next: 0,
            dest_index,
            made_here,
            tree,
        })
    }

    /// Walks `level`'s entries from where it left off: submits each file child
    /// to `pool`, settles each clash, and stops at the first child folder to
    /// descend into, which the walk takes next.
    async fn advance(
        &self,
        pool: &mut LeafPool<'a>,
        at: &LevelAt,
        level: &mut Level,
        root_links: &mut RootLinks,
    ) -> Result<Advance, PathedVolumeError> {
        let Walk {
            source_volume,
            dest_volume,
            state,
            created,
            progress,
            merge,
            plan,
            root,
        } = *self;
        let source_path: &AppPath = &at.source;
        let dest_path_app: &AppPath = &at.dest;
        let dest_path_std = std_of(dest_path_app);
        let dest_path: &Path = &dest_path_std;
        let Level {
            join_dir,
            entries,
            repeats,
            next,
            dest_index,
            made_here,
            tree: tree_level,
        } = level;
        let join_dir: &AppPath = join_dir;
        let level_made_here = *made_here;
        while let Some(entry) = entries.get(*next) {
            let repeat = repeats[*next];
            *next += 1;
            // The cooperative boundary, per entry. The walk's own work (listings,
            // destination creation, a conflict decision per child) never passes
            // through the between-chunks checkpoint the byte path parks at, so this
            // is the only thing that stops a paused merge from walking on.
            if pool.drive(state.stop_or_park_async()).await {
                return Err(VolumeError::Cancelled("Operation cancelled by user".into())).at_source(source_path);
            }

            // The child as its own listing row names it, typed (an MTP twin's path
            // isn't its folder joined with its name). The leaf bodies take it typed;
            // the seams still keyed by `std::path` (a link's target, the Locked
            // plan, the resolver) read its `std::path` spelling, a borrow for UTF-8.
            let child_source = listed_path(entry).at_source(source_path)?.into_owned();
            // A repeat waits for what's in flight to land, then asks the destination
            // afresh, so the earlier item is an ordinary clash for it.
            if repeat {
                pool.drain().await;
            }
            // The level listing settles almost every child: a byte-exact hit, a
            // look-alike (taken, in ITS spelling), or a name nothing folds onto. Only
            // a case-only match costs a probe, and on an ordinary tree none does.
            let dest_folder = match dest_index.as_ref() {
                _ if repeat => DestFolder::Unlisted,
                Some(index) => DestFolder::Listed(index),
                None => DestFolder::CreatedByUs,
            };
            // ❗ The source's listing named this child, and a hostile server or
            // device can name it `../x` or `/x`: ❌ never join it raw.
            let name = ListedName::new(&entry.name)
                .map_err(VolumeError::from)
                .at_source(&*child_source)?;
            // A plain name this folder's path can't hold (a drive or UNC folder
            // takes no `\`: a POSIX server's `a\b` copied onto NTFS) is the
            // destination refusing THIS item, asked per item like any refused name,
            // ❌ never the level's failure, which would blame (and, on a Skip, drop)
            // the whole folder. Nothing is written for it, so the place the question
            // names is display text only: the folder's text and the name as listed.
            if name.under(join_dir).is_err() {
                let child_row = tree_level.child(&entry.name, merges_as_a_directory(entry), entry.size.unwrap_or(0));
                let refused = RefusedChild {
                    entry,
                    source: child_source,
                    dest: shown_under(join_dir, &entry.name),
                    node: child_row,
                };
                refuse_child(pool, merge, created, progress, source_volume, refused).await?;
                continue;
            }
            let (child_dest, dest_hit) = pool
                .drive(where_it_lands(
                    dest_volume,
                    join_dir,
                    &name,
                    dest_folder,
                    NewName::Respell,
                ))
                .await
                // The probe asked the destination, about the name the child would take.
                .map_err(|e| match name.under(join_dir) {
                    Ok(asked) => PathedVolumeError::new(PathRole::Destination, &asked, e),
                    Err(_) => PathedVolumeError::new(PathRole::Destination, dest_path, e),
                })?
                .into_parts();
            let dest_hit = dest_hit.as_ref();
            // The scan's kind, which counts a link as one entry even where this
            // walk descends through it (a backend whose listing follows links).
            let child_row = tree_level.child(&entry.name, merges_as_a_directory(entry), entry.size.unwrap_or(0));

            // A name the destination refuses is refused before anything is written
            // for it: a driver that strips it (vfat's trailing period) would land it
            // on a sibling, or merge a folder into one
            // (`crate::name_rules::DestinationNames`). Asked like a landing's refusal.
            if state.destination_names.refuses(std::ffi::OsStr::new(&entry.name)) {
                let refused = RefusedChild {
                    entry,
                    source: child_source,
                    // The place the asked name takes: it joined above, so it joins here.
                    dest: name.under(join_dir).unwrap_or_else(|_| dest_path_app.to_owned()),
                    node: child_row,
                };
                refuse_child(pool, merge, created, progress, source_volume, refused).await?;
                continue;
            }

            // A Locked child the person said to leave (or the system locked) stays
            // out whole, recorded like a deep Skip so a move's sweep keeps it.
            if merge
                .and_then(|ctx| ctx.locked)
                .is_some_and(|locked| locked.skips(&std_of(&child_source)))
            {
                let skipped_bytes = if entry.is_directory { 0 } else { entry.size.unwrap_or(0) };
                created.record_skip(child_source.clone(), skipped_bytes);
                progress.skip_leaf(skipped_bytes, child_row);
                continue;
            }

            // A link this copy lands as a link is a leaf, whatever it points at; any
            // other link takes the listing's word (`link_carry.rs`).
            let link_target = match merge.and_then(|ctx| ctx.links) {
                Some(links) if entry.is_symlink && plan.is_none() => {
                    pool.drive(through_outages(state, &[source_volume, dest_volume], || async {
                        links
                            .target_of_listed(state, source_volume, dest_volume, &std_of(&child_source))
                            .await
                            .at_source(&*child_source)
                    }))
                    .await?
                }
                _ => None,
            };
            let descends = entry.is_directory && link_target.is_none();

            // A folder this walk is already inside makes the tree endless
            // (ERR-BGEKC): every folder it would descend is checked by node, a
            // link the listing didn't mark included (Samba lists one as a folder).
            let child_path = if descends {
                let into = pool
                    .drive(descend_into(
                        source_volume.as_ref(),
                        &child_source,
                        entry.is_symlink,
                        &at.path,
                        root,
                        root_links,
                    ))
                    .await
                    .at_source(&*child_source)?;
                match into {
                    Descending::LoopsBack => {
                        skip_link_back_up(entry, child_source, created, progress, child_row);
                        continue;
                    }
                    Descending::Into(path) => Some(path),
                }
            } else {
                None
            };

            if let Some(path) = &child_path {
                // Dir-vs-dir (and dir-into-nothing) always descends to merge — no
                // resolver call for the folder itself. A dir landing on a same-named
                // LEAF is a type mismatch, which the resolver (below) handles.
                //
                // ❗ A LINK is a leaf, whatever it points at. The listing reports a
                // link to a folder as `is_directory`, and descending on that alone
                // walks THROUGH it: the incoming files land in its target, a folder
                // the user never picked, and an Overwrite replaces files there. The
                // entry in hand answers it, so a real folder costs no probe.
                let dir_clashes_with_leaf = dest_hit.is_some_and(|d| !merges_as_a_directory(d));
                if !dir_clashes_with_leaf {
                    return Ok(Advance::Descend(
                        LevelAt {
                            source: child_source,
                            dest: child_dest,
                            folder: child_row,
                            path: path.clone(),
                        },
                        OnEnd {
                            dates: SourceFolderDates::of_entry(entry),
                            asks_if_refused: true,
                        },
                    ));
                }
            }

            // At this point the child is either a FILE, or a directory clashing with
            // a same-named dest LEAF (a file or a link: a type mismatch). If there's
            // a dest hit and we have merge context, route it through the file-policy
            // resolver.
            // From here the child is a leaf (or a renamed folder root); the resolver
            // still takes `std::path`, so it reads the destination's spelling.
            let child_dest_std = std_of(&child_dest).into_owned();
            let mut write_dest = child_dest.clone();
            let mut replaces = Replaces::Nothing;
            // Nothing has resolved a conflict for this child yet, so the name it is
            // about to take is one we believe FREE, and ❗ in a level this walk made,
            // free by proof (`LandingName::free`). A resolver decision below is what
            // turns that into a claim (`staged_write.rs::LandingName`).
            let mut landing = LandingName::free(level_made_here && !repeat);
            // Nothing has reserved anything for this child either, until a `Rename`
            // resolution below says otherwise.
            let mut reserved_placeholder = false;
            if let Some(hit) = dest_hit
                && let Some(ctx) = merge
            {
                // What a resolution can fail on is the clashing destination entry
                // (probing it, setting it aside), and the resolver names it: the
                // source's type and size come from the listing in hand.
                match pool
                    .drive(resolve_merge_child(
                        ctx,
                        source_volume,
                        &child_source,
                        entry,
                        descends,
                        dest_volume,
                        &child_dest,
                        hit,
                    ))
                    .await?
                {
                    MergeChildDecision::Skip => {
                        // A DEEP skip: record it so the caller knows this subtree did
                        // not extract in full (the move-out op must keep the source in
                        // the archive; deleting it would drop this un-landed child).
                        let skipped_bytes = entry.size.unwrap_or(0);
                        created.record_skip(child_source.clone(), skipped_bytes);
                        // ...and credit it to the bars. Without this a merge whose
                        // children all clash reports nothing at all until it ends.
                        progress.skip_leaf(skipped_bytes, child_row);
                        continue;
                    }
                    MergeChildDecision::Proceed {
                        write_path,
                        replace,
                        reserved_placeholder: reserved,
                    } => {
                        // The resolver's answer, in the volume's spelling: the
                        // clash's own path, or the free name a `Rename` picked.
                        write_dest = write_path;
                        replaces = replace;
                        reserved_placeholder = reserved;
                        // ❗ Ours to clear only when the pick put a placeholder
                        // there; one that reserved nothing (every remote backend),
                        // or a cross-type Overwrite that moved the destination
                        // aside, saw a FREE name, and keeps the free landing.
                        if reserved {
                            landing = LandingName::ClaimedByTheCaller;
                        }
                    }
                }
            }

            if let Some(path) = child_path {
                // Only a resolver decision sends a folder on from here. With no
                // merge context nobody freed the name, and descending would merge
                // into the leaf that holds it (THROUGH it, for a link): refuse.
                if dest_hit.is_some() && merge.is_none() {
                    return Err(VolumeError::AlreadyExists(child_dest_std.display().to_string()))
                        .at_destination(&*child_dest);
                }
                // Type-mismatch Overwrite/Rename that resolved to Proceed: the
                // resolver already set aside/relocated the dest leaf, so descend
                // into `write_dest` as a fresh (or renamed) directory root.
                // No refused-name question here: a same-named leaf already stands at
                // this name, so the destination stores it.
                return Ok(Advance::Descend(
                    LevelAt {
                        source: child_source,
                        dest: write_dest,
                        folder: child_row,
                        path,
                    },
                    OnEnd {
                        dates: SourceFolderDates::of_entry(entry),
                        asks_if_refused: false,
                    },
                ));
            }

            if let Some(target) = link_target {
                let staging = staging_for(&replaces, landing);
                let row = LeafRow {
                    source: child_source.clone(),
                    dest: write_dest.clone(),
                };
                pool.submit(
                    row,
                    copy_link_leaf(
                        source_volume,
                        child_source,
                        target,
                        entry.size,
                        dest_volume,
                        write_dest,
                        replaces,
                        reserved_placeholder,
                        staging,
                        state,
                        created,
                        progress,
                        child_row,
                    ),
                )
                .await?;
                pool.settle_refused(merge, created, progress).await?;
                continue;
            }

            // Past every Skip, so this child is one the move carries. Recording it
            // here rather than when its bytes land is safe: a leaf that fails fails
            // the whole source, and a failed source is never swept. The one leaf
            // that fails WITHOUT failing its source, a refused name the person
            // skipped, comes off again (`LeafPool::settle_refused`).
            created.record_carried_source(&child_source, entry);

            // PLAN MODE (one-pass sequential extract): the destination + conflict are
            // resolved; record the write and let the caller's single decode pass
            // stream the bytes. Don't stream, count, record, or emit progress here —
            // the data pass owns all of that. The directory structure and conflict
            // prompts still happened above, exactly as a streaming copy would.
            if let Some(plan) = plan {
                plan.record(
                    into_std(child_source),
                    super::sequential_extract::PlannedWrite {
                        dest_path: write_dest,
                        replaces,
                        landing,
                        row: child_row,
                    },
                );
                continue;
            }

            // Conflict resolution for this child is DONE, on the walker, in tree
            // order — the same rule the top-level concurrent driver follows. Only the
            // bytes go wide.
            let staging = staging_for(&replaces, landing);
            let row = LeafRow {
                source: child_source.clone(),
                dest: write_dest.clone(),
            };
            pool.submit(
                row,
                copy_leaf(
                    source_volume,
                    child_source,
                    // The walker listed this level, so the child's size is already
                    // in hand: SMB takes its one-frame read with it.
                    SourceFileFacts::from_entry(entry),
                    dest_volume,
                    write_dest,
                    replaces,
                    reserved_placeholder,
                    staging,
                    state,
                    created,
                    progress,
                    child_row,
                ),
            )
            .await?;
            // Ask about any name refused so far before walking on, so the prompt
            // comes while the person still sees the file it's about.
            pool.settle_refused(merge, created, progress).await?;
        }
        Ok(Advance::Finished)
    }

    /// Hands the outcome of the child folder at `child` to its level, which goes
    /// on with its next entry on `Ok`. `Err` ends that level too, with this
    /// failure.
    async fn settle(
        &self,
        pool: &mut LeafPool<'a>,
        child: LevelAt,
        then: OnEnd,
        outcome: Result<DirectoryCreation, PathedVolumeError>,
    ) -> Result<(), PathedVolumeError> {
        match outcome {
            Ok(made) => {
                if made == DirectoryCreation::Created {
                    pool.folders.note_filled(child.dest, into_std(child.source), then.dates);
                }
                Ok(())
            }
            Err(e) if then.asks_if_refused => {
                let folder = RefusedFolder {
                    source: &child.source,
                    dest: &child.dest,
                    node: child.folder,
                };
                skip_if_folder_refused(
                    pool,
                    self.merge,
                    self.created,
                    self.progress,
                    self.source_volume,
                    &folder,
                    e,
                )
                .await
            }
            Err(e) => Err(e),
        }
    }
}

impl Level {
    /// Every entry is walked: the level is in place, which is all an empty
    /// folder's row waits for. Answers whether THIS walk created it
    /// (`AlreadyExisted` when in doubt).
    fn finish(&self, at: &LevelAt, progress: &Arc<SourceProgress>) -> DirectoryCreation {
        // A level with a FILE's row is a link this walk descended through: the
        // scan counted it as one entry, which has now landed.
        match self.tree.folder() {
            Some(row) => progress.tree().folder_in_place(Some(row)),
            None => progress.tree().complete_whole(at.folder),
        }
        if self.made_here {
            DirectoryCreation::Created
        } else {
            DirectoryCreation::AlreadyExisted
        }
    }
}

/// Whether this backend's `create_directory` reliably returns
/// `VolumeError::AlreadyExists` when a same-name directory already exists.
///
/// `true` for LocalPosix (`std::fs::create_dir` → `ErrorKind::AlreadyExists`),
/// SMB (smb2 typed STATUS_OBJECT_NAME_COLLISION), and InMemoryVolume's
/// merge-test variant. `false` for MTP: the protocol allows same-name sibling
/// objects and `create_folder` happily makes a duplicate, so the merge walker
/// must pre-check existence instead of trusting the create to error.
fn backend_create_directory_detects_collisions(volume: &Arc<dyn Volume>) -> bool {
    volume.create_directory_errors_on_existing_dir()
}

/// Outcome of resolving one clashing child inside a merge.
enum MergeChildDecision {
    /// Honor a Skip: do NOT touch the dest child at all.
    Skip,
    /// Proceed writing to `write_path`; `replace` says what that does to a file
    /// at the name (`Swap`: the write's landing replaces the original there).
    /// `reserved_placeholder` says the resolver put a zero-byte `O_EXCL` file at
    /// `write_path` to hold the name, which the leaf owes taking back if its
    /// write never happens (`naming.rs::ClaimedName`).
    Proceed {
        write_path: AppPathBuf,
        replace: Replaces,
        reserved_placeholder: bool,
    },
}

/// Dispatches one clashing merge child through the volume conflict resolver,
/// handing it the op-wide apply-to-all latch so a "…all" choice from any level
/// (top or deep) applies here. The `conflict_dispatch_lock` inside the resolver
/// — not this cell — is what serializes the human across concurrent merges.
#[allow(
    clippy::too_many_arguments,
    reason = "Both sides of one clash, plus what this walk makes of the child (a link it carries is no folder)."
)]
async fn resolve_merge_child(
    ctx: &MergeCtx<'_>,
    source_volume: &Arc<dyn Volume>,
    child_source: &AppPath,
    entry: &FileEntry,
    // Whether the child is a folder to THIS walk: a link it carries isn't.
    source_is_directory: bool,
    dest_volume: &Arc<dyn Volume>,
    child_dest: &AppPath,
    dest_hit: &FileEntry,
) -> Result<MergeChildDecision, PathedVolumeError> {
    // Deep children aren't top-level sources, so no preflight hint exists for
    // them; the resolver falls back to trait calls. We DO know both sides' type
    // and size from the listing entries already in hand — the source's from this
    // level's source listing, the destination's from the `dest_by_name` map the
    // caller built for the same level. That saves the resolver a redundant
    // `is_directory` probe and seeds the dialog's size annotations.
    //
    // ❗ The dest size matters beyond display: it's what `OverwriteSmaller`
    // compares against. Passing `None` here used to leave the resolver
    // fabricating a `0`, which made every destination look smaller.
    let source_is_directory_hint = Some(source_is_directory);
    let source_size_hint = if source_is_directory { None } else { entry.size };
    let dest_size_hint = if dest_hit.is_directory { None } else { dest_hit.size };
    let _ = ctx.source_hints; // hints are keyed by top-level source path; deep children never match

    let resolved = resolve_volume_conflict(
        source_volume,
        child_source,
        dest_volume,
        child_dest,
        ctx.config,
        ctx.events,
        ctx.operation_id,
        ctx.state,
        ctx.apply_to_all,
        source_size_hint,
        dest_size_hint,
        source_is_directory_hint,
    )
    .await;

    // A failure goes out whole, as the resolver pathed it: a cancel stays
    // `VolumeError::Cancelled` for the post-loop, and a give-up keeps what its
    // outage wait learned (`ServerConnectionLost`).
    match resolved? {
        None => Ok(MergeChildDecision::Skip),
        Some(ResolvedConflict {
            write_path,
            replaces,
            reserved_placeholder,
            displaced,
        }) => {
            // A cross-type Overwrite's aside is the operation's to settle, once
            // it knows how it ended.
            if let Some(displaced) = displaced {
                ctx.displaced.hold(displaced);
            }
            Ok(MergeChildDecision::Proceed {
                write_path,
                replace: replaces,
                reserved_placeholder,
            })
        }
    }
}
