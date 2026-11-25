//! How this platform's own apps behave, as data the UI primitives read instead of asking which OS
//! they run on: dialog button order, which keys commit and cancel a dialog, access keys, the menu
//! model, and the screen readers to expect. Linux answers per desktop (GNOME, KDE, other), so one
//! primitive serves all five without an OS check. See `apps/desktop/src/lib/platform/DETAILS.md`
//! § Conventions; the guideline sources are `docs/notes/windows-port/platform-conventions.md`.

/// Every platform convention a UI primitive asks, for the desktop this window lives on. Each OS arm
/// picks its row of [`conventions_on`] (`macos.rs`, `linux/mod.rs`, `windows/mod.rs`), and
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
    /// What stands in for a window menu bar the user hid (View > Show menu bar).
    pub hidden_menu_bar: HiddenMenuBar,
    /// Who draws the right-click menus.
    pub context_menus: ContextMenus,
    /// Which keys held during a drag force a copy or a move.
    pub drop_modifiers: DropModifiers,
    /// How the OS draws its standard folders' icons (Documents, Downloads, …), which decides
    /// whether Cmdr's gold folder recolor may touch them.
    pub standard_folder_icons: StandardFolderIcons,
    /// The screen readers this platform's users run, most used first: the ones a screen reader
    /// pass tests with and any hint names.
    pub screen_readers: &'static [ScreenReader],
    /// The Linux desktop this window lives on; `None` off Linux. Only the Linux arm can set it,
    /// because [`conventions_on`] derives it from the desktop it answers for.
    pub linux_desktop: Option<LinuxDesktop>,
    /// What an app the user picks by hand is on disk, which a "Choose an app…" picker filters on.
    pub app_file: AppFile,
}

/// What a launchable app is on disk: what the "Choose an app…" picker (the terminal and text
/// editor rows) filters on and where it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AppFile {
    /// A `.app` bundle, picked from `/Applications`: macOS.
    AppBundle,
    /// An `.exe` program: Windows.
    Exe,
    /// Any executable file, which no extension names: Linux.
    Executable,
}

/// How a standard folder's icon is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum StandardFolderIcons {
    /// A plain folder in the system accent with a one-color glyph on it (macOS), so the
    /// grayscale-first gold filter recolors it like the generic folder.
    AccentTinted,
    /// Full-color artwork of their own (Windows' blue Desktop, Downloads arrow, Music note), which
    /// the gold filter would gray into a plain gold folder (#1479). Only the generic folder is
    /// recolored.
    OwnColors,
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

/// Whether labeled controls carry access keys (a letter pressed with Alt), and how they show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AccessKeys {
    /// The platform has no access keys.
    None,
    /// Alt plus the letter presses the control, and the letter shows underlined in its label, as
    /// Win32 draws it: once the keyboard has been used in that surface (Win32's keyboard-cue rule:
    /// a menu or dialog opened by Alt, F10, or another key shows them, one opened by a click hides
    /// them until a key is pressed), and always while Windows' "Underline access keys" is on. A
    /// label without the letter (Chinese, Japanese) carries it in parentheses, "保存 (S)".
    UnderlinesAfterKeyboardUse,
    /// The same keys and underlines, shown while Alt is held, as GTK and Qt draw them.
    UnderlinesWhileAltHeld,
}

/// Where the app's menus live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MenuModel {
    /// One menu bar at the top of the screen, owned by the OS, that follows the focused app.
    SystemMenuBar,
    /// A menu bar inside each window, under its title bar.
    WindowMenuBar,
    /// No menu bar: a "Main menu" button in the window's header holds the menus, the "primary
    /// menu" GNOME's HIG asks for.
    PrimaryMenu,
}

/// What stands in for an in-window menu bar the user hid (the "Show menu bar" setting off), beside
/// the main-menu button every desktop off macOS then shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum HiddenMenuBar {
    /// No bar to hide: the OS owns it (macOS's system menu bar).
    None,
    /// A tap of Alt or F10 shows the hidden bar until focus leaves it, as Win32's guide asks and
    /// VS Code's `toggle` does.
    ShowsOnAltOrF10,
    /// F10 opens the main-menu button's menu, GNOME's HIG key; KDE's hamburger answers it too.
    F10OpensMainMenu,
}

/// Who draws the right-click (context) menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ContextMenus {
    /// The OS's own popup menus (muda's native popups).
    Native,
    /// Cmdr draws them in the page, on the same menu component as the in-window bar.
    InPage,
}

/// Which keys held during a drag force a copy or a move, as the platform's file manager reads
/// them. Without one, a drop moves within a volume and copies across volumes everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DropModifiers {
    /// Finder: Option copies, Command moves (Shift moves too, an accelerator for people coming
    /// from Windows). Option wins when both are held.
    OptionCopiesCommandMoves,
    /// Explorer, Nautilus, and Dolphin: Ctrl copies, Shift moves. Ctrl+Shift (their shortcut or
    /// link) copies: Cmdr makes no links, and Ctrl wins as Option does on a Mac.
    ControlCopiesShiftMoves,
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
/// Every value cites its guideline in `docs/notes/windows-port/platform-conventions.md`; the menu
/// model and the context menus follow decision D5 (`docs/notes/windows-port/menu-model.md`).
pub(crate) const fn conventions_on(desktop: Desktop) -> PlatformConventions {
    match desktop {
        Desktop::Macos => PlatformConventions {
            dialog_button_order: DialogButtonOrder::CancelFirst,
            enter_activates: EnterActivates::DefaultButton,
            cancel_keys: CancelKeys::EscapeOrCommandPeriod,
            access_keys: AccessKeys::None,
            menu_model: MenuModel::SystemMenuBar,
            hidden_menu_bar: HiddenMenuBar::None,
            context_menus: ContextMenus::Native,
            drop_modifiers: DropModifiers::OptionCopiesCommandMoves,
            standard_folder_icons: StandardFolderIcons::AccentTinted,
            screen_readers: &[ScreenReader::VoiceOver],
            linux_desktop: None,
            app_file: AppFile::AppBundle,
        },
        Desktop::Windows => PlatformConventions {
            dialog_button_order: DialogButtonOrder::AffirmativeFirst,
            enter_activates: EnterActivates::FocusedButton,
            cancel_keys: CancelKeys::Escape,
            access_keys: AccessKeys::UnderlinesAfterKeyboardUse,
            menu_model: MenuModel::WindowMenuBar,
            hidden_menu_bar: HiddenMenuBar::ShowsOnAltOrF10,
            // In the page, so the bar's flyouts and the right-click menus share one look (D5).
            context_menus: ContextMenus::InPage,
            drop_modifiers: DropModifiers::ControlCopiesShiftMoves,
            standard_folder_icons: StandardFolderIcons::OwnColors,
            screen_readers: &[ScreenReader::Jaws, ScreenReader::Nvda, ScreenReader::Narrator],
            linux_desktop: None,
            app_file: AppFile::Exe,
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
            // GTK and Qt both underline while Alt is held, so every Linux desktop does.
            access_keys: AccessKeys::UnderlinesWhileAltHeld,
            // GNOME's HIG has no menu bar, only a primary menu in the header bar; KDE and the
            // other desktops keep an in-window bar, as their own apps do.
            menu_model: match desktop {
                LinuxDesktop::Gnome => MenuModel::PrimaryMenu,
                LinuxDesktop::Kde | LinuxDesktop::Other => MenuModel::WindowMenuBar,
            },
            hidden_menu_bar: HiddenMenuBar::F10OpensMainMenu,
            // muda's GTK popups are the desktop's own menus.
            context_menus: ContextMenus::Native,
            // Nautilus, Dolphin, and Thunar all read Ctrl and Shift the way Explorer does.
            drop_modifiers: DropModifiers::ControlCopiesShiftMoves,
            // Linux shows a standard folder as the generic folder today (`icons/CLAUDE.md`), so
            // it's recolored like the rest.
            standard_folder_icons: StandardFolderIcons::AccentTinted,
            screen_readers: &[ScreenReader::Orca],
            linux_desktop: Some(desktop),
            app_file: AppFile::Executable,
        },
    }
}

/// The dev flag `CMDR_MOCK_CONVENTIONS=<desktop>` (`macos`, `windows`, `gnome`, `kde`,
/// `otherLinux`, the fixture's names): every webview gets that desktop's row instead of this
/// one's, so a Mac shows the Windows dialog order and access keys. Debug builds only, like the
/// other `CMDR_MOCK_*` flags; `None` when unset or unknown.
#[cfg(debug_assertions)]
pub(crate) fn mock_conventions() -> Option<(Desktop, PlatformConventions)> {
    mock_desktop(std::env::var("CMDR_MOCK_CONVENTIONS").ok().as_deref())
        .map(|desktop| (desktop, conventions_on(desktop)))
}

impl Desktop {
    /// The OS this desktop runs on, in the frontend's `Platform` spelling. The mock hands it to
    /// the webview as the platform its COPY reads, so a mocked desktop's words show beside its
    /// conventions (`apps/desktop/src/lib/platform/DETAILS.md` § Conventions).
    #[cfg(any(debug_assertions, test))]
    pub(crate) const fn copy_platform(self) -> &'static str {
        match self {
            Desktop::Macos => "macos",
            Desktop::Windows => "windows",
            Desktop::Linux(_) => "linux",
        }
    }
}

#[cfg(any(debug_assertions, test))]
fn mock_desktop(value: Option<&str>) -> Option<Desktop> {
    match value? {
        "macos" => Some(Desktop::Macos),
        "windows" => Some(Desktop::Windows),
        "gnome" => Some(Desktop::Linux(LinuxDesktop::Gnome)),
        "kde" => Some(Desktop::Linux(LinuxDesktop::Kde)),
        "otherLinux" => Some(Desktop::Linux(LinuxDesktop::Other)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_flag_names_each_desktop_as_the_fixture_does() {
        for desktop in Desktop::ALL {
            assert_eq!(mock_desktop(Some(fixture_key(desktop))), Some(desktop));
        }
        assert_eq!(mock_desktop(Some("beos")), None);
        assert_eq!(mock_desktop(None), None);
    }

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

    /// #1479: Cmdr gold grayed Windows' full-color Desktop, Downloads, and Music icons into plain
    /// gold folders.
    #[test]
    fn only_windows_draws_its_standard_folders_in_their_own_colors() {
        for desktop in Desktop::ALL {
            let expected = match desktop {
                Desktop::Windows => StandardFolderIcons::OwnColors,
                Desktop::Macos | Desktop::Linux(_) => StandardFolderIcons::AccentTinted,
            };
            assert_eq!(conventions_on(desktop).standard_folder_icons, expected, "{desktop:?}");
        }
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
    fn access_keys_look_like_each_desktops_own() {
        let keys = |desktop| conventions_on(desktop).access_keys;
        assert_eq!(keys(Desktop::Macos), AccessKeys::None);
        assert_eq!(keys(Desktop::Windows), AccessKeys::UnderlinesAfterKeyboardUse);
        for linux in [LinuxDesktop::Gnome, LinuxDesktop::Kde, LinuxDesktop::Other] {
            assert_eq!(
                keys(Desktop::Linux(linux)),
                AccessKeys::UnderlinesWhileAltHeld,
                "{linux:?}"
            );
        }
    }

    #[test]
    fn gnome_gets_a_primary_menu_and_the_rest_off_macos_a_window_menu_bar() {
        let model = |desktop| conventions_on(desktop).menu_model;
        assert_eq!(model(Desktop::Macos), MenuModel::SystemMenuBar);
        assert_eq!(model(Desktop::Windows), MenuModel::WindowMenuBar);
        assert_eq!(model(Desktop::Linux(LinuxDesktop::Gnome)), MenuModel::PrimaryMenu);
        assert_eq!(model(Desktop::Linux(LinuxDesktop::Kde)), MenuModel::WindowMenuBar);
        assert_eq!(model(Desktop::Linux(LinuxDesktop::Other)), MenuModel::WindowMenuBar);
    }

    #[test]
    fn a_hidden_bar_shows_on_alt_on_windows_and_hands_f10_to_the_main_menu_on_linux() {
        for desktop in Desktop::ALL {
            let expected = match desktop {
                Desktop::Macos => HiddenMenuBar::None,
                Desktop::Windows => HiddenMenuBar::ShowsOnAltOrF10,
                Desktop::Linux(_) => HiddenMenuBar::F10OpensMainMenu,
            };
            assert_eq!(conventions_on(desktop).hidden_menu_bar, expected, "{desktop:?}");
        }
    }

    #[test]
    fn only_windows_draws_its_context_menus_in_the_page() {
        for desktop in Desktop::ALL {
            let expected = if desktop == Desktop::Windows {
                ContextMenus::InPage
            } else {
                ContextMenus::Native
            };
            assert_eq!(conventions_on(desktop).context_menus, expected, "{desktop:?}");
        }
    }

    #[test]
    fn only_macos_presses_the_default_button_from_anywhere_and_cancels_on_command_period() {
        for desktop in Desktop::ALL {
            let row = conventions_on(desktop);
            let mac = desktop == Desktop::Macos;
            assert_eq!(row.enter_activates == EnterActivates::DefaultButton, mac, "{desktop:?}");
            assert_eq!(row.cancel_keys == CancelKeys::EscapeOrCommandPeriod, mac, "{desktop:?}");
        }
    }

    #[test]
    fn only_macos_forces_a_drop_with_option_and_command() {
        for desktop in Desktop::ALL {
            let expected = if desktop == Desktop::Macos {
                DropModifiers::OptionCopiesCommandMoves
            } else {
                DropModifiers::ControlCopiesShiftMoves
            };
            assert_eq!(conventions_on(desktop).drop_modifiers, expected, "{desktop:?}");
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
