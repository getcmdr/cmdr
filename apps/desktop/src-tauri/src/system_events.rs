//! Cross-platform appearance/system event payloads.
//!
//! These events are emitted from per-OS code (the `system_look/platform/` arms and
//! macOS-only `intl/live_locale.rs`), but `collect_events!`
//! in `ipc.rs` can't `#[cfg]`-gate inline, so their typed payload structs live
//! here in an always-compiled module. The emit sites just build and `.emit()`
//! them. Same pattern the MTP and network modules use (structs in an
//! always-compiled module, emits behind `#[cfg]`).

use serde::{Deserialize, Serialize};
use tauri_specta::Event;

/// The OS accent color, as `get_accent_color` answers it and `accent-color-changed` carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemAccent {
    /// The accent itself as `#rrggbb`, or the brand gold when the OS doesn't say.
    pub hex: String,
    /// The shades the OS fills its own controls with, where it names them (Windows), else
    /// `None` and controls fill with `hex`.
    pub fill: Option<AccentFill>,
}

impl SystemAccent {
    /// An accent with no shades of its own: controls fill with `hex` (macOS, Linux).
    pub fn plain(hex: String) -> Self {
        Self { hex, fill: None }
    }
}

/// The accent shade a control fills with, per theme: WinUI's `AccentFillColorDefault`, which
/// is `SystemAccentColorDark1` in the light theme and `SystemAccentColorLight2` in the dark one,
/// each as `#rrggbb`. Both travel, so the frontend picks per `prefers-color-scheme` and a
/// light/dark switch needs no round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccentFill {
    pub light: String,
    pub dark: String,
}

/// `accent-color-changed`: the OS accent color (or light/dark appearance)
/// changed. `accent` is the fresh answer, exactly what `get_accent_color` returns now.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct AccentColorChanged {
    pub accent: SystemAccent,
}

/// The OS display options the UI mirrors as classes on `<html>`: macOS's
/// Accessibility > Display options, plus Windows' Underline access keys. Each is
/// `false` where its OS doesn't have it. Read by `get_display_accessibility`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DisplayAccessibility {
    /// Reduce transparency: translucent materials go opaque.
    pub reduce_transparency: bool,
    /// Increase contrast: borders, secondary text, and focus rings get stronger.
    pub increase_contrast: bool,
    /// Differentiate without color: state cues gain a shape or weight, not just a hue.
    pub differentiate_without_color: bool,
    /// Invert colors: the whole display is inverted. macOS draws file and folder icons
    /// differently under it, so the icon caches key off it.
    pub invert_colors: bool,
    /// Show borders: buttons and controls that draw no edge of their own gain one.
    pub show_borders: bool,
    /// Windows' Underline access keys (Settings › Accessibility › Keyboard,
    /// `SPI_GETKEYBOARDCUES`): access-key letters stay underlined all the time,
    /// not only once the keyboard is in use. `false` on macOS and Linux.
    pub underline_access_keys: bool,
}

/// `display-accessibility-changed`: one of the display options above changed.
/// `options` is the fresh full set, re-read on every change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DisplayAccessibilityChanged {
    pub options: DisplayAccessibility,
}

/// `glass-tint-changed`: the macOS 27 Appearance > Liquid Glass slider moved. `amount` is
/// the new value in `0.0..=1.0` (clearest to most tinted), or `None` when macOS no longer
/// reports one.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct GlassTintChanged {
    pub amount: Option<f32>,
}

/// `system-text-size-changed`:the macOS Accessibility > Display > Text Size
/// value changed. `multiplier` is the new system text-size multiplier (1.0 =
/// default).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SystemTextSizeChanged {
    pub multiplier: f32,
}

/// `os-locales-changed`: the user changed their macOS language or region while
/// the app was running, and one of the two answers actually moved. `locales` is
/// the fresh pair, exactly what `get_os_locales` would return now. Only emitted
/// when it differs from the pair the app is running on, so a listener can act on
/// every one it receives (`intl/live_locale.rs`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct OsLocalesChanged {
    pub locales: crate::intl::OsLocales,
}

/// `menu-bar-rebuilt`: the native menu bar was thrown away and built again in a
/// new language, so every item is a NEW object.
///
/// The rebuild restores what Rust knows (checked states, the view-mode pair), but
/// the things only the frontend knows are gone with the old items: the user's
/// custom accelerators, the pin/unpin label, and whether "Reopen closed tab" is
/// live. The listener in `lib/intl/menu-bar-sync.ts` pushes all three back.
/// Rare (a language change), so re-pushing everything beats tracking what moved.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MenuBarRebuilt {}

/// `drag-image-size`: the OS drag image's pixel dimensions, read on drag enter
/// (macOS swizzle in `drag_image_detection.rs`). Used to size / suppress the
/// DOM drag overlay.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DragImageSize {
    pub width: f64,
    pub height: f64,
}

/// `drag-modifiers`: the modifier-key state during a drag, emitted on drag
/// enter and whenever it changes (macOS swizzle in `drag_image_detection.rs`, the
/// Windows drop target in `commands/file_system/drag/platform/windows/drop_target.rs`).
/// Drives copy/move intent without keyboard focus. What each key means is the
/// platform's `dropModifiers` convention, read by the frontend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DragModifiers {
    pub alt_held: bool,
    pub cmd_held: bool,
    pub ctrl_held: bool,
    pub shift_held: bool,
}

/// `native-drag-drop`: a drag over a Windows webview, from Cmdr's own OLE drop target
/// (`commands/file_system/drag/platform/windows/drop_target.rs`), which stands in for
/// wry's. The frontend's drop door (`#lib/tauri-commands/native-paths.ts`) reads it where
/// macOS and Linux read Tauri's own drag-drop event. Its `paths` are already app paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Event)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NativeDragDrop {
    /// The drag came over the webview, with the files it carries (none for a drag of
    /// Cmdr's own that offers no files to other apps).
    Enter {
        paths: Vec<String>,
        /// Paths no app path names, left out of `paths`.
        refused: u32,
        position: DragPoint,
    },
    /// The drag moved, or a key changed.
    Over { position: DragPoint },
    /// The drag was dropped on the webview.
    Drop {
        paths: Vec<String>,
        refused: u32,
        position: DragPoint,
    },
    /// The drag left the webview, or was cancelled over it.
    Leave,
}

/// A point in the webview's client area, in PHYSICAL pixels, as Windows reports it; the
/// frontend divides by `devicePixelRatio` for CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DragPoint {
    pub x: f64,
    pub y: f64,
}

/// `drag-out-session-started`: the FE raises a signs-of-life in-progress toast
/// when the FIRST fulfillment of a drag-out-to-Finder session begins (macOS,
/// `native_drag/promises.rs`). `total_items` is the top-level dragged-item
/// count. The struct lives here (always compiled) because `native_drag` is
/// macOS-only and `collect_events!` can't `#[cfg]`-gate inline; the macOS emit
/// site builds + `.emit()`s it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "drag-out-session-started")]
pub struct SessionStartedEvent {
    /// The drag sequence key, so the FE can key its in-progress toast and
    /// replace it in place with the completion toast under the same id.
    pub session_key: i64,
    /// Top-level dragged items in this session.
    pub total_items: usize,
}

/// `drag-out-session-complete`: the session drained (gesture ended AND no
/// in-flight fulfillment). The FE replaces the in-progress toast with a
/// completion / failure toast keyed by the same `session_key`, plus the folded
/// per-item outcome counts. Always-compiled for the same reason as
/// `SessionStartedEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "drag-out-session-complete")]
pub struct SessionCompleteEvent {
    /// The drag sequence key (matches the started event's key).
    pub session_key: i64,
    /// Top-level files that landed successfully.
    pub files_succeeded: usize,
    /// Top-level folders that landed successfully.
    pub folders_succeeded: usize,
    /// Leaf names of items that failed (empty on full success).
    pub failures: Vec<String>,
}
