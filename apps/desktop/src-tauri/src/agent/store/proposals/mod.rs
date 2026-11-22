//! The proposal spine in `main.db`: sweeps, the reviewable groups inside them, their ops,
//! and the server-owned acceptance record the claim transaction binds against.
//!
//! Three levels, and the middle one is one executor call:
//!
//! - **Sweep** (`proposal_sets`): one agent wake's output. Display and provenance only.
//! - **Group** (`proposals`): the reviewable, approvable, executable unit. Exactly ONE call
//!   to ONE executor, which is why `source_volume_id` lives here and not on the sweep.
//! - **Op** (`proposal_ops`): one path, which may be a file or a whole directory.
//!
//! ## What this module owns, and what it refuses to own
//!
//! Persistence and the lifecycle machine, nothing else: it takes a [`NewGroup`] and hands
//! back ids, pages, counts, and typed refusals. Selector resolution, analytics, and anything
//! that reads the drive index live a layer up in `agent/suggested_ops/`, so this module keeps
//! `rusqlite` as its only real dependency.
//!
//! ## The shapes that can't be built wrong
//!
//! [`GroupIntent`] pairs each verb with the target its executor requires AND the op shape it
//! takes, so a trash group carrying a destination folder, or a move group whose ops each
//! carry their own destination, is unrepresentable rather than rejected at runtime. The
//! store never validates the pairing because it never can be wrong.
//!
//! Depth (the claim transaction's exact order, the digest, the recovery sweep, the DDL
//! rationale): `DETAILS.md`.

mod claim;
mod complete;
mod draft;
mod read;
mod recovery;
mod server;
mod thread_line;
mod write;

#[cfg(test)]
mod tests;

pub use claim::{
    AcceptanceOutcome, ClaimOutcome, ClaimRefusal, ClaimedGroup, OpBinding, OpSelection, RejectOutcome,
    claim_group_for_execution, live_binding, record_acceptance, reject_group, release_claim,
};
pub use complete::{CompleteOutcome, mark_group_completed, record_op_outcome};
pub use draft::{ServerDraft, ServerEdit, ServerProposal};
pub use read::{
    GroupSummary, ProposalOp, ProposalSweep, count_ops, count_pending, get_group, get_sweep, list_groups, page_ops,
    read_server_proposal, rejected_groups_since,
};
pub use recovery::recover_interrupted_groups;
pub use server::{ClaimedServer, ServerClaimOutcome, claim_server_group, mark_server_group_completed};
pub use thread_line::{
    RunningLine, ThreadLine, decision_of, open_running_line, rewrite_line, running_lines, settle_line,
};
pub use write::{
    NewGroup, NewOp, NewRename, NewSweep, OpSnapshot, ReproposeOutcome, TransferOp, create_group, create_sweep,
    repropose_group,
};

use cmdr_fs::app_path::{AppPathBuf, ESCAPES_LIVE, PathFlavor, is_escape};

use crate::agent::types::{ProposalStatus, ProposalVerb, Reversibility};
use crate::location::Location;

/// The app path a stored destination's text names, as [`WritableDestination::new`] writes it
/// and [`ProposalGroup::destination`] reads it back: this host's flavor, a POSIX `/x` (a
/// share's or a phone's path on Windows), or this host's native spelling (`C:\x`, what a
/// `Path`'s text is on Windows). `None` for a `..` (the MCP path door answers those
/// before a group is built) and for text that's no app path.
///
/// Also `None`, while escapes aren't live (`ESCAPES_LIVE`), for text holding an escape-range
/// code point: its typed spelling escapes it, and the write starts still read the text back
/// as a `PathBuf`, which would name a different folder. Refused at creation, never a write to
/// the wrong place; 10f lifts it with the escape gate.
pub(crate) fn typed_place(text: &str) -> Option<AppPathBuf> {
    if !ESCAPES_LIVE && text.chars().any(is_escape) {
        return None;
    }
    AppPathBuf::from_wire(text)
        .or_else(|_| AppPathBuf::parse(text, PathFlavor::Posix))
        // This host's native spelling: a rename group's parent is a `Path`'s text
        // (`tools/actions/multi_rename.rs`), `C:\x` on Windows, which no app-path grammar
        // reads. Without it every such approval refused as `TargetMissing` there.
        // ❗ A `..` stays refused here too: the Windows native crossing folds it lexically,
        // which by text can name another folder across a link.
        .or_else(|refusal| {
            let native = std::path::Path::new(text);
            if native.components().any(|c| c == std::path::Component::ParentDir) {
                return Err(refusal);
            }
            AppPathBuf::from_native_unicode(native)
        })
        .ok()
}

/// A place a group may write into: a [`Location`] proven not to be inside an archive.
///
/// Copy or move INTO a zip, and move OUT of one, plan their work from their own `WalkDir`
/// rather than the per-source engine, so there is nowhere to apply a source binding. The
/// write engine refuses a bound transfer that lands on one of those routes, which is the
/// right backstop and the wrong layer on its own: a user-started copy-into-zip works while
/// an approved one refuses, and it refuses AFTER the user approved, which is exactly the
/// agent-specific execution behaviour the guiding principle forbids.
///
/// So the constraint lives at the PROPOSAL boundary instead, where a refusal costs the agent
/// a retry and costs the user nothing. A group with such a destination cannot be built, the
/// same way a trash group carrying a destination cannot.
///
/// Extract is unaffected: its SOURCES are archive-inner, its destination an ordinary folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritableDestination(Location);

/// Why a place can't be a group's destination ([`WritableDestination::new`]), each its own
/// refusal for the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnwritableDestination {
    /// A ROUTE serves it: inside an archive, or inside a repo's virtual `.git` trees.
    Routed,
    /// While escapes aren't live, its text holds an escape-range code point ([`typed_place`]).
    EscapeRangeName,
    /// Its text names no app path (relative, or holding a `..`).
    NotAPath,
}

impl WritableDestination {
    /// The destination, or [`UnwritableDestination::Routed`] when a ROUTE serves it: inside
    /// an archive, or inside a repo's virtual `.git` trees. Neither takes a write, so a
    /// proposal aimed there is one the user would approve and the backend would then refuse.
    ///
    /// No I/O, and deliberately permissive at the one edge that matters: the
    /// archive FILE itself is a perfectly good compress target and an ordinary file
    /// to copy, so only a path continuing INSIDE one is refused.
    /// `path_routes_over_its_parent` draws that line already.
    ///
    /// Also refused for a path no app path is made of ([`typed_place`]), so every stored
    /// destination reads back typed; the path it keeps is that app path's own text.
    ///
    /// # Errors
    ///
    /// [`UnwritableDestination`].
    pub fn new(location: Location) -> Result<Self, UnwritableDestination> {
        if !ESCAPES_LIVE && location.path.chars().any(is_escape) {
            return Err(UnwritableDestination::EscapeRangeName);
        }
        let typed = typed_place(&location.path).ok_or(UnwritableDestination::NotAPath)?;
        if crate::file_system::volume::manager::path_routes_over_its_parent(std::path::Path::new(&location.path)) {
            return Err(UnwritableDestination::Routed);
        }
        Ok(Self(Location {
            volume_id: location.volume_id,
            path: typed.into_string(),
        }))
    }

    /// The location it wraps.
    pub fn location(&self) -> &Location {
        &self.0
    }
}

/// What a group asks for: the verb, the target that verb's executor binds, and the ops in
/// the shape that verb takes.
///
/// One enum rather than a verb plus loose optional fields, because the pairing is exact:
/// `trash_files_start` takes raw paths and no target, `move_files_start` takes one shared
/// destination directory (and a name per source, so move and copy ops may carry one), and
/// `start_bulk_rename`'s rows always carry their own destinations (it refuses a row whose
/// source and destination parents differ, so its group binds the shared PARENT rather than a
/// target). Extract, compress, trash, and delete ops can't carry a name at all.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "a handful are built per proposal call and written straight away: boxing the draft buys nothing"
)]
pub enum GroupIntent {
    /// Move every source into one shared destination directory, each under its own name or
    /// the one its op carries.
    Move {
        destination: WritableDestination,
        sources: Vec<TransferOp>,
    },
    /// Copy every source into one shared destination directory, each under its own name or
    /// the one its op carries.
    Copy {
        destination: WritableDestination,
        sources: Vec<TransferOp>,
    },
    /// Extract archive-inner sources into one shared destination directory.
    Extract {
        destination: WritableDestination,
        sources: Vec<NewOp>,
    },
    /// Compress every source into one target archive.
    Compress {
        archive: WritableDestination,
        sources: Vec<NewOp>,
        /// Whether that archive already exists. It decides reversibility: creating one is
        /// undone by deleting it, overwriting one is not undoable at all (the seed is
        /// unconditional and the prior bytes aren't retained).
        overwrites_existing: bool,
    },
    /// Rename sources in place. Every source shares `parent`, and each op carries the name it
    /// becomes.
    Rename { parent: String, renames: Vec<NewRename> },
    /// Move every source to the trash. Binds no target at all.
    Trash { sources: Vec<NewOp> },
    /// Delete every source permanently. Binds no target, and nothing takes it back.
    Delete { sources: Vec<NewOp> },
    /// Save a server: a new one, or (with `edit`) changes to a saved one. ZERO ops: the draft
    /// is the whole proposal, stored beside the group in its own transaction, and the group
    /// takes the server claim rather than the acceptance record (`DETAILS.md` § A server
    /// group).
    SaveServer {
        draft: ServerDraft,
        edit: Option<ServerEdit>,
    },
}

impl GroupIntent {
    /// The stored verb token's variant.
    pub fn verb(&self) -> ProposalVerb {
        match self {
            GroupIntent::Move { .. } => ProposalVerb::Move,
            GroupIntent::Copy { .. } => ProposalVerb::Copy,
            GroupIntent::Extract { .. } => ProposalVerb::Extract,
            GroupIntent::Compress { .. } => ProposalVerb::Compress,
            GroupIntent::Rename { .. } => ProposalVerb::Rename,
            GroupIntent::Trash { .. } => ProposalVerb::Trash,
            GroupIntent::Delete { .. } => ProposalVerb::Delete,
            GroupIntent::SaveServer { edit: None, .. } => ProposalVerb::AddServer,
            GroupIntent::SaveServer { edit: Some(_), .. } => ProposalVerb::EditServer,
        }
    }

    /// How far an approved group of this intent can be taken back. A fact the review dialog
    /// DISCLOSES; per the guiding principle it is never a reason to refuse a group.
    pub fn reversibility(&self) -> Reversibility {
        match self {
            GroupIntent::Move { .. } | GroupIntent::Trash { .. } | GroupIntent::Rename { .. } => {
                Reversibility::RestoreMove
            }
            GroupIntent::Copy { .. } | GroupIntent::Extract { .. } => Reversibility::DeleteWhatWasWritten,
            GroupIntent::Compress {
                overwrites_existing, ..
            } => {
                if *overwrites_existing {
                    Reversibility::Irreversible
                } else {
                    Reversibility::DeleteWhatWasWritten
                }
            }
            GroupIntent::Delete { .. } => Reversibility::Irreversible,
            GroupIntent::SaveServer { .. } => Reversibility::ServerSettings,
        }
    }

    /// The target this intent binds, as the two columns store it: a path plus the volume it
    /// lives on. A rename binds its shared parent on the group's own source volume, so it
    /// carries no volume of its own; trash and delete bind nothing.
    pub(super) fn stored_destination(&self) -> (Option<&str>, Option<&str>) {
        match self {
            GroupIntent::Move { destination, .. }
            | GroupIntent::Copy { destination, .. }
            | GroupIntent::Extract { destination, .. } => {
                let destination = destination.location();
                (Some(destination.path.as_str()), Some(destination.volume_id.as_str()))
            }
            GroupIntent::Compress { archive, .. } => {
                let archive = archive.location();
                (Some(archive.path.as_str()), Some(archive.volume_id.as_str()))
            }
            GroupIntent::Rename { parent, .. } => (Some(parent.as_str()), None),
            GroupIntent::Trash { .. } | GroupIntent::Delete { .. } | GroupIntent::SaveServer { .. } => (None, None),
        }
    }

    /// What `proposals.source_volume_id` (NOT NULL) holds. A file group's is the volume its
    /// sources live on, as given. A server group has no sources: an edit stores the place it
    /// changes, an add `""`, and nothing reads either.
    pub(super) fn stored_source_volume<'a>(&'a self, given: &'a str) -> &'a str {
        match self {
            GroupIntent::SaveServer { edit: Some(edit), .. } => edit.editing.as_str(),
            GroupIntent::SaveServer { edit: None, .. } => "",
            _ => given,
        }
    }

    /// How many ops this intent carries. `COUNT`-free: it's the in-memory list's length.
    pub fn op_count(&self) -> usize {
        match self {
            GroupIntent::Move { sources, .. } | GroupIntent::Copy { sources, .. } => sources.len(),
            GroupIntent::Extract { sources, .. }
            | GroupIntent::Compress { sources, .. }
            | GroupIntent::Trash { sources }
            | GroupIntent::Delete { sources } => sources.len(),
            GroupIntent::Rename { renames, .. } => renames.len(),
            GroupIntent::SaveServer { .. } => 0,
        }
    }

    /// Every op flattened into the row shape: source path, the per-op destination NAME (a
    /// rename's always, a move or copy op's when it lands under a new one), and the creation
    /// snapshot.
    pub(super) fn rows(&self) -> Box<dyn Iterator<Item = (&str, Option<&str>, Option<&OpSnapshot>)> + '_> {
        match self {
            GroupIntent::Move { sources, .. } | GroupIntent::Copy { sources, .. } => Box::new(
                sources
                    .iter()
                    .map(|op| (op.source_path.as_str(), op.new_name.as_deref(), op.snapshot.as_ref())),
            ),
            GroupIntent::Extract { sources, .. }
            | GroupIntent::Compress { sources, .. }
            | GroupIntent::Trash { sources }
            | GroupIntent::Delete { sources } => Box::new(
                sources
                    .iter()
                    .map(|op| (op.source_path.as_str(), None, op.snapshot.as_ref())),
            ),
            GroupIntent::Rename { renames, .. } => Box::new(renames.iter().map(|op| {
                (
                    op.source_path.as_str(),
                    Some(op.new_name.as_str()),
                    op.snapshot.as_ref(),
                )
            })),
            GroupIntent::SaveServer { .. } => Box::new(std::iter::empty()),
        }
    }
}

/// One group as stored, header only. Its ops are read paged (`page_ops`) and counted with
/// `COUNT(*)` (`count_ops`) — a group of 60 000 is legitimate, so nothing loads them to
/// answer a question about them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalGroup {
    pub id: i64,
    pub set_id: i64,
    pub seq: i64,
    pub verb: ProposalVerb,
    pub status: ProposalStatus,
    pub source_volume_id: String,
    /// The shared destination dir, rename parent, or archive path, typed ([`typed_place`]);
    /// `None` for trash/delete, and for a row an older build stored untyped (its approval
    /// refuses as `TargetMissing`).
    pub destination: Option<AppPathBuf>,
    /// Which volume `destination` lives on. `None` for a rename (the source volume's) and for
    /// the verbs that bind no target at all.
    pub destination_volume_id: Option<String>,
    pub reversible: Reversibility,
    pub display_name: String,
    pub rationale: Option<String>,
    /// The selector this group froze, as JSON, when a selector produced it. Display and
    /// provenance only: ❌ a selector is NEVER re-resolved.
    pub selector: Option<String>,
    pub created_at: i64,
    /// When the group left `pending`.
    pub decided_at: Option<i64>,
    /// Bumped by every amend (`repropose_group`); 0 at creation. An approval names the revision
    /// the user read, and preflight refuses any other ([`AcceptanceOutcome::RevisionChanged`]).
    pub revision: i64,
    /// The folder every op sits directly in, written with the op list ([`shared_parent`]); `None`
    /// when the ops span folders.
    pub source_folder: Option<String>,
}

/// Decode a stored token, or report which column held what.
pub(super) fn decode_token<T>(
    column: &'static str,
    token: String,
    parse: fn(&str) -> Option<T>,
) -> Result<T, super::AgentStoreError> {
    parse(&token).ok_or(super::AgentStoreError::Decode { column, value: token })
}

/// The folder every one of `paths` sits directly in, or `None` when they don't all share one
/// (or there are none). Stops at the first path elsewhere. Written to `proposals.source_folder`
/// whenever a group's op list is (creation, amend), so the review names the folder from one
/// column instead of streaming 60 000 paths on every refresh.
pub(super) fn shared_parent<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut shared: Option<&str> = None;
    for path in paths {
        let parent = std::path::Path::new(path).parent()?.to_str()?;
        match shared {
            None => shared = Some(parent),
            Some(seen) if seen == parent => {}
            Some(_) => return None,
        }
    }
    shared.map(str::to_string)
}
