//! The Selection dialog's recent selections (`commands/selection.rs` and
//! `selection/history.rs` in the app), in memory by `cmdr-recents`' rules, over the
//! same `cmdr_search::history` key parts the desktop's entry dedupes on. They last the
//! visit, as the demo's recent searches do.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_recents::RecentItem;
use cmdr_search::history::{HistoryFilters, HistoryMode, filters_fingerprint, flag, normalize_query};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::invoke::{ok, parse};

/// Every command [`SelectionRecents::invoke`] answers, by its Tauri name.
pub const SELECTION_RECENTS_COMMANDS: &[&str] = &[
    "get_recent_selections",
    "add_recent_selection",
    "remove_recent_selection",
    "clear_recent_selections",
    "apply_recent_selections_max_count",
];

/// The cap when the visitor hasn't set `selection.recentSelections.maxCount`: the
/// desktop's `selection::history::DEFAULT_MAX_COUNT`.
const DEFAULT_MAX_COUNT: usize = 1000;

/// The visit's recent selections.
#[derive(Default)]
pub struct SelectionRecents {
    entries: Mutex<Vec<SelectionHistoryEntry>>,
    minted_ids: AtomicU64,
}

impl SelectionRecents {
    /// Whether [`Self::invoke`] answers `command`.
    pub fn handles(command: &str) -> bool {
        SELECTION_RECENTS_COMMANDS.contains(&command)
    }

    /// Runs `command` with Tauri-style `args` (camelCase keys).
    pub fn invoke(&self, command: &str, args: Value) -> Result<Value, Value> {
        let mut entries = self.entries.lock_ignore_poison();
        match command {
            "get_recent_selections" => {
                let a: LimitArgs = parse(command, args)?;
                let limit = a.limit.map_or(entries.len(), |n| n as usize);
                ok(&entries[..limit.min(entries.len())])
            }
            "add_recent_selection" => {
                let a: AddArgs = parse(command, args)?;
                let cap = a.max_count.map_or(DEFAULT_MAX_COUNT, |n| n as usize);
                cmdr_recents::add_to(&mut entries, a.entry, cap, || {
                    format!("demo-{}", self.minted_ids.fetch_add(1, Ordering::Relaxed))
                });
                Ok(Value::Null)
            }
            "remove_recent_selection" => {
                let a: IdArgs = parse(command, args)?;
                cmdr_recents::remove_from(&mut entries, &a.id);
                Ok(Value::Null)
            }
            "clear_recent_selections" => {
                entries.clear();
                Ok(Value::Null)
            }
            "apply_recent_selections_max_count" => {
                let a: MaxCountArgs = parse(command, args)?;
                cmdr_recents::trim_to(&mut entries, a.max_count as usize);
                Ok(Value::Null)
            }
            other => Err(Value::String(format!("the browser demo doesn't answer `{other}`"))),
        }
    }
}

/// `SelectionHistoryEntry` in the app (`selection/history.rs`, which lives in the app crate).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionHistoryEntry {
    id: String,
    timestamp: i64,
    mode: HistoryMode,
    query: String,
    #[serde(default)]
    filters: HistoryFilters,
    case_sensitive: bool,
    match_count: u32,
}

impl RecentItem for SelectionHistoryEntry {
    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }

    /// The desktop's four segments, from the same `cmdr_search::history` parts
    /// (`apps/desktop/src-tauri/src/selection/history.rs`, the canonical): no scope, as
    /// Selection runs in the focused folder.
    fn dedupe_key(&self) -> String {
        let mode = self.mode.as_str();
        let query = normalize_query(&self.query);
        let filters = filters_fingerprint(&self.filters);
        let cs = flag(self.case_sensitive);
        format!("{mode}|{query}|{filters}|{cs}")
    }
}

#[derive(Deserialize)]
struct LimitArgs {
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddArgs {
    entry: SelectionHistoryEntry,
    max_count: Option<u32>,
}

#[derive(Deserialize)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaxCountArgs {
    max_count: u32,
}
