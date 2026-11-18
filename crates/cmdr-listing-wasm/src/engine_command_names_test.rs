//! The drift guard for the demo's copy of what the engines answer
//! (`apps/desktop/src/lib/demo/engine-command-names.ts`): the demo's command
//! coverage is checked against that copy at type check, so a command added here
//! and missing there would show as unanswered, and one dropped here but kept
//! there would hide a real gap.

use std::collections::BTreeSet;

use crate::commands::Demo;
use crate::context_menu::POPUP_COMMANDS;
use crate::menu::MENU_COMMANDS;

/// Read at test time: `rustEmbeddedInputs` in `scripts/check/checks/inputs.go` fingerprints it.
fn ts_list(name: &str) -> BTreeSet<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/src/lib/demo/engine-command-names.ts"
    );
    let text = std::fs::read_to_string(path).expect("the demo's engine command list is readable");
    let start = text
        .find(&format!("export const {name} = ["))
        .unwrap_or_else(|| panic!("`engine-command-names.ts` has no `{name}`"));
    let end = text[start..].find("] as const").expect("the list closes") + start;
    text[start..end]
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| line.starts_with('\''))
        .map(|line| line.trim_end_matches(',').trim_matches('\'').to_string())
        .collect()
}

fn diff(rust: &BTreeSet<String>, ts: &BTreeSet<String>) -> String {
    let missing: Vec<_> = rust.difference(ts).collect();
    let extra: Vec<_> = ts.difference(rust).collect();
    format!("missing from the TS list: {missing:?}; in the TS list but not answered here: {extra:?}")
}

#[test]
fn the_demo_knows_every_command_the_listing_engine_answers() {
    let rust: BTreeSet<String> = Demo::commands().map(str::to_string).collect();
    let ts = ts_list("LISTING_ENGINE_COMMANDS");
    assert_eq!(rust, ts, "{}", diff(&rust, &ts));
}

#[test]
fn the_demo_knows_every_command_the_menu_engine_answers() {
    let rust: BTreeSet<String> = MENU_COMMANDS
        .iter()
        .chain(POPUP_COMMANDS)
        .map(|name| (*name).to_string())
        .collect();
    let ts = ts_list("MENU_ENGINE_COMMANDS");
    assert_eq!(rust, ts, "{}", diff(&rust, &ts));
}
