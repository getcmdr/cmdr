//! Filling in the files-changed counts the `.git/commits/` walk leaves out.
//!
//! The walk lists every commit `CountPending` (`cmdr_git::GitPortal::commit_rows_with_counts`
//! is the other half), so the list and its total show at once. Then:
//!
//! - **A listing of at most `GitPortal::COUNT_ALL_LIMIT` commits is counted in full**
//!   ([`count_all`]), in the background, a batch at a time, so the cells fill and a
//!   Size sort settles as it goes. That's what makes Size sort work there.
//! - **A longer one is counted only where a pane looks** ([`fill`]), and Size sort
//!   isn't offered: the pane learns that from [`CommitCountPass::TooMany`].
//!
//! Counted rows are patched into the listing as `DirectoryChange::Modified`, so they
//! re-sort and stay counted. They go to the listing engine alone, ❌ never
//! `listing_host::directory_changed`, which would also hand virtual git rows to the
//! file index.

use std::path::PathBuf;
use std::time::Duration;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::git_meta::GitEntryMeta;
use cmdr_fs::volume::DirectoryChange;
use cmdr_git::GitPortal;
use cmdr_listing::ListingId;

use super::wiring::portal;
use cmdr_fs::deadline::{BlockingBudget, timeout_detached_typed};

/// One batch is at most a few hundred tree diffs at ~230 µs each, so this only
/// bounds a repo on a stalled mount; past it the cells keep their `…`.
const BATCH_TIMEOUT: Duration = Duration::from_secs(5);
/// More rows than any pane shows at once, so one visible-range ask can't queue
/// minutes of diffs behind a runaway caller.
const MAX_FILL_ROWS: usize = 500;
/// Rows per batch of the whole-listing count: ~50 ms of diffs, so the cells
/// visibly fill and a Size sort settles as it goes.
const COUNT_ALL_BATCH: usize = 200;
/// Two at a time: both panes can be on a commit list, and a scroll re-asks faster
/// than a batch of diffs finishes.
static COUNTING: BlockingBudget = BlockingBudget::new(2);

/// What [`count_all`] decided for a listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CommitCountPass {
    /// Every uncounted row is being counted in the background, a batch at a time;
    /// each batch lands as an ordinary `directory-diff`.
    Counting,
    /// The listing holds more commits than `limit`: only rows a pane shows get
    /// counted, and Size sort isn't available there.
    TooMany {
        /// `GitPortal::COUNT_ALL_LIMIT`, for the pane's wording.
        limit: u32,
    },
    /// Nothing to count: the listing is gone, or every row already has its count.
    NothingToCount,
}

/// Starts counting every uncounted row of `id` when the listing is short enough,
/// and says which way it went. Answers at once; the counting stops by itself when
/// the listing closes.
pub fn count_all(id: ListingId) -> CommitCountPass {
    let Ok(entries) = crate::listing_host::listings().entries(&id) else {
        return CommitCountPass::NothingToCount;
    };
    let pending = match plan_count_all(&entries) {
        CountAllPlan::Count(pending) => pending,
        CountAllPlan::TooMany => {
            return CommitCountPass::TooMany {
                limit: GitPortal::COUNT_ALL_LIMIT as u32,
            };
        }
        CountAllPlan::NothingToCount => return CommitCountPass::NothingToCount,
    };
    tauri::async_runtime::spawn(async move {
        for batch in pending.chunks(COUNT_ALL_BATCH) {
            if !count_and_patch(&id, batch.to_vec()).await {
                break;
            }
        }
    });
    CommitCountPass::Counting
}

/// What [`count_all`] would do with a listing's rows. Pure, so the limit is testable
/// without a listing engine.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum CountAllPlan {
    /// Count these rows: the uncounted ones of a short enough listing.
    Count(Vec<PathBuf>),
    /// More rows than `GitPortal::COUNT_ALL_LIMIT`.
    TooMany,
    /// Every row already has its count.
    NothingToCount,
}

pub(super) fn plan_count_all(entries: &[FileEntry]) -> CountAllPlan {
    if entries.len() > GitPortal::COUNT_ALL_LIMIT {
        return CountAllPlan::TooMany;
    }
    let pending: Vec<PathBuf> = entries
        .iter()
        .filter(|e| matches!(e.git_meta, Some(GitEntryMeta::CountPending { .. })))
        .map(|e| crate::app_text::as_std_path_text(&e.path))
        .collect();
    if pending.is_empty() {
        CountAllPlan::NothingToCount
    } else {
        CountAllPlan::Count(pending)
    }
}

/// Counts the rows at `paths` (the ones a pane shows) and patches them into `id`.
pub async fn fill(id: ListingId, paths: Vec<String>) {
    let paths: Vec<PathBuf> = paths.iter().take(MAX_FILL_ROWS).map(|row| crate::app_text::as_std_path_text(row)).collect();
    let _ = count_and_patch(&id, paths).await;
}

/// Counts `paths` under the shared budget and patches the counted rows into `id`'s
/// listing. Answers `false` when there's no point going on: the listing is gone, or
/// the batch overran its deadline.
async fn count_and_patch(id: &ListingId, paths: Vec<PathBuf>) -> bool {
    let listings = crate::listing_host::listings();
    let Some((volume_id, parent)) = listings.location(id) else {
        return false;
    };
    // Under the budget, so a fast scroll queues cheap futures instead of pool threads.
    let counting = async move {
        COUNTING
            .run(move || portal().commit_rows_with_counts(&paths))
            .await
            .map_err(|_| ())
    };
    let Ok(rows) = timeout_detached_typed(BATCH_TIMEOUT, || (), |_| (), counting).await else {
        return false;
    };
    for row in rows {
        listings.directory_changed(&volume_id, &parent, DirectoryChange::Modified(row));
    }
    true
}
