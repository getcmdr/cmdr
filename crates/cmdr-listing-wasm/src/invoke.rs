//! The Tauri-shaped `invoke` plumbing every command family shares: camelCase args into a
//! typed request, and an answer or a rejection back as JSON. A leaf, so `commands.rs`
//! (which dispatches to every family) and the families it dispatches to (`viewer.rs`,
//! `operations_host/commands/`) all lean on it without leaning back on `commands.rs`.

use cmdr_fs::app_path::{AppPathBuf, ESCAPES_LIVE, Namer, NotAnAppPath, PathFlavor};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// `args` as `T`, or the string Tauri rejects with when they don't parse.
pub(crate) fn parse<T: DeserializeOwned>(command: &str, args: Value) -> Result<T, Value> {
    serde_json::from_value(args).map_err(|e| Value::String(format!("invalid args for command `{command}`: {e}")))
}

/// A command's answer as JSON.
pub(crate) fn ok(answer: impl Serialize) -> Result<Value, Value> {
    serde_json::to_value(answer).map_err(|e| Value::String(e.to_string()))
}

/// A fallible command's result: its answer, or its serialized error as the rejection.
pub(crate) fn answer<T: Serialize, E: Serialize>(result: Result<T, E>) -> Result<Value, Value> {
    match result {
        Ok(answer) => ok(answer),
        Err(error) => Err(ok(error)?),
    }
}

/// `text`, a path the page or the volume spells (an arg, a `FileEntry.path`), as the app path it
/// names. The demo's volume is POSIX on every host, so it parses as POSIX even where the tests
/// run on Windows (where `from_wire` reads only drives). Normalized, as `Path`'s own compare read
/// it (`//`, `.`, a trailing `/`); a `..` or relative text is refused, which no pane spells.
/// ❗ The desktop's literal door, POSIX: while escapes aren't live (`ESCAPES_LIVE`) an
/// escape-range code point in the text is LITERAL (no producer made it), spelled through
/// this host's units as the listing door spells a listed name, ❌ never decoded as an escape
/// (the desktop's `app_text::parse` → `from_literal_store`).
pub(crate) fn demo_path(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if ESCAPES_LIVE {
        return AppPathBuf::parse(text, PathFlavor::Posix);
    }
    AppPathBuf::from_external_text(text, PathFlavor::Posix, Namer::HOST)
}

/// The runtime the engines' background work runs on (the viewer, the folder-size gather, Multi-Rename's media
/// reads, an operation log tree backfill): the page's event loop in the browser, the caller's tokio runtime in
/// native tests.
pub(crate) fn page_runtime() -> cmdr_fs::rt::Runtime {
    #[cfg(target_family = "wasm")]
    return cmdr_fs::rt::Runtime::default();
    #[cfg(not(target_family = "wasm"))]
    cmdr_fs::rt::Runtime::current()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A literal escape-range code point in the page's text stays the text it is while escapes
    /// aren't live: the same app path the volume's listing door spells for a file listed with
    /// that name, ❌ never the escape reading (which names the byte `E9`), as the desktop's
    /// literal door reads it.
    #[test]
    fn a_literal_escape_range_code_point_is_read_as_literal_text() {
        let text = "/docs/caf\u{10FEE9}.txt";
        let path = demo_path(text).expect("a POSIX path");
        if !ESCAPES_LIVE {
            assert_eq!(
                path,
                AppPathBuf::from_listed_text(text, PathFlavor::Posix).expect("listed")
            );
            assert_ne!(
                path,
                AppPathBuf::parse(text, PathFlavor::Posix).expect("an escape reading")
            );
        }
    }
}
