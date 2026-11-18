//! What a page menu prints for a shortcut, and what `aria-keyshortcuts` says, per platform.

use super::keys::{KeyHint, aria_keyshortcuts};
use crate::spec::Platform;

#[test]
fn a_registered_key_prints_the_way_its_platform_does() {
    for (accelerator, platform, shown, aria) in [
        ("Cmd+Shift+P", Platform::MacOs, "⇧⌘P", "Shift+Meta+P"),
        ("Ctrl+Shift+P", Platform::Windows, "Ctrl+Shift+P", "Control+Shift+P"),
        ("Ctrl+Shift+P", Platform::Linux, "Ctrl+Shift+P", "Control+Shift+P"),
        ("Alt+Up", Platform::Windows, "Alt+↑", "Alt+ArrowUp"),
        (
            "Shift+Ctrl+Tab",
            Platform::Windows,
            "Ctrl+Shift+Tab",
            "Control+Shift+Tab",
        ),
        ("Super+E", Platform::Windows, "Win+E", "Meta+E"),
        ("Super+E", Platform::Linux, "Super+E", "Meta+E"),
        ("Ctrl+Equal", Platform::Linux, "Ctrl+=", "Control+="),
        ("Ctrl+Minus", Platform::Windows, "Ctrl+-", "Control+-"),
        ("Cmd++", Platform::MacOs, "⌘+", "Meta+Plus"),
        ("Shift+Space", Platform::MacOs, "⇧Space", "Shift+Space"),
        ("F5", Platform::Windows, "F5", "F5"),
        ("Ctrl+,", Platform::Windows, "Ctrl+,", "Control+,"),
        ("Ctrl+t", Platform::Linux, "Ctrl+T", "Control+T"),
    ] {
        assert_eq!(
            KeyHint::registered(accelerator, platform),
            KeyHint {
                shown: shown.to_string(),
                aria: Some(aria.to_string()),
            },
            "`{accelerator}` on {platform:?}"
        );
    }
}

/// A display-only key is the frontend's own spelling, printed as it is, and still told to a
/// screen reader.
#[test]
fn a_displayed_key_prints_as_pushed() {
    for (shortcut, platform, aria) in [
        ("⇧8", Platform::MacOs, "Shift+8"),
        ("⌥⇧=", Platform::MacOs, "Alt+Shift+="),
        ("*", Platform::MacOs, "*"),
        ("F3", Platform::Windows, "F3"),
        ("Shift+F4", Platform::Linux, "Shift+F4"),
        ("Alt+↑", Platform::Windows, "Alt+ArrowUp"),
        ("Alt+Shift+=", Platform::Windows, "Alt+Shift+="),
    ] {
        assert_eq!(
            KeyHint::displayed(shortcut, platform),
            KeyHint {
                shown: shortcut.to_string(),
                aria: Some(aria.to_string()),
            },
            "`{shortcut}` on {platform:?}"
        );
    }
}

#[test]
fn a_chord_with_no_key_has_no_aria_value() {
    assert_eq!(aria_keyshortcuts(""), None);
}
