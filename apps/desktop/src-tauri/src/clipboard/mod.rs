//! Clipboard file operations for copy/cut/paste workflows.
//!
//! macOS exposes three free functions backed by NSPasteboard (`pasteboard`)
//! in prod and by an in-process store (`mock`) under the `playwright-e2e`
//! Cargo feature. Both impls share the same `store` module so a runtime
//! `CMDR_CLIPBOARD_BACKEND=mock` env opt-out in the prod path delegates to
//! the same data the mock module reads. See `CLAUDE.md` for the rationale.

#[cfg(target_os = "macos")]
mod store;

/// Payload picking, markdown sniffing, and content mapping. Compiled for both
/// the prod and E2E backends (both feed it an already-read `ClipboardData`).
#[cfg(target_os = "macos")]
mod payload;

#[cfg(all(target_os = "macos", not(feature = "playwright-e2e")))]
mod pasteboard;

#[cfg(all(target_os = "macos", feature = "playwright-e2e"))]
mod mock;

// How each OS reaches its clipboard: the pasteboard reads and writes the commands go through.
pub(crate) mod platform;
mod state;

// A paste-as-move of files the clipboard marks cut, followed to the move's end.
pub(crate) mod cut_paste;

pub use state::{clear_cut_state, get_cut_state, set_cut_state};

/// Whether files go on the clipboard to be copied or moved on paste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardIntent {
    /// Ctrl+C / ⌘C: a paste copies them.
    Copy,
    /// Ctrl+X / ⌘X: a paste moves them. Windows marks the clipboard so (`Preferred DropEffect`);
    /// macOS keeps the mark in Cmdr (`state.rs`), since Finder's pasteboard has none.
    Cut,
}

/// The files on the clipboard, as native paths, and whether the CLIPBOARD marks them cut (an
/// Explorer Ctrl+X). Only Windows has such a mark; macOS and Linux always answer `false`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClipboardFiles {
    pub(crate) paths: Vec<std::path::PathBuf>,
    pub(crate) marked_cut: bool,
}

/// Why files couldn't go on, or come off, the clipboard. The frontend words each from the
/// catalog; `detail` is for the log only.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ClipboardFilesError {
    /// Nothing to put on it: no rows, or a row with no path on this computer (a phone's, a
    /// server's), which refuses the whole set.
    NothingToCopy,
    /// A path whose plain form names a different file or none (Windows: past 259 characters, a
    /// name ending in a dot or space, a device name like `CON`). Other apps read `CF_HDROP` as
    /// plain paths, so the whole set stays off the clipboard.
    NoPlainPath {
        /// The path, for the toast to name.
        path: String,
    },
    /// Another app held the clipboard open through every retry.
    #[allow(
        dead_code,
        reason = "made by Windows' arm only: its clipboard is a lock another app holds"
    )]
    Busy,
    /// Files on the clipboard aren't supported on this OS yet.
    #[allow(
        dead_code,
        reason = "made by Linux's arm only, the one OS with no file clipboard yet"
    )]
    NotSupported,
    /// The OS refused for a reason it named.
    Unexpected {
        /// The OS's words, for the log.
        detail: String,
    },
}

/// The kind of clipboard content pasted as a file. Drives the paste toast's
/// noun (text / image / PDF); the filename's extension carries the finer detail
/// (`.md` / `.txt` / `.png` / `.jpg` / `.pdf`). Ungated so the Linux command
/// stub can name it in its signature.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "constructed only by the macOS paste path; Linux keeps the type for the stub command's wire signature"
    )
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PastedKind {
    Text,
    Image,
    Pdf,
}

/// Result of pasting clipboard content as a file: the created file's name and
/// its content kind. At the command boundary, `Option<PastedClipboardFile>`'s
/// `None` is the typed "nothing pasteable" no-op (not an error).
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PastedClipboardFile {
    pub name: String,
    pub kind: PastedKind,
}

/// What `paste_clipboard_as_file` answers within its reply deadline. A refusal
/// inside the deadline is the command's `Err(MutationError)`. Same contract as
/// `MutationReply` (`crates/cmdr-operations/src/mutation_reply.rs`), with the created
/// file riding along.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "Linux keeps the reply type for its unsupported clipboard command's wire signature"
    )
)]
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PasteClipboardReply {
    /// It ended in time: the file it created, or `None` for nothing pasteable.
    Done {
        /// The created file.
        file: Option<PastedClipboardFile>,
    },
    /// The write is still running; a [`ClipboardPasteSettled`] with this id follows.
    StillRunning {
        /// Names this one paste on the settle event.
        pending_id: String,
    },
}

/// `clipboard-paste-settled`: how a paste that answered `StillRunning` ended.
/// Broadcast; the waiting caller picks its own by `pending_id`.
#[derive(Clone, Debug, serde::Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardPasteSettled {
    /// The id the `StillRunning` reply carried.
    pub pending_id: String,
    /// How it ended.
    pub outcome: ClipboardPasteOutcome,
}

/// How a paste that outlived its deadline ended.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "the clipboard settle event is shared across platforms but emitted only on macOS"
    )
)]
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ClipboardPasteOutcome {
    /// It landed as `file`.
    Landed {
        /// The created file.
        file: Option<PastedClipboardFile>,
    },
    /// It didn't, for this reason: the same refusal an in-time reply carries.
    Refused {
        /// Why.
        error: cmdr_operations::MutationError,
    },
}

#[cfg(target_os = "macos")]
pub use payload::{ClipboardPayload, payload_to_content, pick_clipboard_payload};
/// The raw flavors `pick_clipboard_payload` reads; a drop of bare image bytes builds
/// one too (`drop_in/macos.rs`).
#[cfg(target_os = "macos")]
pub(crate) use store::ClipboardData;

// Paste clipboard content as a file: the payload goes through the engine's numbered-create instant op.
#[cfg(target_os = "macos")]
mod paste_file;
#[cfg(target_os = "macos")]
pub(crate) use paste_file::write_payload_replying;

#[cfg(all(target_os = "macos", not(feature = "playwright-e2e")))]
pub use pasteboard::{
    read_file_urls_from_clipboard, read_pasteboard_data, read_text_from_clipboard, write_file_urls_to_clipboard,
};

#[cfg(all(target_os = "macos", feature = "playwright-e2e"))]
pub use mock::{
    read_file_urls_from_clipboard, read_pasteboard_data, read_text_from_clipboard, write_file_urls_to_clipboard,
};

/// E2E-only admin surface: returns a snapshot of the in-process clipboard
/// store. Used by Playwright specs to assert clipboard contents without
/// reading back through IPC (and without touching the real pasteboard).
#[cfg(all(target_os = "macos", feature = "playwright-e2e"))]
#[allow(
    dead_code,
    reason = "Exported for future Playwright specs that assert clipboard contents directly."
)]
pub fn snapshot_mock_clipboard() -> Option<store::ClipboardEntry> {
    store::snapshot()
}

/// E2E-only admin surface: clears the in-process clipboard store.
#[cfg(all(target_os = "macos", feature = "playwright-e2e"))]
#[allow(
    dead_code,
    reason = "Exported for future Playwright specs that reset clipboard state between tests."
)]
pub fn clear_mock_clipboard() {
    store::clear();
}
