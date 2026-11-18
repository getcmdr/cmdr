//! A shortcut as a page menu prints it beside a row, and as `aria-keyshortcuts` spells it.
//!
//! Two inputs, as the native bar has two: a REGISTERED accelerator, in Tauri's spelling
//! (`Cmd+Shift+P`, `Ctrl+Up`), which the OS would print, so the page prints it the way that
//! platform does: macOS's glyphs in AppKit's order ([`glyphs`]), and elsewhere the frontend's own
//! `Ctrl+Alt+Shift+Super` order, with `Win` for the Meta key on Windows (`key-capture.ts`'s
//! `formatChord` and the style guide's platform vocabulary), so a row reads like the palette and
//! the shortcuts window. And a DISPLAY-only shortcut, already in the frontend's spelling for that
//! platform (`⇧8`, `Alt+Shift+=`, `F3`), which is printed as it is.
//!
//! `aria-keyshortcuts` follows WAI-ARIA 1.2: modifiers `Control`, `Alt`, `Shift`, `Meta`, then one
//! key named by its UI Events `key` value (`ArrowUp`, `Enter`), with the two exceptions ARIA sets
//! (`Space` for the space bar, `Plus` for `+`, which is the delimiter). `None` when the shortcut
//! names no key.

use crate::accelerators::{glyphs, to_menu_text};
use crate::spec::Platform;

/// What a row shows for its shortcut, and how a screen reader is told about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyHint {
    /// What the row prints beside its label.
    pub shown: String,
    /// The `aria-keyshortcuts` value, or `None` when the shortcut names no key.
    pub aria: Option<String>,
}

impl KeyHint {
    /// A shortcut the row registers, in Tauri's spelling.
    pub fn registered(accelerator: &str, platform: Platform) -> Self {
        Self {
            shown: shown_text(accelerator, platform),
            aria: aria_keyshortcuts(accelerator),
        }
    }

    /// A shortcut the row only shows, in the frontend's spelling for `platform`, printed as it is.
    pub fn displayed(shortcut: &str, platform: Platform) -> Self {
        Self {
            shown: shortcut.to_string(),
            aria: to_menu_text(shortcut, platform).and_then(|accelerator| aria_keyshortcuts(&accelerator)),
        }
    }
}

/// The four modifiers a chord can hold, in Tauri's vocabulary.
#[derive(Debug, Default, Clone, Copy)]
struct Held {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

/// A Tauri accelerator split into its modifiers and its key token (`Cmd++`'s key is `+`).
fn split(accelerator: &str) -> (Held, &str) {
    let mut held = Held::default();
    let mut key = "";
    let tokens: Vec<&str> = accelerator.split('+').collect();
    for (position, token) in tokens.iter().enumerate() {
        let is_last = position + 1 == tokens.len();
        match token.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" if !is_last => held.ctrl = true,
            "ALT" | "OPTION" if !is_last => held.alt = true,
            "SHIFT" if !is_last => held.shift = true,
            "CMD" | "COMMAND" | "SUPER" | "META" if !is_last => held.meta = true,
            _ => key = token,
        }
    }
    if key.is_empty() && accelerator.ends_with('+') {
        key = "+";
    }
    (held, key)
}

/// A registered accelerator as `platform` prints it.
fn shown_text(accelerator: &str, platform: Platform) -> String {
    if platform == Platform::MacOs {
        return glyphs(accelerator);
    }
    let (held, key) = split(accelerator);
    let meta = match platform {
        Platform::Windows => "Win",
        Platform::MacOs | Platform::Linux => "Super",
    };
    let mut parts: Vec<&str> = [
        (held.ctrl, "Ctrl"),
        (held.alt, "Alt"),
        (held.shift, "Shift"),
        (held.meta, meta),
    ]
    .into_iter()
    .filter_map(|(on, token)| on.then_some(token))
    .collect();
    let key = frontend_key_name(key);
    parts.push(&key);
    parts.join("+")
}

/// A Tauri key token in the frontend's canonical spelling (`normalizeKeyName`): arrows as arrows,
/// punctuation as the character, letters upper-case, named keys as they are.
fn frontend_key_name(key: &str) -> String {
    match key.to_ascii_uppercase().as_str() {
        "UP" => "↑".to_string(),
        "DOWN" => "↓".to_string(),
        "LEFT" => "←".to_string(),
        "RIGHT" => "→".to_string(),
        "EQUAL" => "=".to_string(),
        "MINUS" => "-".to_string(),
        "PLUS" => "+".to_string(),
        "RETURN" => "Enter".to_string(),
        "ESC" => "Escape".to_string(),
        _ if key.chars().count() == 1 => key.to_uppercase(),
        _ => key.to_string(),
    }
}

/// A Tauri accelerator in `aria-keyshortcuts` spelling, or `None` when it names no key.
pub fn aria_keyshortcuts(accelerator: &str) -> Option<String> {
    let (held, key) = split(accelerator);
    if key.is_empty() {
        return None;
    }
    let key = match key.to_ascii_uppercase().as_str() {
        "UP" => "ArrowUp".to_string(),
        "DOWN" => "ArrowDown".to_string(),
        "LEFT" => "ArrowLeft".to_string(),
        "RIGHT" => "ArrowRight".to_string(),
        "ENTER" | "RETURN" => "Enter".to_string(),
        "SPACE" => "Space".to_string(),
        "TAB" => "Tab".to_string(),
        "ESCAPE" | "ESC" => "Escape".to_string(),
        "BACKSPACE" => "Backspace".to_string(),
        "DELETE" => "Delete".to_string(),
        "INSERT" => "Insert".to_string(),
        "PAGEUP" => "PageUp".to_string(),
        "PAGEDOWN" => "PageDown".to_string(),
        "HOME" => "Home".to_string(),
        "END" => "End".to_string(),
        "EQUAL" => "=".to_string(),
        "MINUS" => "-".to_string(),
        "PLUS" | "+" => "Plus".to_string(),
        _ if key.chars().count() == 1 => key.to_uppercase(),
        // F-keys and any other named key keep their name, upper-cased the way UI Events spells
        // an F-key (`F5`).
        _ => key.to_string(),
    };
    let mut parts: Vec<String> = [
        (held.ctrl, "Control"),
        (held.alt, "Alt"),
        (held.shift, "Shift"),
        (held.meta, "Meta"),
    ]
    .into_iter()
    .filter(|&(on, _token)| on)
    .map(|(_on, token)| token.to_string())
    .collect();
    parts.push(key);
    Some(parts.join("+"))
}
