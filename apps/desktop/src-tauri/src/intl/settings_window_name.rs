//! The English name of Cmdr's settings window on this session's system, for agent-facing text: an
//! MCP tool's note telling the agent where the user flips a switch ("under Preferences › AI").
//!
//! User-facing copy never comes from here; it's the catalog's (`settings.window.title`,
//! `menu.app.settings`, and every "in Settings" pointer, whose `linuxGnome` / `linuxKde` branches
//! say "Preferences" and "Configure Cmdr"). The test below pins this table to the English menu row.

use super::native_strings::{NativeText, pick};

/// "Preferences" on GNOME, "Configure Cmdr" on KDE (Dolphin's own help writes "Configure Dolphin >
/// View"), "Settings" elsewhere.
const SETTINGS_WINDOW_NAME_EN: NativeText = NativeText::PerOs {
    macos: "Settings",
    windows: "Settings",
    linux_gnome: "Preferences",
    linux_kde: "Configure Cmdr",
    other: "Settings",
};

/// This session's name for the settings window, in English.
pub(crate) fn settings_window_name_en() -> &'static str {
    pick(SETTINGS_WINDOW_NAME_EN, super::platform::native_text_os())
}

#[cfg(test)]
mod tests {
    use super::super::native_strings::{lookup_text, per_os_test::EVERY_OS};
    use super::*;

    #[test]
    fn starts_like_the_english_menu_row_on_every_system() {
        let row = lookup_text("en", "menu.app.settings").expect("the settings row is a native key");
        for os in EVERY_OS {
            let name = pick(SETTINGS_WINDOW_NAME_EN, os);
            let menu = pick(row, os);
            let first = name.split(' ').next().expect("a name has a word");
            assert!(menu.starts_with(first), "{os:?}: {name:?} vs the menu row {menu:?}");
        }
    }
}
