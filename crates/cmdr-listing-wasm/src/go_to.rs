//! "Go to path" (⌘G / Ctrl+G) over the demo's volume: the desktop's own rules
//! (`cmdr_fs::go_to_path`, which `apps/desktop/src-tauri/src/go_to_path/` runs over
//! the local disk), plus the dialog's recent paths, in memory by `cmdr-recents`'
//! rules, as `commands/go_to_path.rs` and `go_to_path/history.rs` answer them.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor};
use cmdr_fs::go_to_path::{FoundKind, GoToHost, resolve as resolve_go_to};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::tilde::expand_tilde_with;
use cmdr_fs::typed_path::TypedPathHost;
use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};
use cmdr_recents::RecentItem;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::invoke::{ok, parse};

/// Every command [`DemoGoTo::invoke`] answers, by its Tauri name.
pub const GO_TO_COMMANDS: &[&str] = &[
    "resolve_go_to_path",
    "get_recent_paths",
    "add_recent_path",
    "remove_recent_path",
    "clear_recent_paths",
];

/// The dialog's cap, the desktop's `go_to_path::history::MAX_RECENTS`: digit keys 1–9 and 0.
const MAX_RECENTS: usize = 10;

/// The go-to dialog's backend over one in-memory volume.
pub struct DemoGoTo {
    store: Arc<InMemoryVolume>,
    home: Option<String>,
    recents: Mutex<Vec<RecentPathEntry>>,
    minted_ids: AtomicU64,
}

impl DemoGoTo {
    /// The dialog over `store`, expanding `~` to `home`.
    pub fn new(store: Arc<InMemoryVolume>, home: Option<String>) -> Self {
        Self {
            store,
            home,
            recents: Mutex::new(Vec::new()),
            minted_ids: AtomicU64::new(0),
        }
    }

    /// Whether [`Self::invoke`] answers `command`.
    pub fn handles(command: &str) -> bool {
        GO_TO_COMMANDS.contains(&command)
    }

    /// Runs `command` with Tauri-style `args` (camelCase keys).
    pub async fn invoke(&self, command: &str, args: Value) -> Result<Value, Value> {
        match command {
            "resolve_go_to_path" => {
                let a: ResolveArgs = parse(command, args)?;
                ok(self.resolve(&a.input, &a.base_dir).await)
            }
            "get_recent_paths" => ok(&*self.recents.lock_ignore_poison()),
            "add_recent_path" => {
                let a: AddArgs = parse(command, args)?;
                cmdr_recents::add_to(&mut self.recents.lock_ignore_poison(), a.entry, MAX_RECENTS, || {
                    format!("demo-{}", self.minted_ids.fetch_add(1, Ordering::Relaxed))
                });
                Ok(Value::Null)
            }
            "remove_recent_path" => {
                let a: IdArgs = parse(command, args)?;
                cmdr_recents::remove_from(&mut self.recents.lock_ignore_poison(), &a.id);
                Ok(Value::Null)
            }
            "clear_recent_paths" => {
                self.recents.lock_ignore_poison().clear();
                Ok(Value::Null)
            }
            other => Err(Value::String(format!("the browser demo doesn't answer `{other}`"))),
        }
    }

    /// The desktop's resolution over the volume. A pane folder that's no app path reads
    /// a relative input as nothing to go to, as the desktop does.
    async fn resolve(&self, input: &str, base_dir: &str) -> cmdr_fs::go_to_path::GoToPathResolution {
        let base = AppPathBuf::parse(base_dir, PathFlavor::Posix).ok();
        resolve_go_to(input, base.as_deref(), self).await
    }
}

impl TypedPathHost for DemoGoTo {
    fn flavor(&self) -> PathFlavor {
        PathFlavor::Posix
    }

    fn expand_tilde(&self, text: &str) -> String {
        expand_tilde_with(text, self.home.as_deref(), PathFlavor::Posix)
    }

    /// The in-memory volume's own answer: its default, the lexical parent.
    async fn parent_for_dot_dot(&self, path: &AppPath) -> Result<AppPathBuf, VolumeError> {
        self.store.parent_for_dot_dot(path).await
    }
}

impl GoToHost for DemoGoTo {
    /// What's at `path`, following links as the desktop's `metadata` does. An in-memory
    /// volume always answers, so this is never "couldn't check".
    async fn found(&self, path: &AppPath) -> Result<Option<FoundKind>, VolumeError> {
        let Some(target) = self.store.canonical_app_path(path) else {
            return Ok(None);
        };
        Ok(self.store.get_metadata(&target).await.ok().map(|entry| {
            if entry.is_directory {
                FoundKind::Directory
            } else {
                FoundKind::File
            }
        }))
    }
}

/// `RecentPathEntry` in the app (`go_to_path/history.rs`, which lives in the app crate).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentPathEntry {
    pub(crate) id: String,
    /// Unix epoch milliseconds.
    pub(crate) timestamp: i64,
    /// The resolved target the dialog jumped to.
    pub(crate) path: String,
}

impl RecentItem for RecentPathEntry {
    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }

    fn dedupe_key(&self) -> String {
        self.path.clone()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveArgs {
    input: String,
    /// The pane's folder, as an app path.
    base_dir: String,
}

#[derive(Deserialize)]
struct AddArgs {
    entry: RecentPathEntry,
}

#[derive(Deserialize)]
struct IdArgs {
    id: String,
}
