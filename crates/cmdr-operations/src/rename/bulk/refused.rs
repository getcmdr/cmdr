//! The folder's name rules, met by Ask Cmdr's bulk renames like every other
//! rename (`name_rules::check_new_name`, #1229): a row whose new name the folder
//! refuses fails in place, before anything moves, and the rest run.

use cmdr_fs::twin_path;

use super::super::super::host::EngineHost;
use super::{BulkRenameOutcome, BulkRenameRow, BulkRenameRun};

/// Which rows' new names their folder refuses, in row order. Every row shares
/// one folder (`NotInOneFolder` refused the rest), so the rules are looked up
/// once. A `.` or `..` target is caught on the raw path: `Path::file_name`
/// can't see it (`name_rules::ends_in_dot_name`).
pub(super) async fn refused_rows(host: &EngineHost, volume_id: &str, rows: &[BulkRenameRow]) -> Vec<bool> {
    let Some(folder) = rows.first().and_then(|row| twin_path::folder_of(&row.destination)) else {
        return vec![false; rows.len()];
    };
    let rules = crate::name_rules::name_rules_at(host, volume_id, folder).await;
    rows.iter()
        .map(|row| {
            let name = row
                .destination
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            crate::name_rules::ends_in_dot_name(&row.destination)
                || cmdr_fs::validation::validate_filename(&name, rules).is_err()
        })
        .collect()
}

/// `rows` split by `refused`: the ones to run, and the refused ones.
pub(super) fn split_refused(rows: &[BulkRenameRow], refused: &[bool]) -> (Vec<BulkRenameRow>, Vec<BulkRenameRow>) {
    let (kept, refused): (Vec<_>, Vec<_>) = rows
        .iter()
        .cloned()
        .zip(refused.iter().copied())
        .partition(|(_, is_refused)| !is_refused);
    (
        kept.into_iter().map(|(row, _)| row).collect(),
        refused.into_iter().map(|(row, _)| row).collect(),
    )
}

impl BulkRenameRun {
    /// The run over every row, in row order: this run's outcomes for the rows
    /// that ran, `Failed` for the rows `refused` marks.
    pub(super) fn with_refused(self, refused: &[bool]) -> Self {
        let mut ran = self.outcomes.into_iter();
        let outcomes = refused
            .iter()
            .map(|&is_refused| {
                if is_refused {
                    BulkRenameOutcome::Failed
                } else {
                    // A run that stopped early has no outcome for the rest: they never moved.
                    ran.next().unwrap_or(BulkRenameOutcome::Skipped)
                }
            })
            .collect();
        Self {
            outcomes,
            cancelled: self.cancelled,
        }
    }
}
