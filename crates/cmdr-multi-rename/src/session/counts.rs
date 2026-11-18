//! How a preview's rows add up.

use serde::{Deserialize, Serialize};

use cmdr_fs::validation::NameRules;

use crate::plan::{PreviewRow, RowStatus};

/// How a preview's rows add up, for the footer and the Rename button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCounts {
    pub ready: usize,
    pub unchanged: usize,
    /// Every other row: a bad or taken name, a duplicate, a file that's gone, a
    /// media field the file doesn't have.
    pub problems: usize,
    /// Rows whose media fields are still being read: neither ready nor a problem yet.
    pub reading: usize,
    /// Ready rows whose new name the folder takes but won't keep portable
    /// (`PreviewRow::warning`): they rename, and the sheet says Windows won't read them.
    pub not_portable: usize,
    /// Of those, the ones Linux misreads too (`PortabilityWarning::reaches_linux`).
    pub not_portable_on_linux: usize,
}

impl PreviewCounts {
    pub(crate) fn of(rows: &[PreviewRow], rules: NameRules) -> Self {
        let mut counts = Self::default();
        for row in rows {
            match row.status {
                RowStatus::Ready => counts.ready += 1,
                RowStatus::Unchanged => counts.unchanged += 1,
                RowStatus::Reading => counts.reading += 1,
                _ => counts.problems += 1,
            }
            if let Some(warning) = &row.warning {
                counts.not_portable += 1;
                counts.not_portable_on_linux += usize::from(warning.reaches_linux(rules));
            }
        }
        counts
    }
}

#[cfg(test)]
#[path = "counts_test.rs"]
mod tests;
