//! The approval bridge: from a claimed group to a running operation.
//!
//! Approval is one hand-off, and this module is all of it. It claims the group through the
//! store transaction, turns the stored row plus its live ops into the executor call the verb
//! describes, and injects a sink wrapped so the ops report their own outcomes back
//! (`decorator.rs`).
//!
//! ## What this deliberately does not do
//!
//! It builds an ORDINARY executor call. No conflict policy of its own, no auto-skip, no
//! refusal to create a destination folder or to overwrite: once the user approves, it is
//! exactly as if they started the action, because they did. Everything this module adds is
//! bookkeeping about a proposal, none of it reaches the filesystem.
//!
//! The write engine, correspondingly, knows nothing about any of this. It reports per-source
//! outcomes through the sink every operation already emits through, and, as a
//! crate with no `cmdr` dependency, it can't name `agent::`.

mod decorator;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use cmdr_fs::app_path::{AppPath, AppPathBuf};

use rusqlite::Connection;

use decorator::ProposalReportingSink;

use super::super::memory::MemoryStore;
use super::super::store::AgentStoreError;
use super::super::store::proposals::{
    AcceptanceOutcome, ClaimOutcome, ClaimRefusal, ClaimedGroup, OpSelection, ProposalGroup, ProposalOp, get_group,
    page_ops, record_acceptance,
};
use super::super::types::{OpStatus, ProposalVerb};
use super::facts::{RunsAs, runs_as};
use crate::file_system::volume::Volume;
use crate::operations_host::{operations, refuse_not_installed};
use cmdr_operation_log::types::Initiator;
use cmdr_operations::{
    BulkRenameRow, ExpectedSources, OperationEventSink, Operations, RenameStartError, RenamesStarted,
    SourceFingerprint, SourceUnread, TransferLeaves, VolumeCopyConfig, WriteOperationConfig, WriteOperationError,
    WriteOperationStartResult,
};

/// A group whose operation is running, and the operation it became.
#[derive(Debug, Clone)]
pub struct ApprovedGroup {
    pub group_id: i64,
    pub operation: WriteOperationStartResult,
}

/// Why an approval did not become a running operation.
#[derive(Debug, Clone)]
pub enum ApprovalRefusal {
    /// Preflight would not accept: the group had already left `pending`, is unknown, or is a
    /// server group, which this path never approves (`AcceptanceOutcome::NotAFileGroup`).
    NotAccepted(AcceptanceOutcome),
    /// The claim transaction refused. Its two variants mean different recoveries.
    Claim(ClaimRefusal),
    /// No volume is registered for the group's sources: a phone or server nobody has connected
    /// (`SourceNotConnected`), or a drive that left (`SourceNoLongerConnected`). Classified by
    /// `crate::unregistered_volumes`, exactly as a clicked copy off it would be. Nothing was
    /// claimed.
    SourceUnavailable(WriteOperationError),
    /// The stored row lacks the target its verb binds. Unreachable through `GroupIntent`,
    /// which pairs each verb with its target at construction; reported rather than panicked
    /// because the row is read back from SQLite, where a hand-edit could produce it.
    TargetMissing { verb: ProposalVerb },
    /// The group claimed cleanly and the write engine refused to start.
    Engine(WriteOperationError),
    /// The group claimed cleanly and the rename batch refused to start.
    Rename(RenameStartError),
    /// The group claimed cleanly, but preflight could fingerprint none of its files, so the batch
    /// had no row to run: the rename's `NothingToRename`, with the typed reason the stats gave.
    NothingToRun(NothingLeft),
}

/// Why preflight left nothing to run, from the stats themselves (`SourceUnread`), ❌ never a
/// message: "these files aren't there" and "Cmdr couldn't read them" are different answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NothingLeft {
    /// Every file is gone from its path: renamed, moved, or deleted since the suggestion.
    Gone,
    /// At least one file didn't answer its stat, so "gone" would be a guess.
    Unreadable,
}

/// What approving a group did.
#[derive(Debug, Clone)]
pub enum ApprovalOutcome {
    Started(ApprovedGroup),
    Refused(ApprovalRefusal),
}

/// How many op rows one read pulls back. A group of 60 000 is legitimate, and the executor
/// needs every path, so this is the one place the spine does materialize them: paging keeps
/// the peak at a page rather than the whole group plus the whole group again.
const OP_PAGE: u32 = 1_000;

/// Approve a group and start the operation it describes.
///
/// The whole hand-off, in order: preflight records what the user accepted, the claim
/// transaction binds against it and moves the group to `approved`, the live ops become an
/// ordinary executor call, and the sink goes in wrapped so each source reports its outcome
/// back into the spine.
///
/// **Two connections, deliberately.** The claim and the reads run on the caller connection;
/// `reporting_conn` is MOVED into the decorator because the operation outlives this call by
/// minutes or hours, so its writer cannot borrow anything the caller owns.
///
/// ❌ Nothing here adds an execution behaviour. The config is the default one a user-started
/// operation gets, the destination is created if missing exactly as it would be, and a
/// conflict is answered exactly as it would be. Approval transfers responsibility; it does
/// not change what runs.
#[allow(
    clippy::too_many_arguments,
    reason = "the approval hand-off; every one of these outlives the caller and has to be moved in"
)]
pub async fn approve_and_execute(
    conn: &Connection,
    reporting_conn: Connection,
    events: Arc<dyn OperationEventSink>,
    group_id: i64,
    shown_revision: i64,
    selection: &OpSelection,
    shown_runs_as: Option<RunsAs>,
    now: i64,
    memory: Option<MemoryStore>,
) -> Result<ApprovalOutcome, AgentStoreError> {
    match record_acceptance(conn, group_id, shown_revision, selection, now)? {
        AcceptanceOutcome::Accepted { .. } => {}
        other => return Ok(ApprovalOutcome::Refused(ApprovalRefusal::NotAccepted(other))),
    }

    // Read the accepted set and fingerprint it BEFORE the claim, so the binding describes the
    // files as they were while the user was deciding. The claim that follows is the last gate
    // before the engine.
    let ops = live_ops(conn, group_id)?;
    let op_ids: HashMap<AppPathBuf, i64> = ops
        .iter()
        .filter_map(|op| Some((decorator::op_key(&op.source_path)?, op.id)))
        .collect();
    // The starts take app paths; the binding (`ExpectedSources`) keys the engine's own
    // `std::path` spelling of each ([`stored_spellings`]).
    let typed_sources = match crate::commands::wire_path::start_paths(ops.iter().map(|op| op.source_path.as_str())) {
        Ok(typed) => typed,
        Err(refused) => {
            return Ok(ApprovalOutcome::Refused(ApprovalRefusal::SourceUnavailable(
                refused.into(),
            )));
        }
    };
    let sources = stored_spellings(&typed_sources);

    let volume_id = source_volume_of(conn, group_id)?;
    // Before `install` no source volume resolves, and the start refusal says why.
    let engine = match operations() {
        Ok(engine) => engine,
        Err(not_installed) => {
            let refusal = refuse_not_installed(not_installed);
            return Ok(ApprovalOutcome::Refused(ApprovalRefusal::SourceUnavailable(refusal)));
        }
    };
    let Some((source_volume, _)) = engine
        .resolve_source_volume(&volume_id, typed_sources.first().map(|path| &**path))
        .await
    else {
        let first = sources
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let refusal = engine.unregistered_source_error(&volume_id, &first).await;
        return Ok(ApprovalOutcome::Refused(ApprovalRefusal::SourceUnavailable(refusal)));
    };
    let preflight = capture_expected_sources(source_volume.as_ref(), &sources).await;
    let unread = preflight.nothing_left();

    // Through the service layer, which owns both the claim's announcement and the approval
    // metric. The metric waits for the start: a refused start gives the group back.
    let claimed = match super::approve(conn, group_id, now)? {
        ClaimOutcome::Claimed(claimed) => claimed,
        ClaimOutcome::Refused(refusal) => return Ok(ApprovalOutcome::Refused(ApprovalRefusal::Claim(refusal))),
    };

    let sink: Arc<dyn OperationEventSink> = Arc::new(ProposalReportingSink::new(
        events,
        group_id,
        op_ids,
        reporting_conn,
        memory,
    ));

    // The fallback runs only when the review showed it AND the volume still calls for it: a
    // drive that gained a Trash since takes the trash the verb asked for, and one that lost it
    // under a review that offered a trash refuses each item, as a clicked trash there would.
    let runs = approved_runs_as(shown_runs_as, runs_as(claimed.group.verb, Some(source_volume.as_ref())));
    let bound = BoundOps {
        ops: &ops,
        sources: &sources,
        expected: preflight.expected,
    };
    match start_for(engine, &claimed.group, runs, typed_sources, bound, sink).await {
        Ok(operation) => {
            super::started(&claimed);
            // The thread says "running" now, and the decorator's settle rewrites that line.
            crate::agent::outcomes::record_running(conn, group_id, now);
            Ok(ApprovalOutcome::Started(ApprovedGroup { group_id, operation }))
        }
        Err(refusal) => {
            // A batch with no row left says WHY none was: every file gone reads differently
            // from files that didn't answer (#1524).
            let refusal = match (refusal, unread) {
                (ApprovalRefusal::Rename(RenameStartError::NothingToRename), Some(why)) => {
                    ApprovalRefusal::NothingToRun(why)
                }
                (refusal, _) => refusal,
            };
            give_back_after(conn, group_id, &refusal);
            Ok(ApprovalOutcome::Refused(refusal))
        }
    }
}

/// Start the rows a rename review approved, for a group its caller already claimed.
///
/// The rename review (`commands/agent/bulk_rename.rs`) builds its own rows from the plan the
/// user saw, so it can't go through [`approve_and_execute`]. Everything after the claim is the
/// same hand-off, though, and lives here so the two approval paths can't drift: the reporting
/// decorator (without it the group never settles, so it reads `interrupted` at the next launch
/// and its thread never says what ran), the start count, the "running" thread line, or giving
/// the group back when the batch refuses to start.
///
/// **Takes its connections by value**, so the caller's future stays `Send` across the await.
#[allow(
    clippy::too_many_arguments,
    reason = "the approval hand-off; every one of these outlives the caller and has to be moved in"
)]
pub(crate) async fn start_reviewed_renames(
    conn: Connection,
    reporting_conn: Connection,
    events: Arc<dyn OperationEventSink>,
    claimed: ClaimedGroup,
    volume_id: String,
    rows: Vec<BulkRenameRow>,
    initiator: Initiator,
    memory: Option<MemoryStore>,
    now: i64,
) -> Result<RenamesStarted, RenameStartError> {
    let group_id = claimed.group.id;
    // Each row's id IS its op id: the review's rows come straight off the group's ops.
    let op_ids: HashMap<AppPathBuf, i64> = rows
        .iter()
        .filter_map(|row| Some((decorator::op_key(row.source.to_str()?)?, row.row_id.parse().ok()?)))
        .collect();
    let sink: Arc<dyn OperationEventSink> = Arc::new(ProposalReportingSink::new(
        events,
        group_id,
        op_ids,
        reporting_conn,
        memory,
    ));
    let started = match operations() {
        Ok(engine) => engine.start_renames(sink, volume_id, rows, initiator).await,
        Err(not_installed) => Err(RenameStartError::Engine {
            error: refuse_not_installed(not_installed),
        }),
    };
    match started {
        Ok(started) => {
            super::started(&claimed);
            crate::agent::outcomes::record_running(&conn, group_id, now);
            Ok(started)
        }
        Err(reason) => {
            // Nothing ran, so the claim goes back: the review re-reads the plan, and a group
            // left `approved` would read as expired with no operation behind it.
            match super::give_back(&conn, group_id) {
                Ok(true) => {}
                Ok(false) => log::warn!(
                    target: "agent::propose",
                    "rename group {group_id} didn't start ({reason}) and is no longer a claim to give back"
                ),
                Err(e) => log::warn!(
                    target: "agent::propose",
                    "rename group {group_id} didn't start ({reason}) and couldn't go back to pending: {e}"
                ),
            }
            Err(reason)
        }
    }
}

/// The claim landed before the engine was asked, so a refusal from it would leave the group
/// `approved` with nothing running: off the review list, without a word. Nothing ran, so the
/// group goes back to `pending` and the dialog shows the refusal under it.
///
/// A store that can't take it back is logged, ❌ never raised: the refusal is the answer the
/// user needs, and the group then reads as interrupted on the next launch, which is honest.
fn give_back_after(conn: &Connection, group_id: i64, refusal: &ApprovalRefusal) {
    match super::give_back(conn, group_id) {
        Ok(true) => log::info!(
            target: "agent::suggested_ops",
            "group {group_id} went back to the list: the engine refused it ({refusal:?})"
        ),
        Ok(false) => log::warn!(
            target: "agent::suggested_ops",
            "group {group_id} was refused ({refusal:?}) but is no longer a claim to give back"
        ),
        Err(e) => log::warn!(
            target: "agent::suggested_ops",
            "group {group_id} was refused ({refusal:?}) and couldn't go back to the list: {e}"
        ),
    }
}

/// The volume every source in the group lives on. A sweep may span volumes; a group may not,
/// which is what lets one capture rule cover the whole batch.
fn source_volume_of(conn: &Connection, group_id: i64) -> Result<String, AgentStoreError> {
    Ok(get_group(conn, group_id)?
        .map(|group| group.source_volume_id)
        .unwrap_or_default())
}

/// Every op in the group live set, paged.
fn live_ops(conn: &Connection, group_id: i64) -> Result<Vec<ProposalOp>, AgentStoreError> {
    let mut out = Vec::new();
    let mut offset = 0u32;
    loop {
        let page = page_ops(conn, group_id, OP_PAGE, offset)?;
        let read = page.len() as u32;
        out.extend(page.into_iter().filter(|op| op.status == OpStatus::Pending));
        if read < OP_PAGE {
            return Ok(out);
        }
        offset += read;
    }
}

/// What an approved group runs as: the fallback the review showed, but only while the volume
/// still calls for it (`now`). Pure, so the two ways a stale review can be wrong are pinned.
fn approved_runs_as(shown: Option<RunsAs>, now: Option<RunsAs>) -> Option<RunsAs> {
    if shown == now { now } else { None }
}

/// Turn a claimed group into the executor call its verb describes, or into the fallback the
/// user approved instead (`runs`).
///
/// Every cross-volume verb goes through the ROUTED entry points, the same ones the transfer
/// commands use, so an approved transfer resolves its volumes and its destination path the
/// way a clicked one does. Extract needs no arm of its own: its sources resolve to an
/// `ArchiveVolume` inside that routing, which is the whole reason extract has no operation
/// type.
async fn start_for(
    engine: &Operations,
    group: &ProposalGroup,
    runs: Option<RunsAs>,
    sources: Vec<AppPathBuf>,
    bound: BoundOps<'_>,
    events: Arc<dyn OperationEventSink>,
) -> Result<WriteOperationStartResult, ApprovalRefusal> {
    let BoundOps {
        ops,
        sources: spellings,
        expected,
    } = bound;
    // A trash group on a drive with no Trash, approved as the permanent delete the review
    // offered: the same delete a user gets there, through the same entry point as `Delete`.
    if runs == Some(RunsAs::PermanentDelete) {
        return engine
            .delete_files_start(
                events,
                sources,
                WriteOperationConfig::default(),
                Some(group.source_volume_id.clone()),
                Initiator::Agent,
                Some(expected),
            )
            .await
            .map_err(ApprovalRefusal::Engine);
    }
    match group.verb {
        ProposalVerb::Move => {
            let (path, volume) = target_of(group)?;
            let config = named_config(ops, spellings, &expected);
            engine
                .start_volume_move(
                    events,
                    group.source_volume_id.clone(),
                    sources,
                    volume,
                    path,
                    config,
                    Initiator::Agent,
                    Some(expected),
                )
                .await
                .map_err(ApprovalRefusal::Engine)
        }
        ProposalVerb::Copy | ProposalVerb::Extract => {
            let (path, volume) = target_of(group)?;
            // An extract's ops never carry a name (`GroupIntent::Extract` can't hold one), so
            // this is the default config there.
            let config = named_config(ops, spellings, &expected);
            engine
                .start_volume_copy(
                    events,
                    group.source_volume_id.clone(),
                    sources,
                    volume,
                    path,
                    config,
                    Initiator::Agent,
                    Some(expected),
                )
                .await
                .map_err(ApprovalRefusal::Engine)
        }
        ProposalVerb::Compress => {
            let (path, volume) = target_of(group)?;
            engine
                .start_volume_compress(
                    events,
                    group.source_volume_id.clone(),
                    sources,
                    volume,
                    path,
                    VolumeCopyConfig::default(),
                    Initiator::Agent,
                )
                .await
                .map_err(ApprovalRefusal::Engine)
        }
        ProposalVerb::Trash => engine
            .trash_files_start(
                events,
                sources,
                None,
                WriteOperationConfig::default(),
                Initiator::Agent,
                Some(expected),
            )
            .await
            .map_err(ApprovalRefusal::Engine),
        ProposalVerb::Delete => engine
            .delete_files_start(
                events,
                sources,
                WriteOperationConfig::default(),
                Some(group.source_volume_id.clone()),
                Initiator::Agent,
                Some(expected),
            )
            .await
            .map_err(ApprovalRefusal::Engine),
        // Rename is the one verb whose executor takes per-row destinations and a fingerprint
        // per row, which is exactly what the live preflight capture produces. The group binds
        // the shared PARENT, and each op carries the NAME it becomes.
        ProposalVerb::Rename => {
            // A rename stays on its source volume, so the group stores no destination volume
            // and `target_of`, which needs one, would refuse every rename group.
            let parent = group
                .destination
                .clone()
                .ok_or(ApprovalRefusal::TargetMissing { verb: group.verb })?;
            let rows = rename_rows(&parent, ops, spellings, &expected)?;
            // Routed: where a rename copies (an S3 folder), the batch runs as one move.
            engine
                .start_renames(events, group.source_volume_id.clone(), rows, Initiator::Agent)
                .await
                .map(|started| started.operation)
                .map_err(ApprovalRefusal::Rename)
        }
        // Unreachable: preflight refuses a server group before any claim. Named so a server
        // group can never fall into a file executor's arm.
        ProposalVerb::AddServer | ProposalVerb::EditServer => {
            Err(ApprovalRefusal::NotAccepted(AcceptanceOutcome::NotAFileGroup))
        }
    }
}

/// The rows `start_bulk_rename` takes, built from the group's ops and the live preflight.
///
/// The stored `destination` is a NAME, not a path, because the executor refuses a row whose
/// source and destination parents differ — so the group binds the shared parent and this
/// rejoins the two. A row whose source went unfingerprinted at preflight is dropped, which is
/// the same answer the binding gives every other verb: an unreadable source is not the source
/// anybody reviewed.
fn rename_rows(
    parent: &AppPath,
    ops: &[ProposalOp],
    sources: &[PathBuf],
    expected: &ExpectedSources,
) -> Result<Vec<BulkRenameRow>, ApprovalRefusal> {
    // Crossed as the sources are, so a row's source and destination parents agree.
    let parent = engine_spelling(parent);
    let mut rows = Vec::with_capacity(ops.len());
    for (op, source) in ops.iter().zip(sources) {
        let Some(new_name) = op.destination.as_deref() else {
            return Err(ApprovalRefusal::TargetMissing {
                verb: ProposalVerb::Rename,
            });
        };
        let Some(fingerprint) = expected.fingerprint_of(source) else {
            continue;
        };
        rows.push(BulkRenameRow {
            row_id: op.id.to_string(),
            source: source.clone(),
            destination: parent.join(new_name),
            expected_fingerprint: fingerprint.clone(),
        });
    }
    Ok(rows)
}

/// The default config a clicked transfer gets, plus the name each op lands under: the ONE
/// thing a proposal adds, and the user saw every one in the review. No name means the default.
fn named_config(ops: &[ProposalOp], sources: &[PathBuf], expected: &ExpectedSources) -> VolumeCopyConfig {
    VolumeCopyConfig {
        leaves: TransferLeaves::per_source(transfer_names(ops, sources, expected)),
        ..VolumeCopyConfig::default()
    }
}

/// The names a move's or copy's ops land under, as the engine's per-source leaves take them:
/// keyed by EXACTLY the sources the binding keeps (`sources`, one per op, the engine's own
/// spelling), so a source preflight couldn't read (which the binding drops) takes its name with
/// it. An op keeping its own name sends none.
fn transfer_names(ops: &[ProposalOp], sources: &[PathBuf], expected: &ExpectedSources) -> Vec<(PathBuf, String)> {
    ops.iter()
        .zip(sources)
        .filter_map(|(op, source)| {
            let name = op.destination.clone()?;
            expected.fingerprint_of(source)?;
            Some((source.clone(), name))
        })
        .collect()
}

/// The `std::path` spelling of each op's source, one per op, in order: what preflight stats, what
/// the binding keys by, and what the rename rows and transfer names carry. The ONE place it's
/// derived, so every key agrees.
///
/// `typed` are the start paths, read from the stored text through the door the review's live
/// facts read it through (`wire_path::read_on_volume`), and each crosses as the engine's starts
/// cross it ([`engine_spelling`]), so preflight stats the very file the review showed and the
/// engine looks its binding up by the same key. ❌ Never the stored text as a `PathBuf`: it keeps a
/// trailing `/` the app path drops (a file's stat refuses it), and once escapes go live it holds
/// escapes only the crossing decodes to the name's bytes
/// (`tests.rs::preflight_stats_the_path_the_live_facts_stat`).
fn stored_spellings(typed: &[AppPathBuf]) -> Vec<PathBuf> {
    typed.iter().map(|path| engine_spelling(path)).collect()
}

/// `path` as the engine's starts cross it to `std::path` (its sources and a rename's parent).
fn engine_spelling(path: &AppPath) -> PathBuf {
    path.as_std_legacy().into_owned()
}

/// A group's live ops beside what preflight made of them: each source's spelling (one per op, in
/// op order, [`stored_spellings`]) and the binding over those.
struct BoundOps<'a> {
    ops: &'a [ProposalOp],
    sources: &'a [PathBuf],
    expected: ExpectedSources,
}

/// What preflight captured: the binding, and how many sources gave no fingerprint, by why.
struct Preflight {
    expected: ExpectedSources,
    captured: usize,
    gone: usize,
    unreadable: usize,
}

impl Preflight {
    /// Why nothing is left to run, when no source gave a fingerprint: [`NothingLeft::Gone`] only
    /// when every one is gone (one that didn't answer makes "gone" a guess). `None` while any
    /// source was captured.
    fn nothing_left(&self) -> Option<NothingLeft> {
        if self.captured > 0 {
            return None;
        }
        if self.gone > 0 && self.unreadable == 0 {
            Some(NothingLeft::Gone)
        } else {
            Some(NothingLeft::Unreadable)
        }
    }
}

/// Stat every source as it is RIGHT NOW, and bind the operation to what it finds.
///
/// **This is a live capture, not the stored snapshot, and the two answer different
/// questions.** The creation snapshot on `proposal_ops` came from the drive index when the
/// agent proposed the group: second-precision, often absent, and its job is the review-time
/// question "has this changed since the agent looked at it?" — a stale BELIEF the dialog
/// surfaces so the user can re-judge. An execution binding answers the other question, "has
/// this changed since I showed it to you?", which is a RACE: the window is the review plus
/// however long the operation then waits for its lane, and catching it needs the full
/// nanosecond precision a live stat gives.
///
/// So this stats now, the way the rename preflight does, and the fingerprints never reach the
/// database. A restart must force a fresh preflight rather than resurrect one, because a
/// fingerprint describes a file as it was at review time and nothing else.
///
/// A source that can't be read gets no entry, and the binding drops what it doesn't name, so
/// a source that vanished between the proposal and the review is skipped and reported rather
/// than acted on. Each miss is counted by its typed reason (`SourceUnread`), so a batch left
/// with nothing says whether its files are gone or only didn't answer.
///
/// `sources` are [`stored_spellings`], the same keys the engine looks its binding up by.
async fn capture_expected_sources(volume: &dyn Volume, sources: &[PathBuf]) -> Preflight {
    // The rule the binding itself documents: a local-FS volume answers with the identity the
    // kernel maintains, anything else answers through its own backend.
    let local = volume.local_path().is_some();
    let mut entries = Vec::with_capacity(sources.len());
    let (mut gone, mut unreadable) = (0, 0);
    for source in sources {
        let captured = if local {
            SourceFingerprint::read_local(source)
        } else {
            SourceFingerprint::read_remote(volume, source).await
        };
        match captured {
            Ok(fingerprint) => entries.push((source.clone(), fingerprint)),
            Err(why) => {
                match why {
                    SourceUnread::Gone => gone += 1,
                    SourceUnread::Unreadable => unreadable += 1,
                }
                log::info!(
                    target: "agent::suggested_ops",
                    "{} couldn't be read at preflight ({why:?}), so the operation won't touch it",
                    cmdr_fs::log_private::LogPrivate::path(source)
                );
            }
        }
    }
    Preflight {
        captured: entries.len(),
        expected: ExpectedSources::new(entries),
        gone,
        unreadable,
    }
}

/// The target a destination-binding verb stored, as the routed entry points take it.
fn target_of(group: &ProposalGroup) -> Result<(String, String), ApprovalRefusal> {
    match (group.destination.clone(), group.destination_volume_id.clone()) {
        // The routed entry points take the app path's text, as a click's call sends it.
        (Some(path), Some(volume)) => Ok((path.into_string(), volume)),
        _ => Err(ApprovalRefusal::TargetMissing { verb: group.verb }),
    }
}
