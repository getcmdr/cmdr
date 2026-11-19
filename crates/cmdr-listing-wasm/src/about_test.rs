//! "Tell me about this" over the demo's volume: the card's messages, in the
//! desktop host's order, on the channel the page made, and the wire pinned
//! against `bindings.ts`.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::about::{ABOUT_COMMANDS, CHANNEL_EVENT};
use crate::commands::Demo;
use crate::events::EventSink;
use crate::events_test::{BINDINGS, ts_keys};
use crate::tree::DemoTree;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89";
const CHANNEL: &str = "__CHANNEL__:main:7";

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
    /// Every channel notice, in order.
    fn channel(&self) -> Vec<Value> {
        self.0
            .lock()
            .expect("collector lock")
            .iter()
            .filter(|(name, _)| name == CHANNEL_EVENT)
            .map(|(_, payload)| payload.clone())
            .collect()
    }
}

struct Fixture {
    demo: Demo,
    events: Arc<Collector>,
    runtime: tokio::runtime::Runtime,
}

impl Fixture {
    fn call(&self, command: &str, args: Value) -> Value {
        self.runtime
            .block_on(self.demo.invoke(command, args))
            .unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn start(&self, request_id: &str, path: &str) -> Vec<Value> {
        let answer = self.call(
            "about_start",
            json!({
                "request": { "requestId": request_id, "path": path, "volumeId": "root", "isDirectory": false },
                "onEvent": CHANNEL,
            }),
        );
        assert_eq!(answer, Value::Null, "the card's answers go on its channel");
        self.events.channel()
    }
}

fn fixture() -> Fixture {
    let mut tree: DemoTree = serde_json::from_value(json!({
        "volumeId": "root",
        "volumeName": "Macintosh HD",
        "root": "/",
        "entries": [
            { "path": "/docs", "kind": "dir" },
            { "path": "/docs/dot.png", "kind": "file", "modifiedMs": 1_700_000_000_000_u64 },
            { "path": "/docs/big.bin", "kind": "file", "size": 200_000 },
        ],
    }))
    .expect("tree parses");
    tree.contents = HashMap::from([("/docs/dot.png".to_string(), PNG.to_vec())]);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let events = Arc::new(Collector::default());
    let demo = {
        let _entered = runtime.enter();
        Demo::new(tree, Arc::clone(&events) as _).expect("the demo builds")
    };
    Fixture { demo, events, runtime }
}

/// Each message's update type, in order, with the channel's end as `"end"`.
fn kinds(notices: &[Value]) -> Vec<String> {
    notices
        .iter()
        .map(|notice| {
            assert_eq!(
                notice["channel"], CHANNEL,
                "every message goes to the card's own channel"
            );
            if notice["end"] == true {
                "end".to_string()
            } else {
                notice["message"]["update"]["type"]
                    .as_str()
                    .expect("an update type")
                    .to_string()
            }
        })
        .collect()
}

#[test]
fn the_engine_answers_every_about_command() {
    for command in ABOUT_COMMANDS {
        assert!(Demo::handles(command), "{command}");
        assert!(Demo::commands().any(|c| c == *command), "{command}");
    }
}

#[test]
fn a_file_card_plans_its_identity_and_real_type_then_settles_with_no_verdict() {
    let f = fixture();
    let notices = f.start("card-1", "/docs/dot.png");
    assert_eq!(
        kinds(&notices),
        ["planned", "verdictUnavailable", "fact", "fact", "factsSettled", "end"]
    );
    for notice in &notices[..notices.len() - 1] {
        assert_eq!(notice["message"]["requestId"], "card-1");
    }

    let planned = &notices[0]["message"]["update"];
    assert_eq!(planned["plan"]["target"], "file");
    let rows: Vec<&Value> = planned["plan"]["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .flat_map(|section| section["kinds"].as_array().expect("kinds"))
        .collect();
    assert_eq!(
        rows,
        [&json!("identity"), &json!("realType")],
        "only the rows a page can answer"
    );
    // Cmdr sees only some of it here: no OS records, index, or repo.
    assert_eq!(planned["plan"]["volumeNote"]["limit"], "someFacts");
    assert_eq!(planned["plan"]["volumeNote"]["volume"], "Macintosh HD");
    assert!(planned["derived"]["headline"].is_object());

    assert_eq!(notices[1]["message"]["update"]["why"], json!({ "kind": "aiOff" }));

    let identity = &notices[2]["message"]["update"]["fact"];
    assert_eq!(identity["kind"], "identity");
    assert_eq!(identity["fact"]["value"]["name"], "dot.png");
    assert_eq!(identity["fact"]["value"]["logicalSize"], PNG.len());

    let real_type = &notices[3]["message"]["update"]["fact"];
    assert_eq!(real_type["kind"], "realType");
    assert_eq!(real_type["fact"]["value"]["agreement"], "matches", "{real_type}");
}

#[test]
fn a_file_of_generated_bytes_cant_tell_its_kind_and_a_folder_has_only_its_identity() {
    let f = fixture();
    // Past the head, so the tail's read runs too. The demo's filler bytes match no
    // format, which the card says as "can't tell", never as a mismatch.
    let big = f.start("card-big", "/docs/big.bin");
    assert_eq!(
        kinds(&big),
        ["planned", "verdictUnavailable", "fact", "fact", "factsSettled", "end"]
    );
    let real_type = &big[3]["message"]["update"]["fact"]["fact"]["value"];
    assert_eq!(real_type["sniffed"]["value"]["kind"], "unrecognized");
    assert_eq!(real_type["agreement"], "cantTell");

    let f = fixture();
    let folder = f.start("card-dir", "/docs");
    assert_eq!(
        kinds(&folder),
        ["planned", "verdictUnavailable", "fact", "factsSettled", "end"]
    );
    assert_eq!(folder[0]["message"]["update"]["plan"]["target"], "folder");
}

#[test]
fn a_missing_item_gets_unavailable_alone() {
    let f = fixture();
    let notices = f.start("card-gone", "/docs/nope.txt");
    assert_eq!(kinds(&notices), ["unavailable", "end"]);
    assert_eq!(notices[0]["message"]["update"]["why"], "notFound");
}

#[test]
fn a_cancel_before_the_start_leaves_the_card_silent() {
    let f = fixture();
    f.call("about_cancel", json!({ "requestId": "card-early" }));
    let notices = f.start("card-early", "/docs/dot.png");
    assert_eq!(kinds(&notices), ["end"], "the start after its cancel sends nothing");
    // The tombstone is spent: the same id starts again.
    let again = f.start("card-early", "/docs/dot.png");
    assert_eq!(kinds(&again).first().map(String::as_str), Some("end"));
    assert!(kinds(&again).contains(&"planned".to_string()));
    assert_eq!(
        f.call("about_stop_verdict", json!({ "requestId": "card-early" })),
        Value::Null
    );
}

/// The `type` (or `kind`) tags of one `export type <name> =` union in `bindings.ts`.
fn ts_union_tags(name: &str, tag: &str) -> BTreeSet<String> {
    let header = format!("export type {name} =");
    let start = BINDINGS.find(&header).unwrap_or_else(|| panic!("no `{name}`")) + header.len();
    let end = BINDINGS[start..]
        .find("\nexport ")
        .map_or(BINDINGS.len(), |end| end + start);
    let needle = format!("{tag}: '");
    BINDINGS[start..end]
        .match_indices(&needle)
        .map(|(at, _)| {
            let rest = &BINDINGS[start + at + needle.len()..];
            rest[..rest.find('\'').expect("closing quote")].to_string()
        })
        .collect()
}

#[test]
fn the_wire_matches_the_desktops() {
    let f = fixture();
    let mut notices = f.start("card-wire", "/docs/dot.png");
    notices.extend(f.start("card-missing", "/nope"));
    let updates: Vec<&Value> = notices
        .iter()
        .filter(|notice| notice["end"] != true)
        .map(|notice| &notice["message"])
        .collect();
    let keys = |value: &Value| -> BTreeSet<String> { value.as_object().expect("an object").keys().cloned().collect() };
    let update_tags = ts_union_tags("AboutUpdate", "type");
    for message in &updates {
        assert_eq!(keys(message), ts_keys("AboutEvent"));
        let update = &message["update"];
        assert!(update_tags.contains(update["type"].as_str().expect("type")), "{update}");
        if let Some(derived) = update.get("derived") {
            assert_eq!(keys(derived), ts_keys("Derived"));
        }
    }
    let unavailable = updates
        .iter()
        .find(|message| message["update"]["type"] == "unavailable")
        .expect("the missing item's card");
    let whys = BINDINGS
        .split("export type AboutUnavailable =")
        .nth(1)
        .and_then(|rest| rest.split("\nexport ").next())
        .expect("`AboutUnavailable`");
    let why = unavailable["update"]["why"].as_str().expect("why");
    assert!(whys.contains(&format!("'{why}'")), "{why}");
    assert!(ts_union_tags("VerdictUnavailable", "kind").contains("aiOff"));
}
