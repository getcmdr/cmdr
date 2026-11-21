//! "Go to path" end to end on the demo's host: a real `Demo`, its in-memory volume,
//! and the Tauri-shaped JSON in and out, against the desktop's answers.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::commands::Demo;
use crate::events::EventSink;
use crate::tree::DemoTree;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: &str, _payload: Value) {}
}

fn demo() -> Demo {
    let tree: DemoTree = serde_json::from_value(json!({
        "volumeId": "root",
        "root": "/",
        "home": "/Users/you",
        "entries": [
            { "path": "/Users", "kind": "dir" },
            { "path": "/Users/you", "kind": "dir" },
            { "path": "/Users/you/Downloads", "kind": "dir" },
            { "path": "/Users/you/notes.md", "kind": "file", "size": 10 },
            { "path": "/Users/you/Shortcut", "kind": "symlink", "symlinkTarget": "/Users/you/Downloads" },
        ],
    }))
    .expect("tree parses");
    Demo::new(tree, Arc::new(Quiet)).expect("valid tree")
}

async fn call(demo: &Demo, command: &str, args: Value) -> Value {
    demo.invoke(command, args)
        .await
        .unwrap_or_else(|e| panic!("{command} refused: {e}"))
}

async fn go(demo: &Demo, input: &str, base_dir: &str) -> Value {
    call(
        demo,
        "resolve_go_to_path",
        json!({ "input": input, "baseDir": base_dir }),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tilde_path_to_a_folder_navigates_into_it() {
    let demo = demo();
    assert_eq!(
        go(&demo, "~/Downloads", "/").await,
        json!({ "kind": "directory", "path": "/Users/you/Downloads", "crossedLink": false })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_names_its_folder_and_itself_relative_to_the_pane() {
    let demo = demo();
    assert_eq!(
        go(&demo, "notes.md", "/Users/you").await,
        json!({ "kind": "file", "path": "/Users/you/notes.md", "parentDir": "/Users/you", "fileName": "notes.md", "crossedLink": false })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_to_a_folder_classifies_as_the_folder_on_the_link_path() {
    let demo = demo();
    assert_eq!(
        go(&demo, "~/Shortcut", "/").await,
        json!({ "kind": "directory", "path": "/Users/you/Shortcut", "crossedLink": false })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_path_lands_on_its_nearest_existing_ancestor() {
    let demo = demo();
    assert_eq!(
        go(&demo, "~/Downloads/nope/deeper", "/").await,
        json!({ "kind": "nearestAncestor", "requested": "/Users/you/Downloads/nope/deeper", "ancestorDir": "/Users/you/Downloads" })
    );
    assert_eq!(
        go(&demo, "/nowhere", "/Users/you").await,
        json!({ "kind": "nearestAncestor", "requested": "/nowhere", "ancestorDir": "/" })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn blank_input_is_invalid() {
    let demo = demo();
    assert_eq!(go(&demo, "   ", "/").await["kind"], "invalid");
}

#[tokio::test(flavor = "multi_thread")]
async fn recent_paths_keep_one_row_per_path_newest_first_and_forget_on_request() {
    let demo = demo();
    for (id, path) in [("a", "/Users/you"), ("b", "/Users/you/Downloads"), ("c", "/Users/you")] {
        call(
            &demo,
            "add_recent_path",
            json!({ "entry": { "id": id, "timestamp": 1_700_000_000_000i64, "path": path } }),
        )
        .await;
    }
    let paths = |rows: &Value| -> Vec<String> {
        rows.as_array()
            .expect("a list")
            .iter()
            .map(|row| row["path"].as_str().expect("a path").to_string())
            .collect()
    };
    let rows = call(&demo, "get_recent_paths", json!({})).await;
    assert_eq!(paths(&rows), ["/Users/you", "/Users/you/Downloads"]);
    let first = rows[0]["id"].as_str().expect("an id").to_string();
    call(&demo, "remove_recent_path", json!({ "id": first })).await;
    assert_eq!(
        paths(&call(&demo, "get_recent_paths", json!({})).await),
        ["/Users/you/Downloads"]
    );
    call(&demo, "clear_recent_paths", json!({})).await;
    assert_eq!(call(&demo, "get_recent_paths", json!({})).await, json!([]));
}

#[test]
fn a_recent_path_entry_crosses_with_the_apps_keys() {
    let keys = crate::events_test::ts_keys("RecentPathEntry");
    assert_eq!(
        keys,
        ["id", "path", "timestamp"].into_iter().map(String::from).collect()
    );
}
