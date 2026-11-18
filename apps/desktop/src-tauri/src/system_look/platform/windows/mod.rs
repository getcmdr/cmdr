//! Windows: the display options carry Underline access keys (`keyboard_cues.rs`), live through
//! `WM_SETTINGCHANGE` (`setting_changes.rs`). The rest has no reader yet (`UISettings` for the
//! accent color and text scale, high contrast and transparency through `SystemParametersInfoW`:
//! task 40, which hangs its readers off the same observer), so those answer the default the other
//! platforms fall back to.

mod keyboard_cues;
mod setting_changes;

use tauri::AppHandle;

use crate::system_events::DisplayAccessibility;

/// Brand fallback accent (mustard gold from getcmdr.com).
const FALLBACK_ACCENT_HEX: &str = "#d4a006";

/// The brand fallback.
pub(crate) async fn accent_color(_app: AppHandle) -> String {
    FALLBACK_ACCENT_HEX.to_owned()
}

/// Underline access keys as Windows has it; every other option off.
pub(crate) async fn display_accessibility(_app: AppHandle) -> DisplayAccessibility {
    read_display_accessibility()
}

/// Every display option this arm reads, fresh. One `SystemParametersInfoW` call: cheap, and
/// callable from any thread.
fn read_display_accessibility() -> DisplayAccessibility {
    keyboard_cues::display_accessibility_from(keyboard_cues::underline_access_keys())
}

/// No slider.
pub(crate) async fn glass_tint_amount() -> Option<f32> {
    None
}

/// No system scaling.
pub(crate) fn text_size_multiplier() -> f32 {
    1.0
}

/// Follows `WM_SETTINGCHANGE`, emitting `display-accessibility-changed` when an option moved.
pub(crate) fn observe_changes(app: AppHandle) {
    setting_changes::observe(app);
}
