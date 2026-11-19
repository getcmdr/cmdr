//! "Calculate folder sizes" on the demo's host: the fold already sized every folder
//! row, so a count plans nothing and says so, as the desktop does over a volume its
//! index covers; an unknown listing is `gone`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::commands::Demo;
use crate::events::EventSink;
use crate::tree::DemoTree;

#[derive(Default)]
struct Collector(Mutex<Vec<(String, Value)>>);

impl EventSink for Collector {
    fn emit(&self, event: &str, payload: Value) {
        self.0
            .lock()
            .expect("collector lock")
            .push((event.to_string(), payload));
    }
}

impl Collector {
    fn completed(&self, listing_id: &str) -> bool {
        let events = self.0.lock().expect("collector lock");
        events
            .iter()
            .any(|(name, p)| name == "listing-complete" && p["listingId"] == listing_id)
    }
}

fn demo() -> (Demo, Arc<Collector>) {
    let tree: DemoTree = serde_json::from_value(json!({
        "volumeId": "root",
        "root": "/",
        "entries": [
            { "path": "/Users", "kind": "dir" },
            { "path": "/Users/you", "kind": "dir" },
            { "path": "/Users/you/Documents", "kind": "dir" },
            { "path": "/Users/you/Documents/a.txt", "kind": "file", "size": 30 },
            { "path": "/Users/you/Music", "kind": "dir" },
            { "path": "/Users/you/notes.md", "kind": "file", "size": 12 },
        ],
    }))
    .expect("tree parses");
    let events = Arc::new(Collector::default());
    let demo = Demo::new(tree, Arc::clone(&events) as _).expect("valid tree");
    (demo, events)
}

async fn call(demo: &Demo, command: &str, args: Value) -> Result<Value, Value> {
    demo.invoke(command, args).await
}

async fn open(demo: &Demo, events: &Collector, id: &str, path: &str) {
    call(
        demo,
        "list_directory_start_streaming",
        json!({
            "volumeId": "root", "path": path, "includeHidden": false,
            "sortBy": "name", "sortOrder": "ascending", "listingId": id,
        }),
    )
    .await
    .expect("the listing starts");
    for _ in 0..500 {
        if events.completed(id) {
            return;
        }
        // allowed-test-sleep: polls for the engine's own background task, with a hard cap.
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("listing {id} never completed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_count_over_folders_the_fold_sized_plans_nothing_and_ends_clean() {
    let (demo, events) = demo();
    open(&demo, &events, "pane-1", "/Users/you").await;
    let whole = call(
        &demo,
        "count_folder_sizes",
        json!({ "listingId": "pane-1", "includeHidden": false, "paths": null }),
    )
    .await
    .expect("the count runs");
    assert_eq!(whole, json!({ "counted": 0, "unreadable": 0, "cancelled": false }));
    let one = call(
        &demo,
        "count_folder_sizes",
        json!({ "listingId": "pane-1", "includeHidden": false, "paths": ["/Users/you/Documents"] }),
    )
    .await
    .expect("the count runs");
    assert_eq!(one["counted"], 0);
    // The rows still show the fold's sizes.
    let rows = call(
        &demo,
        "get_file_range",
        json!({ "listingId": "pane-1", "start": 0, "count": 10, "includeHidden": false }),
    )
    .await
    .expect("rows");
    let documents = rows
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["name"] == "Documents")
        .expect("Documents is listed");
    assert_eq!(documents["recursiveSize"], 30);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_count_for_a_listing_that_moved_on_is_gone() {
    let (demo, _events) = demo();
    let refused = call(
        &demo,
        "count_folder_sizes",
        json!({ "listingId": "nope", "includeHidden": false, "paths": null }),
    )
    .await
    .expect_err("no such listing");
    assert_eq!(refused, json!({ "type": "gone", "listingId": "nope" }));
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_runs_for_a_cancel_to_stop() {
    let (demo, _events) = demo();
    assert_eq!(
        call(&demo, "cancel_folder_size_count", json!({ "listingId": "pane-1" }))
            .await
            .expect("answers"),
        json!(false)
    );
}

#[test]
fn the_outcome_crosses_with_the_apps_keys() {
    let keys = crate::events_test::ts_keys("FolderSizeCountOutcome");
    assert_eq!(
        keys,
        ["cancelled", "counted", "unreadable"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}
