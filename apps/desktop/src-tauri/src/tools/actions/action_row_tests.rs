//! A kind's trailing action row (the Servers list's "Add server…", `views/trailing-row.ts`) in
//! the MCP pane state: no file tool ever takes it as a source or target, by cursor or by name.

use super::*;
use crate::ui_state::pane_state::PaneFileEntry;

/// One server, then the "Add server…" row, with the cursor at `cursor_index`.
fn servers_with_action_row(cursor_index: usize) -> PaneState {
    PaneState {
        path: "smb://".to_string(),
        files: vec![
            PaneFileEntry {
                name: "Testnas".to_string(),
                path: "smb://testnas".to_string(),
                is_directory: true,
                ..Default::default()
            },
            PaneFileEntry {
                name: "+ Add server…".to_string(),
                action: Some("servers.connect".to_string()),
                ..Default::default()
            },
        ],
        cursor_index,
        total_files: 1,
        ..Default::default()
    }
}

#[test]
fn an_operation_with_the_cursor_on_the_action_row_has_nothing_to_act_on() {
    let msg = file_ops::empty_operation_error(&servers_with_action_row(1), "left", "copy").expect("should reject");
    assert!(msg.contains("Nothing to copy"));
    assert!(msg.contains("action row"));
    // On a real row, the cursor fallback proceeds as ever.
    assert!(file_ops::empty_operation_error(&servers_with_action_row(0), "left", "copy").is_none());
}

#[test]
fn target_paths_never_take_the_action_row() {
    let err = resolve_pane_target_paths(&servers_with_action_row(1), None).expect_err("cursor on the action row");
    assert_eq!(err.code, INVALID_PARAMS);
    // By name, it's no item either.
    let names = vec!["+ Add server…".to_string()];
    assert!(resolve_pane_target_paths(&servers_with_action_row(0), Some(&names)).is_err());
    assert_eq!(
        resolve_pane_target_paths(&servers_with_action_row(0), None).expect("the real row"),
        vec!["smb://testnas".to_string()]
    );
}
