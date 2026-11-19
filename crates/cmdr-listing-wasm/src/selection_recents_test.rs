//! The Selection dialog's recents on the demo's host, through `Demo::invoke`.

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
        "entries": [{ "path": "/Users", "kind": "dir" }],
    }))
    .expect("tree parses");
    Demo::new(tree, Arc::new(Quiet)).expect("valid tree")
}

async fn call(demo: &Demo, command: &str, args: Value) -> Value {
    demo.invoke(command, args)
        .await
        .unwrap_or_else(|e| panic!("{command} refused: {e}"))
}

fn entry(id: &str, query: &str) -> Value {
    json!({
        "id": id, "timestamp": 1_700_000_000_000i64, "mode": "filename", "query": query,
        "filters": {}, "caseSensitive": false, "matchCount": 3,
    })
}

fn queries(rows: &Value) -> Vec<String> {
    rows.as_array()
        .expect("a list")
        .iter()
        .map(|row| row["query"].as_str().expect("a query").to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_selection_is_remembered_once_newest_first_by_the_desktops_key() {
    let demo = demo();
    for (id, query) in [("a", "*.pdf"), ("b", "*.jpg"), ("c", "  *.PDF ")] {
        call(
            &demo,
            "add_recent_selection",
            json!({ "entry": entry(id, query), "maxCount": null }),
        )
        .await;
    }
    let rows = call(&demo, "get_recent_selections", json!({ "limit": null })).await;
    assert_eq!(
        queries(&rows),
        ["  *.PDF ", "*.jpg"],
        "the same query in another case and spacing is one row"
    );
    assert_eq!(
        queries(&call(&demo, "get_recent_selections", json!({ "limit": 1 })).await),
        ["  *.PDF "]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_selection_is_forgotten_trimmed_and_cleared_on_request() {
    let demo = demo();
    for (id, query) in [("a", "one"), ("b", "two"), ("c", "three")] {
        call(
            &demo,
            "add_recent_selection",
            json!({ "entry": entry(id, query), "maxCount": null }),
        )
        .await;
    }
    call(&demo, "remove_recent_selection", json!({ "id": "b" })).await;
    assert_eq!(
        queries(&call(&demo, "get_recent_selections", json!({ "limit": null })).await),
        ["three", "one"]
    );
    call(&demo, "apply_recent_selections_max_count", json!({ "maxCount": 1 })).await;
    assert_eq!(
        queries(&call(&demo, "get_recent_selections", json!({ "limit": null })).await),
        ["three"]
    );
    call(&demo, "clear_recent_selections", json!({})).await;
    assert_eq!(
        call(&demo, "get_recent_selections", json!({ "limit": null })).await,
        json!([])
    );
}

#[test]
fn an_entry_crosses_with_the_apps_keys() {
    let keys = crate::events_test::ts_keys("SelectionHistoryEntry");
    assert_eq!(
        keys,
        [
            "caseSensitive",
            "filters",
            "id",
            "matchCount",
            "mode",
            "query",
            "timestamp"
        ]
        .into_iter()
        .map(String::from)
        .collect()
    );
}
