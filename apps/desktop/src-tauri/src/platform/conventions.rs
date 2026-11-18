//! How this platform's own apps behave, as data the UI primitives read instead of asking which OS
//! they run on: dialog button order, which keys commit and cancel a dialog, access keys, the menu
//! model, and the screen readers to expect. Linux answers per desktop (GNOME, KDE, other), so one
//! primitive serves all five without an OS check. See `apps/desktop/src/lib/platform/DETAILS.md`
//! § Conventions; the guideline sources are `docs/notes/windows-port/platform-conventions.md`.

/// Every platform convention a UI primitive asks, for the desktop this window lives on. Each OS arm
/// picks its row of [`conventions_on`] (`macos.rs`, `linux/mod.rs`, `windows.rs`), and
/// `frontend.rs` hands it to every webview beside the capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConventions {
    /// The order of a dialog's commit buttons.
    pub dialog_button_order: DialogButtonOrder,
    /// Which button Enter (Return) presses in a dialog.
    pub enter_activates: EnterActivates,
    /// The keys that press a dialog's Cancel (or Close) button.
    pub cancel_keys: CancelKeys,
    /// Whether controls carry access keys (mnemonics) and how the user reaches them.
    pub access_keys: AccessKeys,
    /// Where the app's menus live.
    pub menu_model: MenuModel,
    /// The screen readers this platform's users run, most used first: the ones a screen reader
    /// pass tests with and any hint names.
    pub screen_readers: &'static [ScreenReader],
    /// The Linux desktop this window lives on; `None` off Linux. Only the Linux arm can set it,
    /// because [`conventions_on`] derives it from the desktop it answers for.
    pub linux_desktop: Option<LinuxDesktop>,
}

/// Where the affirmative ("do it") button sits relative to Cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DialogButtonOrder {
    /// Cancel first, the affirmative button last (on the trailing side): macOS, GNOME.
    CancelFirst,
    /// The affirmative button first, Cancel after it: Windows, KDE.
    AffirmativeFirst,
}

/// Which button Enter presses while a dialog has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum EnterActivates {
    /// Always the dialog's default button, wherever focus is; Space presses the focused one.
    DefaultButton,
    /// The focused button when a button has focus, else the default button.
    FocusedButton,
}

/// The keys that cancel a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CancelKeys {
    /// Esc.
    Escape,
    /// Esc, or ⌘. (Command-period).
    EscapeOrCommandPeriod,
}

/// Whether labeled controls carry access keys (an underlined letter pressed with Alt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AccessKeys {
    /// The platform has no access keys.
    None,
    /// Alt plus the marked letter presses the control, and holding Alt reveals the marks.
    Alt,
}

/// Where the app's menus live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MenuModel {
    /// One menu bar at the top of the screen, owned by the OS, that follows the focused app.
    SystemMenuBar,
    /// A menu bar inside each window, under its title bar.
    WindowMenuBar,
}

/// A screen reader a platform's users run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ScreenReader {
    VoiceOver,
    Jaws,
    Nvda,
    Narrator,
    Orca,
}

/// The Linux desktop a window lives on, from `XDG_CURRENT_DESKTOP` (`linux/desktop.rs`).
#[allow(
    dead_code,
    reason = "only the Linux target constructs a desktop; the rest compile for the cross-target table"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LinuxDesktop {
    Gnome,
    Kde,
    /// Any other desktop (Xfce, Cinnamon, MATE, COSMIC, …), or none named.
    Other,
}

/// The desktops that answer conventions differently: each OS, and each Linux desktop.
#[allow(
    dead_code,
    reason = "each target constructs only its own desktop; every row compiles so one test pins them all"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Desktop {
    Macos,
    Windows,
    Linux(LinuxDesktop),
}

#[cfg(test)]
impl Desktop {
    /// Every desktop, in the order the frontend's fixture lists them.
    pub(crate) const ALL: [Desktop; 5] = [
        Desktop::Macos,
        Desktop::Windows,
        Desktop::Linux(LinuxDesktop::Gnome),
        Desktop::Linux(LinuxDesktop::Kde),
        Desktop::Linux(LinuxDesktop::Other),
    ];
}

/// The conventions on `desktop`. Every desktop's row lives here, compiled on every target, so the
/// frontend's fixture of all five (`conventions-on.json`) is pinned by one test on any machine;
/// each OS arm only picks its row.
///
/// Every value cites its guideline in `docs/notes/windows-port/platform-conventions.md`. Where the
/// value is a product decision still open (Windows and GNOME's menu model, task 63 of
/// `docs/specs/windows-port-plan.md`), it carries today's behavior.
pub(crate) const fn conventions_on(desktop: Desktop) -> PlatformConventions {
    match desktop {
        Desktop::Macos => PlatformConventions {
            dialog_button_order: DialogButtonOrder::CancelFirst,
            enter_activates: EnterActivates::DefaultButton,
            cancel_keys: CancelKeys::EscapeOrCommandPeriod,
            access_keys: AccessKeys::None,
            menu_model: MenuModel::SystemMenuBar,
            screen_readers: &[ScreenReader::VoiceOver],
            linux_desktop: None,
        },
        Desktop::Windows => PlatformConventions {
            dialog_button_order: DialogButtonOrder::AffirmativeFirst,
            enter_activates: EnterActivates::FocusedButton,
            cancel_keys: CancelKeys::Escape,
            access_keys: AccessKeys::Alt,
            // Today's in-window bar (muda's Win32 menu); task 63 decides the model.
            menu_model: MenuModel::WindowMenuBar,
            screen_readers: &[ScreenReader::Jaws, ScreenReader::Nvda, ScreenReader::Narrator],
            linux_desktop: None,
        },
        Desktop::Linux(desktop) => PlatformConventions {
            dialog_button_order: match desktop {
                LinuxDesktop::Kde => DialogButtonOrder::AffirmativeFirst,
                // Other desktops are mostly GTK-based, and Cmdr's own window is GTK
                // (WebKitGTK), so GTK's order matches the system dialogs it shows.
                LinuxDesktop::Gnome | LinuxDesktop::Other => DialogButtonOrder::CancelFirst,
            },
            enter_activates: EnterActivates::FocusedButton,
            cancel_keys: CancelKeys::Escape,
            access_keys: AccessKeys::Alt,
            // Today's in-window bar on every desktop. GNOME's HIG has no menu bar (a primary
            // menu in the header bar instead); task 63 revisits it per desktop.
            menu_model: MenuModel::WindowMenuBar,
            screen_readers: &[ScreenReader::Orca],
            linux_desktop: Some(desktop),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend's copy of every desktop's row, which the browser demo and the per-desktop test
    /// helpers read. Lives beside `#lib/platform`, here only pinned.
    const FIXTURE: &str = include_str!("../../../src/lib/platform/conventions-on.json");

    fn fixture_key(desktop: Desktop) -> &'static str {
        match desktop {
            Desktop::Macos => "macos",
            Desktop::Windows => "windows",
            Desktop::Linux(LinuxDesktop::Gnome) => "gnome",
            Desktop::Linux(LinuxDesktop::Kde) => "kde",
            Desktop::Linux(LinuxDesktop::Other) => "otherLinux",
        }
    }

    #[test]
    fn the_frontend_fixture_is_every_desktops_row() {
        let mut expected = serde_json::Map::new();
        for desktop in Desktop::ALL {
            let row = serde_json::to_value(conventions_on(desktop)).expect("serializes");
            expected.insert(fixture_key(desktop).to_string(), row);
        }
        let expected = serde_json::Value::Object(expected);
        let actual: serde_json::Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
        assert_eq!(
            actual,
            expected,
            "`apps/desktop/src/lib/platform/conventions-on.json` drifted from `conventions_on`; write this:\n{}",
            serde_json::to_string_pretty(&expected).expect("serializes")
        );
    }

    #[test]
    fn only_a_linux_desktop_names_one() {
        for desktop in Desktop::ALL {
            let named = conventions_on(desktop).linux_desktop;
            match desktop {
                Desktop::Linux(linux) => assert_eq!(named, Some(linux)),
                Desktop::Macos | Desktop::Windows => assert_eq!(named, None),
            }
        }
    }

    #[test]
    fn every_desktop_expects_a_screen_reader() {
        for desktop in Desktop::ALL {
            assert!(!conventions_on(desktop).screen_readers.is_empty(), "{desktop:?}");
        }
    }

    #[test]
    fn button_order_splits_linux_by_desktop() {
        let order = |desktop| conventions_on(desktop).dialog_button_order;
        assert_eq!(order(Desktop::Macos), DialogButtonOrder::CancelFirst);
        assert_eq!(order(Desktop::Windows), DialogButtonOrder::AffirmativeFirst);
        assert_eq!(
            order(Desktop::Linux(LinuxDesktop::Gnome)),
            DialogButtonOrder::CancelFirst
        );
        assert_eq!(
            order(Desktop::Linux(LinuxDesktop::Kde)),
            DialogButtonOrder::AffirmativeFirst
        );
    }
}
