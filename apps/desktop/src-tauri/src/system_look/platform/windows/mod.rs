//! Windows: the accent color with WinUI's fill shades and Transparency effects (`UISettings`,
//! `ui_settings.rs`), contrast themes (`contrast_theme.rs`), and Underline access keys
//! (`keyboard_cues.rs`), all live through one settings window (`setting_changes.rs`).
//!
//! Text size reports `1.0` on purpose: WebView2 already applies Windows' Text size to the page
//! itself, as a zoom on top of the display scale that follows the setting live (Chromium's
//! `UwpTextScaleFactor`, crbug 40586200; the host must be per-monitor DPI aware, which tao makes
//! it), the way Edge does. Reporting the factor here too would scale every text twice.

mod contrast_theme;
mod keyboard_cues;
mod setting_changes;
mod ui_settings;

use log::warn;
use tauri::AppHandle;
use windows::UI::ViewManagement::UISettings;

use crate::system_events::{DisplayAccessibility, SystemAccent};

pub(crate) use setting_changes::on_setting_change;

/// Brand fallback accent (mustard gold from getcmdr.com).
const FALLBACK_ACCENT_HEX: &str = "#d4a006";

/// The accent and its fill shades, or the brand fallback when `UISettings` can't answer.
pub(crate) async fn accent_color(_app: AppHandle) -> SystemAccent {
    read_accent()
}

/// Every display option Windows has an answer for.
pub(crate) async fn display_accessibility(_app: AppHandle) -> DisplayAccessibility {
    read_display_accessibility()
}

/// The accent, read fresh: a few COM calls, callable from any thread.
fn read_accent() -> SystemAccent {
    accent_from(&ui_settings::settings())
}

/// Every display option this arm reads, fresh. Cheap, and callable from any thread.
fn read_display_accessibility() -> DisplayAccessibility {
    display_accessibility_with(&ui_settings::settings())
}

/// Both answers from ONE `UISettings`, for the settings window's re-read, which wants them
/// together.
fn read_look() -> (SystemAccent, DisplayAccessibility) {
    let settings = ui_settings::settings();
    (accent_from(&settings), display_accessibility_with(&settings))
}

/// The accent from `settings`, or the brand fallback when it (or the read) failed.
fn accent_from(settings: &windows_core::Result<UISettings>) -> SystemAccent {
    match settings.as_ref().map_err(Clone::clone).and_then(ui_settings::accent) {
        Ok(accent) => accent,
        Err(error) => {
            warn!(target: "system_look", "Couldn't read the accent color ({error}), using the brand gold");
            SystemAccent::plain(FALLBACK_ACCENT_HEX.to_owned())
        }
    }
}

/// The display options, with Transparency effects read from `settings`.
fn display_accessibility_with(settings: &windows_core::Result<UISettings>) -> DisplayAccessibility {
    let transparency_effects = settings
        .as_ref()
        .map_err(Clone::clone)
        .and_then(ui_settings::transparency_effects)
        .unwrap_or_else(|error| {
            warn!(target: "system_look", "Couldn't read Transparency effects ({error}), treating it as on");
            true
        });
    display_accessibility_from(WindowsDisplaySettings {
        contrast_theme: contrast_theme::contrast_theme_on(),
        transparency_effects,
        underline_access_keys: keyboard_cues::underline_access_keys(),
    })
}

/// The Windows settings the display options come from.
#[derive(Debug, Clone, Copy)]
struct WindowsDisplaySettings {
    /// A contrast theme is on (`SPI_GETHIGHCONTRAST`).
    contrast_theme: bool,
    /// Settings › Personalization › Colors › Transparency effects.
    transparency_effects: bool,
    /// Settings › Accessibility › Keyboard › Underline access keys.
    underline_access_keys: bool,
}

/// The display options as Windows answers them. A contrast theme is Windows' increased contrast,
/// and it draws no translucency (WinUI drops acrylic under one), so it reduces transparency too.
/// The macOS-only options stay off.
fn display_accessibility_from(settings: WindowsDisplaySettings) -> DisplayAccessibility {
    DisplayAccessibility {
        reduce_transparency: !settings.transparency_effects || settings.contrast_theme,
        increase_contrast: settings.contrast_theme,
        underline_access_keys: settings.underline_access_keys,
        ..DisplayAccessibility::default()
    }
}

/// No slider.
pub(crate) async fn glass_tint_amount() -> Option<f32> {
    None
}

/// `1.0`: WebView2 applies Windows' Text size itself (the module doc has why).
pub(crate) fn text_size_multiplier() -> f32 {
    1.0
}

/// Follows every setting above live, emitting `accent-color-changed` and
/// `display-accessibility-changed` when one moved.
pub(crate) fn observe_changes(app: AppHandle) {
    setting_changes::observe(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: WindowsDisplaySettings = WindowsDisplaySettings {
        contrast_theme: false,
        transparency_effects: true,
        underline_access_keys: false,
    };

    #[test]
    fn windows_defaults_turn_every_option_off() {
        assert_eq!(display_accessibility_from(DEFAULTS), DisplayAccessibility::default());
    }

    #[test]
    fn underline_access_keys_maps_to_its_own_field_alone() {
        let on = display_accessibility_from(WindowsDisplaySettings {
            underline_access_keys: true,
            ..DEFAULTS
        });
        assert_eq!(
            on,
            DisplayAccessibility {
                underline_access_keys: true,
                ..DisplayAccessibility::default()
            }
        );
    }

    #[test]
    fn transparency_effects_off_reduces_transparency() {
        let off = display_accessibility_from(WindowsDisplaySettings {
            transparency_effects: false,
            ..DEFAULTS
        });
        assert_eq!(
            off,
            DisplayAccessibility {
                reduce_transparency: true,
                ..DisplayAccessibility::default()
            }
        );
    }

    #[test]
    fn a_contrast_theme_increases_contrast_and_drops_translucency() {
        let theme = display_accessibility_from(WindowsDisplaySettings {
            contrast_theme: true,
            ..DEFAULTS
        });
        assert_eq!(
            theme,
            DisplayAccessibility {
                reduce_transparency: true,
                increase_contrast: true,
                ..DisplayAccessibility::default()
            }
        );
    }
}
