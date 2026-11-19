//! The WinRT half of the Windows look: `UISettings` answers the accent color with WinUI's shade
//! ramp and the Transparency effects switch (Settings › Personalization › Colors), and posts an
//! event when either moves.
//!
//! Every read makes its own `UISettings`, which is cheap and works from any thread: windows-rs
//! joins the process's multithreaded apartment on first use when the thread has none. The
//! observer keeps one alive for its events, which arrive on a thread pool thread.

use windows::Foundation::TypedEventHandler;
use windows::UI::Color;
use windows::UI::ViewManagement::{UIColorType, UISettings};

use crate::system_events::{AccentFill, SystemAccent};

/// A fresh `UISettings`.
pub(super) fn settings() -> windows_core::Result<UISettings> {
    UISettings::new()
}

/// The accent and the shades WinUI fills controls with: `AccentDark1` in the light theme,
/// `AccentLight2` in the dark one (`AccentFillColorDefault` in `Common_themeresources_any.xaml`).
pub(super) fn accent(settings: &UISettings) -> windows_core::Result<SystemAccent> {
    Ok(SystemAccent {
        hex: hex(settings.GetColorValue(UIColorType::Accent)?),
        fill: Some(AccentFill {
            light: hex(settings.GetColorValue(UIColorType::AccentDark1)?),
            dark: hex(settings.GetColorValue(UIColorType::AccentLight2)?),
        }),
    })
}

/// Whether Transparency effects is on (`AdvancedEffectsEnabled`, Windows 10 1703 and later).
pub(super) fn transparency_effects(settings: &UISettings) -> windows_core::Result<bool> {
    settings.AdvancedEffectsEnabled()
}

/// Runs `on_change` whenever the accent, the light/dark colors, or Transparency effects move. The
/// registrations last as long as `settings` does, so the caller keeps it alive.
pub(super) fn subscribe(settings: &UISettings, on_change: fn()) -> windows_core::Result<()> {
    let handler = TypedEventHandler::<UISettings, windows_core::IInspectable>::new(move |_, _| {
        on_change();
        Ok(())
    });
    // The tokens would only unregister, and these registrations last for the process.
    settings.ColorValuesChanged(&handler)?;
    settings.AdvancedEffectsEnabledChanged(&handler)?;
    Ok(())
}

/// `#rrggbb`. Alpha is dropped: `UISettings` answers every accent shade opaque.
fn hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.R, color.G, color.B)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_color_writes_as_lowercase_rgb_hex() {
        let color = |r, g, b| Color {
            A: 255,
            R: r,
            G: g,
            B: b,
        };
        assert_eq!(hex(color(0, 120, 212)), "#0078d4");
        assert_eq!(hex(color(255, 255, 255)), "#ffffff");
        assert_eq!(hex(Color { A: 0, R: 1, G: 2, B: 3 }), "#010203");
    }

    /// The real machine: `UISettings` activates outside a packaged app and answers a full ramp.
    #[test]
    fn this_machines_accent_reads_with_its_shades() {
        let settings = settings().expect("UISettings activates");
        let accent = accent(&settings).expect("the accent reads");
        let fill = accent.fill.as_ref().expect("Windows names its fill shades");
        for shade in [&accent.hex, &fill.light, &fill.dark] {
            assert!(shade.len() == 7 && shade.starts_with('#'), "{shade}");
        }
        // Dark1 is darker than the accent, Light2 lighter, on any accent.
        assert_ne!(fill.light, fill.dark);
        transparency_effects(&settings).expect("Transparency effects reads");
        subscribe(&settings, || {}).expect("the change events register");
        log::info!(target: "system_look", "This machine's accent: {accent:?}");
    }
}
