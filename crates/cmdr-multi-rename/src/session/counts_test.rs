//! `PreviewCounts` over rows that warn: a portability warning is no problem.

use cmdr_fs::validation::{NameRules, PortabilityWarning};

use super::PreviewCounts;
use crate::plan::{PreviewRow, RowStatus};

fn row(warning: Option<PortabilityWarning>) -> PreviewRow {
    PreviewRow {
        row: 0,
        old_name: "a.txt".to_string(),
        new_name: "b.txt".to_string(),
        status: RowStatus::Ready,
        icon_id: None,
        is_directory: false,
        edited: false,
        warning,
    }
}

/// A warned row still counts as ready, and the counts say how many warn, and
/// whether Linux misreads them too, across every row (not only the first page).
#[test]
fn a_warned_row_is_ready_and_counted() {
    let rows = [
        row(Some(PortabilityWarning::WindowsCharacter {
            character: ":".to_string(),
        })),
        row(Some(PortabilityWarning::ReservedName {
            name: "CON".to_string(),
        })),
        row(None),
    ];
    let counts = PreviewCounts::of(&rows, NameRules::windows_format());
    assert_eq!(
        (counts.ready, counts.not_portable, counts.not_portable_on_linux),
        (3, 2, 1)
    );
    let on_a_share = PreviewCounts::of(&rows, NameRules::smb_share());
    assert_eq!(on_a_share.not_portable_on_linux, 0);
}
