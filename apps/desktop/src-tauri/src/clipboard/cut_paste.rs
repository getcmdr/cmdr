//! The paste-as-move of files the CLIPBOARD marks as cut (Explorer's Ctrl+X, or Cmdr's own on
//! Windows), followed from the read to the move's end so the source hears how it went.
//!
//! - **Noted** by `read_clipboard_files` when the clipboard says "cut" (`note_read`).
//! - **Claimed** by the move that starts on exactly those paths (`claim_for_operation`, from
//!   `move_files` and `move_between_volumes`, beside the drop-folder claim).
//! - **Settled** by `operations_host::terminal_effects`: a move that completed with nothing
//!   skipped hands back the native paths, and the OS arm tells the source
//!   (`platform::after_cut_paste`); a cancel, a failure, or a skip just forgets it.
//!
//! Portable and pure; only Windows ever notes a cut (macOS and Linux keep the cut in
//! `state.rs`, since Finder's pasteboard has no cut mark).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use cmdr_fs::app_path::AppPathBuf;

use crate::ignore_poison::IgnorePoison;

/// A cut the clipboard carried at the last read, both spellings: the app paths the frontend
/// got (and sends back to start the move), and the native ones the clipboard holds.
#[derive(Debug, Clone)]
struct MarkedCut {
    app: Vec<String>,
    native: Vec<PathBuf>,
}

/// The cut a paste may claim, and the moves that claimed one.
#[derive(Debug, Default)]
pub(crate) struct CutPastes {
    marked: Option<MarkedCut>,
    /// Operation id → the native paths its move took off the clipboard.
    claimed: HashMap<String, Vec<PathBuf>>,
    /// The last few operations that ended before anything claimed for them, and whether each
    /// moved everything: the claim lands after the start returns, and a one-file rename can
    /// finish first (as `drop_in/folders.rs` meets it).
    ended_unclaimed: VecDeque<(String, bool)>,
}

/// How many unclaimed endings to remember: far more than can race one claim.
const ENDED_MEMORY: usize = 64;

/// What a claim found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Claimed {
    /// The move doesn't take the noted cut.
    No,
    /// It does, and it's still running: its end settles it.
    Pending,
    /// It does, and it already ended: these native paths to tell the source about, or `None`
    /// when it left something behind.
    AlreadyEnded(Option<Vec<PathBuf>>),
}

impl CutPastes {
    /// A clipboard read: remembers its paths when the clipboard marks them cut, else forgets
    /// any earlier cut (the clipboard moved on).
    pub(crate) fn note_read(&mut self, app: &[String], native: &[PathBuf], marked_cut: bool) {
        self.marked = (marked_cut && !app.is_empty()).then(|| MarkedCut {
            app: app.to_vec(),
            native: native.to_vec(),
        });
    }

    /// A move started on `sources`: it claims the noted cut when it moves exactly those paths
    /// (any order).
    pub(crate) fn claim(&mut self, operation_id: &str, sources: &[AppPathBuf]) -> Claimed {
        let Some(marked) = &self.marked else {
            return Claimed::No;
        };
        let wanted: HashSet<&str> = marked.app.iter().map(String::as_str).collect();
        let got: HashSet<&str> = sources.iter().map(|s| s.as_str()).collect();
        if wanted != got {
            return Claimed::No;
        }
        let Some(marked) = self.marked.take() else {
            return Claimed::No;
        };
        if let Some(i) = self.ended_unclaimed.iter().position(|(id, _)| id == operation_id) {
            let moved_all = self.ended_unclaimed.remove(i).is_some_and(|(_, moved_all)| moved_all);
            return Claimed::AlreadyEnded(moved_all.then_some(marked.native));
        }
        self.claimed.insert(operation_id.to_string(), marked.native);
        Claimed::Pending
    }

    /// The operation ended: its claimed paths when it moved every one of them (`moved_all`),
    /// else `None`. Either way the claim goes; an operation nothing claimed yet is remembered
    /// for a claim still on its way.
    pub(crate) fn settle(&mut self, operation_id: &str, moved_all: bool) -> Option<Vec<PathBuf>> {
        let Some(native) = self.claimed.remove(operation_id) else {
            if self.ended_unclaimed.len() == ENDED_MEMORY {
                self.ended_unclaimed.pop_front();
            }
            self.ended_unclaimed.push_back((operation_id.to_string(), moved_all));
            return None;
        };
        moved_all.then_some(native)
    }
}

static CUT_PASTES: LazyLock<Mutex<CutPastes>> = LazyLock::new(|| Mutex::new(CutPastes::default()));

/// [`CutPastes::note_read`] on the process-wide ledger.
pub(crate) fn note_read(app: &[String], native: &[PathBuf], marked_cut: bool) {
    CUT_PASTES.lock_ignore_poison().note_read(app, native, marked_cut);
}

/// [`CutPastes::claim`] on the process-wide ledger; a move that already ended settles here.
/// Returns whether it claimed the cut.
pub(crate) fn claim_for_operation(operation_id: &str, sources: &[AppPathBuf]) -> bool {
    let claimed = CUT_PASTES.lock_ignore_poison().claim(operation_id, sources);
    match claimed {
        Claimed::No => false,
        Claimed::Pending => true,
        Claimed::AlreadyEnded(settled) => {
            if let Some(native) = settled {
                super::platform::after_cut_paste(native);
            }
            true
        }
    }
}

/// [`claim_for_operation`] for a move start's result, handed back as it came.
pub(crate) fn claimed_by_start<E>(
    started: Result<cmdr_operations::WriteOperationStartResult, E>,
    sources: &[AppPathBuf],
) -> Result<cmdr_operations::WriteOperationStartResult, E> {
    if let Ok(started) = &started {
        // a move that claims nothing is an ordinary move.
        let _ = claim_for_operation(&started.operation_id, sources);
    }
    started
}

/// [`CutPastes::settle`] on the process-wide ledger, then the OS arm tells the source when the
/// move took everything. Never blocks: the arm works on its own thread.
pub(crate) fn settle_for_operation(operation_id: &str, moved_all: bool) {
    let settled = CUT_PASTES.lock_ignore_poison().settle(operation_id, moved_all);
    if let Some(native) = settled {
        super::platform::after_cut_paste(native);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// POSIX-shaped app paths on every host: the ledger compares text, whatever its flavor.
    fn app(paths: &[&str]) -> Vec<AppPathBuf> {
        paths
            .iter()
            .map(|p| AppPathBuf::parse(p, cmdr_fs::app_path::PathFlavor::Posix).expect("an app path"))
            .collect()
    }

    fn texts(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    /// Opaque stand-ins for the native paths the clipboard held: the ledger only carries them.
    fn natives(paths: &[&str]) -> Vec<PathBuf> {
        paths
            .iter()
            .map(|p| {
                let mut native = PathBuf::new();
                native.push(p);
                native
            })
            .collect()
    }

    fn noted(paths: &[&str], marked_cut: bool) -> CutPastes {
        let mut ledger = CutPastes::default();
        let native: Vec<String> = paths.iter().map(|p| format!("N:{p}")).collect();
        let native: Vec<&str> = native.iter().map(String::as_str).collect();
        ledger.note_read(&texts(paths), &natives(&native), marked_cut);
        ledger
    }

    #[test]
    fn a_move_of_the_cut_paths_claims_them_and_settles_with_their_native_paths() {
        let mut ledger = noted(&["/a/x", "/a/y"], true);
        assert_eq!(ledger.claim("op-1", &app(&["/a/y", "/a/x"])), Claimed::Pending);
        assert_eq!(ledger.settle("op-1", true), Some(natives(&["N:/a/x", "N:/a/y"])));
        assert_eq!(ledger.settle("op-1", true), None, "settles once");
    }

    #[test]
    fn a_move_of_other_paths_claims_nothing() {
        let mut ledger = noted(&["/a/x", "/a/y"], true);
        assert_eq!(ledger.claim("op-1", &app(&["/a/x"])), Claimed::No);
        assert_eq!(
            ledger.claim("op-2", &app(&["/a/x", "/a/y"])),
            Claimed::Pending,
            "the cut stays claimable"
        );
    }

    #[test]
    fn a_copy_on_the_clipboard_is_nothing_to_claim() {
        let mut ledger = noted(&["/a/x"], true);
        ledger.note_read(&texts(&["/a/x"]), &natives(&["N:/a/x"]), false);
        assert_eq!(ledger.claim("op-1", &app(&["/a/x"])), Claimed::No);
    }

    #[test]
    fn one_cut_is_claimed_by_one_move() {
        let mut ledger = noted(&["/a/x"], true);
        assert_eq!(ledger.claim("op-1", &app(&["/a/x"])), Claimed::Pending);
        assert_eq!(ledger.claim("op-2", &app(&["/a/x"])), Claimed::No);
    }

    #[test]
    fn a_move_that_left_something_behind_tells_nobody() {
        let mut ledger = noted(&["/a/x"], true);
        assert_eq!(ledger.claim("op-1", &app(&["/a/x"])), Claimed::Pending);
        assert_eq!(ledger.settle("op-1", false), None);
        assert_eq!(
            ledger.settle("op-1", true),
            None,
            "the claim went with the first settle"
        );
    }

    /// A one-file rename can end before its start returns and claims.
    #[test]
    fn a_move_that_ended_before_its_claim_settles_at_the_claim() {
        let mut ledger = noted(&["/a/x"], true);
        assert_eq!(ledger.settle("op-1", true), None);
        assert_eq!(
            ledger.claim("op-1", &app(&["/a/x"])),
            Claimed::AlreadyEnded(Some(natives(&["N:/a/x"])))
        );

        let mut ledger = noted(&["/a/x"], true);
        assert_eq!(ledger.settle("op-2", false), None);
        assert_eq!(ledger.claim("op-2", &app(&["/a/x"])), Claimed::AlreadyEnded(None));
    }
}
