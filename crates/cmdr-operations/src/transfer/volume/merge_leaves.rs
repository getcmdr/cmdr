//! The merge walk's leaves: the [`LeafPool`] each top-level source's walker
//! holds, the one-file copies it runs inside the operation's `FileWindow`
//! ([`copy_leaf`], [`copy_link_leaf`]), and the refused-name half (a name the
//! destination won't take, asked about on the walker, ❌ never in a leaf).
//!
//! The walk itself is `merge.rs`: it discovers serially, in tree order, and
//! hands each file's byte copy here. `DETAILS.md` § "One window for the whole
//! operation".

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;

use super::super::super::state::WriteOperationState;
use super::super::super::transfer_tree::NodeId;
use super::super::transfer_driver::SourceProgress;
use super::super::transfer_probe::{CURRENT_TASK_PROBE, TaskProbeHandle, TaskRole};
use super::folder_metadata::FolderMetadata;
use super::listed_name::std_of;
use super::merge_ctx::{CreatedPaths, FileWindow, MergeCtx, MergeProbe};
use super::naming::take_back_reservation;
use super::preflight::SourceFileFacts;
use super::refused_item::{RefusalSubject, has_no_content, refused_here, skip_if_refused};
use super::rename_merge::merges_as_a_directory;
use super::strategy::{Replaces, WriteStaging};
use super::stream_pipe::{land_link, stream_pipe_file};
use super::transfer_error::{PathRole, PathedVolumeError};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{Volume, VolumeError};

/// What one leaf file's copy reports back to the walker.
pub(super) type LeafResult = Result<LeafEnd, PathedVolumeError>;

/// How a leaf that didn't fail the walk ended.
pub(super) enum LeafEnd {
    /// The file landed, with this many bytes.
    Landed(u64),
    /// The destination refused the file's name. Nothing of it is there (its
    /// temp went with the failed landing), and the WALKER asks the person
    /// about it (`LeafPool::settle_refused`): a leaf may never wait on one.
    Refused(Box<RefusedLeaf>),
}

/// A leaf whose name the destination refused, waiting for its walker to ask.
pub(super) struct RefusedLeaf {
    source: AppPathBuf,
    dest: AppPathBuf,
    size: Option<u64>,
    node: Option<NodeId>,
    error: PathedVolumeError,
}

/// What a leaf is called in the operation's in-flight table. Built even when no
/// probe is registered (the tests), which costs two path clones per file
/// against a network round trip.
pub(super) struct LeafRow {
    pub(super) source: AppPathBuf,
    pub(super) dest: AppPathBuf,
}

/// The leaves this operation currently has in flight, plus the running totals
/// and the created folders the walker reads once the tree is walked.
///
/// The `FuturesUnordered` is LOCAL to one top-level source's walk (it lives on
/// that walker's task, so nothing here needs `'static` or a spawn), while the
/// [`FileWindow`] it reserves from is shared by the whole operation. That split
/// is the point: a walker can hold as many leaves as it likes in its own set,
/// but it cannot start one without a slot from the single op-wide window.
///
/// ❗ The leaves are futures ONLY this walker polls, so a leaf moves only while
/// the walker is awaiting the pool (`reserve`'s drain arm, `drain`). ❌ A leaf
/// must never wait on anything its walker can hold or be waiting on (the
/// conflict-dispatch mutex, a person's answer, a lock the walker takes): the
/// walker would wait on itself and the copy would hang past Cancel. That's the
/// shape of ERR-8MEX6, which the concurrent driver escaped by spawning its
/// tasks (`copy_concurrent.rs`). Today a leaf only streams and lands one file
/// (`copy_leaf`), and the dispatch mutex never spans a write. A leaf that has
/// to wait on the walker's side needs spawned leaves first.
pub(super) struct LeafPool<'a> {
    window: FileWindow,
    /// The in-flight table and the row of the source this walk descends from,
    /// which is what every leaf row is numbered under.
    probe: Option<MergeProbe>,
    /// Which leaf of this source the next row is. Only ever climbs, so a number
    /// is never reused inside one dump.
    next_leaf: usize,
    in_flight: FuturesUnordered<Pin<Box<dyn Future<Output = LeafResult> + Send + 'a>>>,
    pub(super) bytes: u64,
    /// The FIRST leaf failure. Later ones are dropped: the walker reports the
    /// file that actually broke, and a second error piled on top of it says
    /// nothing the user can act on (same rule `cleanup.rs::remove_tree` follows).
    pub(super) first_error: Option<PathedVolumeError>,
    /// Leaves whose names the destination refused, in the order they came
    /// back, for the walker to ask about.
    refused: std::collections::VecDeque<RefusedLeaf>,
    /// Every folder below the root this walk created, noted as the walk leaves
    /// it, so the subtree can date them once its leaves have drained.
    pub(super) folders: FolderMetadata,
}

impl<'a> LeafPool<'a> {
    pub(super) fn new(window: FileWindow, probe: Option<MergeProbe>) -> Self {
        Self {
            window,
            probe,
            next_leaf: 0,
            in_flight: FuturesUnordered::new(),
            bytes: 0,
            first_error: None,
            refused: std::collections::VecDeque::new(),
            folders: FolderMetadata::default(),
        }
    }

    fn fold(&mut self, result: LeafResult) {
        match result {
            Ok(LeafEnd::Landed(bytes)) => self.bytes += bytes,
            Ok(LeafEnd::Refused(refused)) => self.refused.push_back(*refused),
            Err(e) => {
                if self.first_error.is_none() {
                    self.first_error = Some(e);
                }
            }
        }
    }

    /// Reserves an op-wide slot, keeping THIS walker's own leaves moving while it
    /// waits. Without the drain arm a full window would park the walker on a
    /// permit its own in-flight leaves are the only ones able to release.
    async fn reserve(&mut self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        enum Step {
            Landed(LeafResult),
            Reserved(Option<tokio::sync::OwnedSemaphorePermit>),
        }
        let window = self.window.clone();
        loop {
            if self.in_flight.is_empty() {
                return window.reserve().await;
            }
            let step = {
                let in_flight = &mut self.in_flight;
                tokio::select! {
                    // Prefer landing finished work: it frees a slot, records the
                    // file, and keeps the progress bar moving.
                    biased;
                    Some(landed) = in_flight.next() => Step::Landed(landed),
                    permit = window.reserve() => Step::Reserved(permit),
                }
            };
            match step {
                Step::Landed(landed) => self.fold(landed),
                Step::Reserved(permit) => return permit,
            }
        }
    }

    /// Runs one of the walker's own awaits while THIS walker's leaves keep
    /// moving.
    ///
    /// The leaves are futures only this walker polls, so an await of the
    /// walker's that leaves them unpolled can wait on something a leaf holds.
    /// The case that bit: a leaf takes the share's single-flight reconnect
    /// (SMB's `reconnect_lock`) and must be polled again to finish the dial,
    /// while the walker waits out the same outage on a clash's probe and queues
    /// behind that lock, so neither moves until the outage budget runs out. ❌
    /// Every walker await goes through here or [`Self::reserve`]; recursing
    /// into a level passes the pool down, and that level's awaits drive it.
    pub(super) async fn drive<T>(&mut self, work: impl Future<Output = T>) -> T {
        let mut work = std::pin::pin!(work);
        loop {
            if self.in_flight.is_empty() {
                return work.await;
            }
            let landed = {
                let in_flight = &mut self.in_flight;
                tokio::select! {
                    biased;
                    out = &mut work => return out,
                    Some(landed) = in_flight.next() => landed,
                }
            };
            self.fold(landed);
        }
    }

    /// Runs one leaf's byte copy inside the window.
    ///
    /// A SERIAL window awaits it inline, so an MTP transfer sees exactly the
    /// sequence it always did — one operation at a time on the bulk transport,
    /// directory creates included. A concurrent window parks it in this walker's
    /// set with a permit and an in-flight-table row of its own.
    ///
    /// `Err` means the walk must stop: the pool already holds every leaf that
    /// landed, and this is the first failure.
    pub(super) async fn submit(
        &mut self,
        row: LeafRow,
        leaf: impl Future<Output = LeafResult> + Send + 'a,
    ) -> Result<(), PathedVolumeError> {
        if let Some(e) = self.first_error.take() {
            return Err(e);
        }
        if self.window.is_serial() {
            let landed = leaf.await;
            self.fold(landed);
            return match self.first_error.take() {
                Some(e) => Err(e),
                None => Ok(()),
            };
        }
        let permit = self.reserve().await;
        if let Some(e) = self.first_error.take() {
            return Err(e);
        }
        // One row per in-flight WRITE, which is the invariant every `TaskProbe`
        // field is built on: `arm_stall_abort` replaces the row's token per
        // attempt and `set_bytes` stores (never adds) the attempt's count, so two
        // leaves sharing a row would clobber each other's stall-abort signal and
        // keep resetting the watchdog's stillness clock. Dropping the handle with
        // the future takes the row away again.
        let table_row = self.probe.as_ref().map(|probe| {
            let handle = probe.operation.begin_task(
                // Under this source's own row, so the dump says which top-level
                // source is producing the leaf and no two rows collide.
                probe.source_row.leaf(self.next_leaf),
                // Every row this walker opens is one leaf FILE's byte copy,
                // holding the permit reserved just above.
                TaskRole::File,
                &std_of(&row.source),
                &std_of(&row.dest),
            );
            self.next_leaf += 1;
            handle
        });
        self.in_flight.push(Box::pin(async move {
            // Both ride the leaf's whole life: the permit returns the op-wide
            // slot and the handle clears the in-flight row, on success, failure,
            // or a drop mid-write.
            let _permit = permit;
            let table_row = table_row;
            match table_row.as_ref().map(TaskProbeHandle::probe) {
                Some(probe) => CURRENT_TASK_PROBE.scope(probe, leaf).await,
                None => leaf.await,
            }
        }));
        Ok(())
    }

    /// Waits out every leaf still in flight and folds it in.
    ///
    /// ❌ Never skipped, on any exit path: a leaf still running when the walk
    /// returns would keep writing to the destination after the driver has moved
    /// on to cleanup, and its `created` record would land too late for rollback
    /// to see it. Under cancel the leaves wind down on their own next chunk
    /// (`stream_pipe_file` checks the intent per chunk), and the driver's
    /// cancel-drain deadline is the backstop for one that doesn't.
    pub(super) async fn drain(&mut self) {
        while let Some(landed) = self.in_flight.next().await {
            self.fold(landed);
        }
    }

    /// Asks the person about every leaf whose name the destination refused,
    /// on the WALKER, with this walker's other leaves still moving (`drive`).
    ///
    /// A Skip records the leaf exactly like a conflict Skip (`record_skip`,
    /// which a move's source sweep spares, and out of the carried-source
    /// ledger too) and credits the bars. Anything else is the walk's failure.
    /// With no merge context there's nobody to ask through, so the refusal
    /// stands, as it did before the question existed.
    pub(super) async fn settle_refused(
        &mut self,
        merge: Option<&MergeCtx<'_>>,
        created: &CreatedPaths,
        progress: &Arc<SourceProgress>,
    ) -> Result<(), PathedVolumeError> {
        while let Some(refused) = self.refused.pop_front() {
            let Some(ctx) = merge else {
                return Err(refused.error);
            };
            let (source, dest) = (std_of(&refused.source), std_of(&refused.dest));
            let subject = RefusalSubject {
                source: &source,
                dest: &dest,
                source_size: refused.size,
                source_is_directory: false,
            };
            self.drive(skip_if_refused(ctx, &subject, refused.error.clone()))
                .await?;
            let bytes = refused.size.unwrap_or(0);
            created.forget_carried_source(&refused.source);
            created.record_skip(refused.source, bytes);
            progress.skip_leaf(bytes, refused.node);
        }
        Ok(())
    }
}

/// Streams ONE file to the destination and records it, which is everything the
/// walker used to do inline per file child. Split out so it can be handed to the
/// window as a unit of work.
#[allow(
    clippy::too_many_arguments,
    reason = "One leaf's whole context: both volumes, both paths, what the listing knows about the source, what the write replaces, how the write is staged, shared state, the ledger, the source's progress accounting, and the file's tree row."
)]
pub(super) async fn copy_leaf<'a>(
    source_volume: &'a Arc<dyn Volume>,
    child_source: AppPathBuf,
    source_facts: SourceFileFacts,
    dest_volume: &'a Arc<dyn Volume>,
    write_dest: AppPathBuf,
    replaces: Replaces,
    reserved_placeholder: bool,
    staging: WriteStaging,
    state: &'a Arc<WriteOperationState>,
    created: &'a CreatedPaths,
    progress: &'a Arc<SourceProgress>,
    node: Option<NodeId>,
) -> LeafResult {
    // This leaf's own share of the operation's in-flight byte total, held for
    // its whole life. Siblings streaming beside it hold their own, so neither
    // this one finishing nor a sibling's can take the other's bytes off the bar.
    // A leaf that never lands withdraws its share when this handle drops. It is
    // also the file's tree row, active from here.
    let leaf = progress.begin_leaf(node);
    let on_chunk = |file_bytes_done: u64, _file_bytes_total: u64| leaf.on_chunk(file_bytes_done);
    let source_size = source_facts.size;
    // ❗ This is the deepest frame that knows WHICH file failed, and the error
    // leaves it carrying that file's path on the side that refused. Report it
    // one level up and the user gets the name of the folder they selected
    // instead of the file that broke.
    let streamed = stream_pipe_file(
        source_volume,
        &child_source,
        source_facts,
        dest_volume,
        &write_dest,
        state,
        &on_chunk,
        staging,
    )
    .await;
    // This child gave up (a read failure, a cancel between chunks), so the name
    // it reserved goes back now, rather than waiting for the post-loop sweep of
    // unfilled reservations (`naming.rs::take_back_unfilled_reservations`), which
    // is there for the leaf whose future is dropped before it gets here.
    if streamed.is_err() && reserved_placeholder {
        take_back_reservation(dest_volume, &std_of(&write_dest), &state.claimed_names).await;
    }
    let piped = match streamed {
        Ok(piped) => piped,
        Err(error) => {
            return leaf_stopped_short(error, child_source, write_dest, source_size, created, progress, node);
        }
    };
    let bytes = piped.bytes;
    // The file landed at `write_dest`, the original's own name for an
    // overwrite (a swap's landing ran inside the write). A landed overwrite's
    // row says so; a swap that failed after its delete has already told the
    // journal (`staged_write.rs::land`), and one that failed before it left the
    // original in place, which is no overwrite at all.
    if replaces.overwrites() {
        created.record_overwrite();
    }
    created.record_file(write_dest, bytes, piped.landed);
    leaf.complete(bytes);
    Ok(LeafEnd::Landed(bytes))
}

/// What a leaf's write stopping short of a landing comes to.
///
/// A child with no bytes and no way to land as itself (a link the source can't
/// recreate, a fifo) is one skipped item, ❌ never the end of its folder; a move's
/// sweep spares it. A NAME the destination refused is one item the person decides
/// about, handed back for the walker to ask, since a leaf must never wait on a
/// person (`LeafPool`). Anything else is the walk's failure.
fn leaf_stopped_short(
    error: PathedVolumeError,
    source: AppPathBuf,
    dest: AppPathBuf,
    size: Option<u64>,
    created: &CreatedPaths,
    progress: &Arc<SourceProgress>,
    node: Option<NodeId>,
) -> LeafResult {
    if has_no_content(&error) {
        let bytes = size.unwrap_or(0);
        created.record_skip(source, bytes);
        progress.skip_leaf(bytes, node);
        return Ok(LeafEnd::Landed(0));
    }
    let (source_std, dest_std) = (std_of(&source), std_of(&dest));
    let subject = RefusalSubject {
        source: &source_std,
        dest: &dest_std,
        source_size: size,
        source_is_directory: false,
    };
    if refused_here(&subject, &error).is_some() {
        drop((source_std, dest_std));
        return Ok(LeafEnd::Refused(Box::new(RefusedLeaf {
            error,
            source,
            dest,
            size,
            node,
        })));
    }
    Err(error)
}

/// Lands ONE symlink the copy carries and records it: [`copy_leaf`] for a child
/// with a target in place of bytes. Credits the bars what the scan counted for
/// the entry, since that's what they total.
#[allow(
    clippy::too_many_arguments,
    reason = "One leaf's whole context, the same set `copy_leaf` carries, with the target in place of the listing's facts."
)]
pub(super) async fn copy_link_leaf<'a>(
    source_volume: &'a Arc<dyn Volume>,
    child_source: AppPathBuf,
    target: PathBuf,
    listed_size: Option<u64>,
    dest_volume: &'a Arc<dyn Volume>,
    write_dest: AppPathBuf,
    replaces: Replaces,
    reserved_placeholder: bool,
    staging: WriteStaging,
    state: &'a Arc<WriteOperationState>,
    created: &'a CreatedPaths,
    progress: &'a Arc<SourceProgress>,
    node: Option<NodeId>,
) -> LeafResult {
    let leaf = progress.begin_leaf(node);
    let landed = land_link(
        source_volume,
        &child_source,
        &target,
        dest_volume,
        &write_dest,
        state,
        staging,
    )
    .await;
    if landed.is_err() && reserved_placeholder {
        take_back_reservation(dest_volume, &std_of(&write_dest), &state.claimed_names).await;
    }
    if let Err(error) = landed {
        return leaf_stopped_short(error, child_source, write_dest, listed_size, created, progress, node);
    }
    if replaces.overwrites() {
        created.record_overwrite();
    }
    created.record_link(write_dest, target);
    let bytes = listed_size.unwrap_or(0);
    leaf.complete(bytes);
    Ok(LeafEnd::Landed(bytes))
}

/// A listed child whose name the destination refuses, before anything is written for it.
pub(super) struct RefusedChild<'e> {
    pub(super) entry: &'e FileEntry,
    pub(super) source: AppPathBuf,
    pub(super) dest: AppPathBuf,
    pub(super) node: Option<NodeId>,
}

/// Refuses `child` the way a landing's refusal is: a file waits for its walker to ask
/// (`LeafPool::settle_refused`), a folder is asked about now and skipped whole on a Skip.
pub(super) async fn refuse_child(
    pool: &mut LeafPool<'_>,
    merge: Option<&MergeCtx<'_>>,
    created: &CreatedPaths,
    progress: &Arc<SourceProgress>,
    source_volume: &Arc<dyn Volume>,
    child: RefusedChild<'_>,
) -> Result<(), PathedVolumeError> {
    let error = PathedVolumeError::new(
        PathRole::Destination,
        &child.dest,
        VolumeError::InvalidName("The destination's filesystem doesn't take this name".into()),
    );
    if merges_as_a_directory(child.entry) {
        let folder = RefusedFolder {
            source: &child.source,
            dest: &child.dest,
            node: child.node,
        };
        skip_if_folder_refused(pool, merge, created, progress, source_volume, &folder, error).await
    } else {
        pool.refused.push_back(RefusedLeaf {
            source: child.source,
            dest: child.dest,
            size: child.entry.size,
            node: child.node,
            error,
        });
        Ok(())
    }
}

/// A child folder of the walk, as its refused-name question names it.
pub(super) struct RefusedFolder<'p> {
    pub(super) source: &'p AppPath,
    pub(super) dest: &'p AppPath,
    pub(super) node: Option<NodeId>,
}

/// A child folder's level ended in `error`. When that is the destination
/// refusing the FOLDER's own name (its `create_directory`, before anything
/// under it was made), the walker asks, as for a refused file; a Skip leaves
/// the whole subtree out and the walk carries on with its siblings.
///
/// The skip counts every file under it (one `scan_for_copy` of the source
/// subtree, paid only on a skip) and records the folder as a skipped source,
/// which a move's sweep spares whole: nothing of it was carried. Anything else,
/// a refusal deeper down included (its own level asked about it), stands.
pub(super) async fn skip_if_folder_refused(
    pool: &mut LeafPool<'_>,
    merge: Option<&MergeCtx<'_>>,
    created: &CreatedPaths,
    progress: &Arc<SourceProgress>,
    source_volume: &Arc<dyn Volume>,
    folder: &RefusedFolder<'_>,
    error: PathedVolumeError,
) -> Result<(), PathedVolumeError> {
    let Some(ctx) = merge else {
        return Err(error);
    };
    let (source_std, dest_std) = (std_of(folder.source), std_of(folder.dest));
    let subject = RefusalSubject {
        source: &source_std,
        dest: &dest_std,
        source_size: None,
        source_is_directory: true,
    };
    if refused_here(&subject, &error).is_none() {
        return Err(error);
    }
    pool.drive(skip_if_refused(ctx, &subject, error)).await?;
    let (files, bytes) = match pool.drive(source_volume.scan_for_copy(folder.source)).await {
        Ok(scan) => (scan.file_count, scan.total_bytes),
        // The skip stands either way; only its count is short.
        Err(e) => {
            log::warn!(
                "merge: couldn't count the skipped folder {}: {}",
                LogPrivate::path(&*source_std),
                LogError(&e)
            );
            (0, 0)
        }
    };
    created.record_skipped_subtree(folder.source.to_owned(), files, bytes);
    progress.skip_leaves(files, bytes, folder.node);
    Ok(())
}
