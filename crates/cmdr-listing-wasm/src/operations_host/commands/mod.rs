//! The write, queue, and undo commands, answered the way the desktop's Tauri
//! commands answer them: the same names, the same camelCase arguments, the same
//! JSON back, through the same engine calls. One file per domain, as the
//! desktop's `commands/` splits them.
//!
//! Where the desktop's command does host work the page has none of (tilde
//! expansion, S3 cost pricing, the OS trash), the demo answers what a volume
//! without it would; `DETAILS.md` § "Where the demo differs from the app".

mod history;
mod instant;
mod paths;
mod queue;
mod transfers;

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::error_detail::ErrorDetail;
use serde::Deserialize;
use serde_json::Value;

use super::DemoOperations;

impl DemoOperations {
    /// Wire text as the app path a write start takes, read as the desktop's command edge
    /// reads it: `~` and `~/rest` name the visitor's home folder (`expand_tilde`), then
    /// `invoke::demo_path`: POSIX on every host, as the demo's volume is (`from_wire` reads only
    /// drives on a Windows test host); a trailing `/` goes, a `..` is refused. Text the page
    /// can't have sent (relative, empty, a `..`) is refused as the desktop refuses it: the
    /// generic I/O failure, its details naming the path (`commands/wire_path.rs`).
    fn wire_path(&self, text: &str) -> Result<AppPathBuf, cmdr_operations::WriteOperationError> {
        let expanded = self.expand_home(text);
        crate::invoke::demo_path(&expanded).map_err(|why| cmdr_operations::WriteOperationError::IoError {
            message: refused_detail(text, &why),
            path: text.to_string(),
        })
    }

    /// [`Self::wire_path`] over a start's sources, refusing the set on the first refusal.
    fn wire_paths(&self, texts: &[String]) -> Result<Vec<AppPathBuf>, cmdr_operations::WriteOperationError> {
        texts.iter().map(|text| self.wire_path(text)).collect()
    }

    /// [`Self::wire_path`] for an instant mutation (a rename): the one "something went
    /// wrong" fallback, details as above.
    fn wire_mutation_path(&self, text: &str) -> Result<AppPathBuf, cmdr_operations::MutationError> {
        let expanded = self.expand_home(text);
        crate::invoke::demo_path(&expanded).map_err(|why| cmdr_operations::MutationError::Unexpected {
            detail: refused_detail(text, &why),
        })
    }

    /// `~` and `~/rest` as the home folder and a path under it; anything else as sent.
    fn expand_home<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        match (&self.home, text.strip_prefix('~')) {
            (Some(home), Some("")) => home.clone().into(),
            (Some(home), Some(rest)) if rest.starts_with('/') => format!("{home}{rest}").into(),
            _ => text.into(),
        }
    }
}

/// The technical-details line of a refused wire path: the path, then why.
fn refused_detail(text: &str, why: &cmdr_fs::app_path::NotAnAppPath) -> ErrorDetail {
    ErrorDetail::path_then(text.to_string(), format!(": {why}, which no command acts on"))
}

/// The arguments of every command that names one operation.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationIdArgs {
    operation_id: String,
}

/// The arguments of every command that names several.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationIdsArgs {
    operation_ids: Vec<String>,
}

/// Every command group, by its Tauri names.
const GROUPS: &[&[&str]] = &[
    transfers::NAMES,
    instant::NAMES,
    queue::NAMES,
    history::NAMES,
    paths::NAMES,
];

/// Whether [`DemoOperations::invoke`] answers `command`.
pub(super) fn handles(command: &str) -> bool {
    GROUPS.iter().any(|names| names.contains(&command))
}

/// Every command [`DemoOperations::invoke`] answers.
pub(super) fn names() -> impl Iterator<Item = &'static str> {
    GROUPS.iter().flat_map(|names| names.iter().copied())
}

impl DemoOperations {
    /// Runs `command` with Tauri-style `args` (camelCase keys): `Ok` is the
    /// command's answer, `Err` what the promise rejects with.
    pub async fn invoke(&self, command: &str, args: Value) -> Result<Value, Value> {
        if transfers::NAMES.contains(&command) {
            self.transfers(command, args).await
        } else if instant::NAMES.contains(&command) {
            self.instant(command, args).await
        } else if queue::NAMES.contains(&command) {
            self.queue(command, args).await
        } else if paths::NAMES.contains(&command) {
            self.paths(command, args).await
        } else if history::NAMES.contains(&command) {
            self.history(command, args).await
        } else {
            Err(Value::String(format!("the demo doesn't answer `{command}`")))
        }
    }
}
