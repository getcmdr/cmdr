//! macOS: the known-terminals table, keyed by bundle id, and the `open` argv each
//! app takes a folder through.
//!
//! - [`KNOWN_TERMINALS`], the table: bundle id, display name, and the recipe that
//!   says how that app takes a folder.
//! - [`launch_argv`], a pure choice + folder → argv function, so every recipe is
//!   unit-testable without launching anything.
//!
//! Installed-ness is a millisecond `NSWorkspace` question, asked per call. ❌ Never
//! scan `/Applications`.

use std::path::{Path, PathBuf};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use cmdr_fs::app_path::AppPath;

use super::super::{OpenTerminalError, OpenTerminalOutcome, TerminalApp, TerminalAppList};
use crate::file_system::open_with::{
    app_icon_data_url, installed_app_path, read_app_display_name, read_bundle_identifier,
};

/// Terminal.app: always present on macOS, so it's both the default choice and the
/// fallback when the chosen app has been uninstalled.
pub const TERMINAL_APP_BUNDLE_ID: &str = "com.apple.Terminal";

/// How one terminal app takes the folder it should start in.
///
/// Each app is launched the way it natively accepts a folder, and whether that
/// lands in a window or a tab is left to the app's own preferences. There is no
/// portable way to ask for one or the other, and the user already configured that
/// in their terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchRecipe {
    /// `open -b <bundle id> <dir>`: the app registers as a folder handler and
    /// opens there. Covers most terminals.
    FolderAsDocument,
    /// `open -n -b <bundle id> --args --working-directory <dir>`: the app won't
    /// take a folder as a document, so the directory goes in as a CLI flag.
    /// `-n` is load-bearing: without it an already-running instance is merely
    /// activated and the args are dropped.
    WorkingDirectoryFlag,
    /// `open warp://action/new_window?path=<percent-encoded dir>`: Warp's own
    /// documented URI scheme.
    WarpUri,
}

/// One terminal app Cmdr knows how to launch a folder in; its `id` is the bundle id.
pub type KnownTerminal = super::super::KnownTerminal<LaunchRecipe>;

/// Every terminal app Cmdr knows, Terminal first (it's the default) and the rest
/// alphabetically, which is the order the settings dropdown renders.
///
/// Bundle ids verified 2026-09-04, one source each:
/// - Terminal, Ghostty, Warp: `mdls -name kMDItemCFBundleIdentifier` against the
///   copy installed on the machine.
/// - Alacritty: `extra/osx/Alacritty.app/Contents/Info.plist` (`alacritty/alacritty`, `master`).
/// - Hyper: `appId` in `electron-builder.json` (`vercel/hyper`, `canary`).
/// - iTerm2: `PRODUCT_BUNDLE_IDENTIFIER` in `iTerm2.xcodeproj/project.pbxproj`
///   (`gnachman/iTerm2`, `master`). The `com.iterm2.*` ids beside it belong to
///   helper targets (pidinfo, the proxy, the sandboxed worker), not the app.
/// - kitty: `CFBundleIdentifier=f'net.kovidgoyal.{appname}'` in `setup.py`
///   (`kovidgoyal/kitty`, `master`).
/// - WezTerm: `assets/macos/WezTerm.app/Contents/Info.plist` (`wezterm/wezterm`, `main`).
pub const KNOWN_TERMINALS: &[KnownTerminal] = &[
    KnownTerminal {
        id: TERMINAL_APP_BUNDLE_ID,
        display_name: "Terminal",
        recipe: LaunchRecipe::FolderAsDocument,
    },
    KnownTerminal {
        id: "org.alacritty",
        display_name: "Alacritty",
        recipe: LaunchRecipe::WorkingDirectoryFlag,
    },
    KnownTerminal {
        id: "com.mitchellh.ghostty",
        display_name: "Ghostty",
        recipe: LaunchRecipe::FolderAsDocument,
    },
    KnownTerminal {
        id: "co.zeit.hyper",
        display_name: "Hyper",
        recipe: LaunchRecipe::FolderAsDocument,
    },
    KnownTerminal {
        id: "com.googlecode.iterm2",
        display_name: "iTerm2",
        recipe: LaunchRecipe::FolderAsDocument,
    },
    KnownTerminal {
        id: "net.kovidgoyal.kitty",
        display_name: "kitty",
        recipe: LaunchRecipe::FolderAsDocument,
    },
    KnownTerminal {
        id: "dev.warp.Warp-Stable",
        display_name: "Warp",
        recipe: LaunchRecipe::WarpUri,
    },
    KnownTerminal {
        id: "com.github.wez.wezterm",
        display_name: "WezTerm",
        recipe: LaunchRecipe::FolderAsDocument,
    },
];

/// The known terminal with this bundle id, if Cmdr carries a recipe for it.
pub fn known_terminal(bundle_id: &str) -> Option<&'static KnownTerminal> {
    KNOWN_TERMINALS.iter().find(|t| t.id == bundle_id)
}

/// Which app the "open terminal here" setting names: a known bundle id, or a custom
/// pick held as the absolute path to its `.app` bundle, launched as a folder handler,
/// which is what nearly every terminal registers as.
pub type TerminalChoice = super::super::TerminalChoice<LaunchRecipe>;

/// Reads a stored setting value into a choice.
///
/// `None` means the setting names something Cmdr can't launch: a bundle id that
/// left the table, or a value from a build that knew more apps than this one. The
/// caller falls back to Terminal and says so.
pub fn parse_choice(setting: &str) -> Option<TerminalChoice> {
    let setting = setting.trim();
    if setting.is_empty() {
        // No choice recorded yet, which is the same thing as the default.
        return known_terminal(TERMINAL_APP_BUNDLE_ID).map(TerminalChoice::Known);
    }
    let path = Path::new(setting);
    if path.is_absolute() {
        return Some(TerminalChoice::CustomApp(path.to_path_buf()));
    }
    known_terminal(setting).map(TerminalChoice::Known)
}

/// The name to call a stored choice when the app it names has just gone missing: a
/// known bundle id answers from the table, a custom `.app` path with its file stem.
pub(crate) fn choice_display_name(setting: &str) -> Option<String> {
    parse_choice(setting)?.name_without_io()
}

/// The bytes left literal inside Warp's `path=` query value: the RFC 3986
/// unreserved set plus `/`, which is legal in a query and is the shape Warp's own
/// docs show. Everything else, `&`, `?`, `#`, `%`, `+`, space, quotes, and every
/// non-ASCII byte, is percent-encoded, so a folder name can't rewrite the URI.
const WARP_PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// Warp's documented URI scheme for opening a folder in a new window.
/// <https://docs.warp.dev/terminal/more-features/uri-scheme/>
fn warp_new_window_uri(dir: &Path) -> String {
    let dir = dir.to_string_lossy();
    let encoded = utf8_percent_encode(&dir, WARP_PATH_SET);
    format!("warp://action/new_window?path={encoded}")
}

/// Builds the argv that opens `dir` in the chosen terminal, with `open` itself as
/// the first element.
///
/// Pure: no filesystem, no `NSWorkspace`, no spawning, so every recipe is
/// unit-tested directly. Nothing here goes through a shell, so a folder name with
/// spaces, quotes, or `$` is an ordinary argument.
pub fn launch_argv(choice: &TerminalChoice, dir: &Path) -> Vec<String> {
    let dir_arg = dir.to_string_lossy().into_owned();
    match choice {
        TerminalChoice::Known(terminal) => match terminal.recipe {
            LaunchRecipe::FolderAsDocument => {
                vec!["open".into(), "-b".into(), terminal.id.into(), dir_arg]
            }
            LaunchRecipe::WorkingDirectoryFlag => vec![
                "open".into(),
                "-n".into(),
                "-b".into(),
                terminal.id.into(),
                "--args".into(),
                "--working-directory".into(),
                dir_arg,
            ],
            LaunchRecipe::WarpUri => vec!["open".into(), warp_new_window_uri(dir)],
        },
        TerminalChoice::CustomApp(app_path) => vec![
            "open".into(),
            "-a".into(),
            app_path.to_string_lossy().into_owned(),
            dir_arg,
        ],
    }
}

/// Resolves a stored setting to the app that will actually be launched: an
/// installed choice as is, anything else Terminal, reporting that it fell back.
fn resolve_choice(
    setting: &str,
    is_installed: impl Fn(&TerminalChoice) -> bool,
) -> (TerminalChoice, OpenTerminalOutcome) {
    super::super::resolve_choice(parse_choice(setting), is_installed, terminal_app)
}

/// Terminal.app: always present on macOS, so it's both the default and the fallback.
fn terminal_app() -> TerminalChoice {
    known_terminal(TERMINAL_APP_BUNDLE_ID)
        .map(TerminalChoice::Known)
        .expect("the known-terminals table always carries Terminal.app")
}

/// Whether an app with this bundle id has a running instance right now.
fn is_running(bundle_id: &str) -> bool {
    autoreleasepool(|_| {
        !NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id)).is_empty()
    })
}

/// Where a choice's `.app` bundle sits, or `None` once it's been uninstalled.
fn choice_app_path(choice: &TerminalChoice) -> Option<PathBuf> {
    match choice {
        TerminalChoice::Known(terminal) => installed_app_path(terminal.id),
        // A custom pick is an absolute path the user handed us, so its own
        // existence is the whole question.
        TerminalChoice::CustomApp(path) => path.is_dir().then(|| path.clone()),
    }
}

/// The dropdown entry for an app known to be installed at `app_path`. The icon
/// comes from the bundle's own `.icns` (`open_with::app_icon_data_url` says why).
fn app_entry(id: String, display_name: String, app_path: &Path, bundle_id: &str) -> TerminalApp {
    TerminalApp {
        id,
        display_name,
        icon: app_icon_data_url(app_path),
        is_running: is_running(bundle_id),
    }
}

/// Installed-ness is one LaunchServices lookup per app.
pub(crate) fn list_terminal_apps(setting: &str) -> TerminalAppList {
    let mut apps: Vec<TerminalApp> = KNOWN_TERMINALS
        .iter()
        .filter_map(|terminal| {
            let app_path = installed_app_path(terminal.id)?;
            Some(app_entry(
                terminal.id.to_string(),
                terminal.display_name.to_string(),
                &app_path,
                terminal.id,
            ))
        })
        .collect();

    let choice = parse_choice(setting);

    // A custom pick isn't in the table, so it's appended as its own option;
    // otherwise the dropdown couldn't show what's currently selected.
    if let Some(TerminalChoice::CustomApp(path)) = &choice
        && path.is_dir()
    {
        let bundle_id = read_bundle_identifier(path).unwrap_or_default();
        apps.push(app_entry(
            path.to_string_lossy().into_owned(),
            read_app_display_name(path),
            path,
            &bundle_id,
        ));
    }

    let listed = |id: &String| apps.iter().any(|app| &app.id == id);
    let chosen_id = choice.map(|choice| choice.id()).filter(listed);
    let default_id = Some(TERMINAL_APP_BUNDLE_ID.to_string()).filter(listed);
    TerminalAppList {
        apps,
        chosen_id,
        default_id,
    }
}

/// `open` takes the folder's POSIX spelling, which every local app path has.
pub(crate) fn open_in_terminal(dir: &AppPath, setting: &str) -> Result<OpenTerminalOutcome, OpenTerminalError> {
    let Ok(dir) = dir.to_native() else {
        return Ok(OpenTerminalOutcome::NotALocalPath);
    };
    let (choice, outcome) = resolve_choice(setting, |choice| choice_app_path(choice).is_some());
    launch(&launch_argv(&choice, &dir), &dir)?;
    log::info!(
        target: "terminal",
        "opened {dir:?} in {} ({outcome:?})",
        choice.id(),
        dir = cmdr_fs::log_private::LogPrivate::dir_path(&dir),
    );
    Ok(outcome)
}

/// Spawns the built argv. Fire and forget: `open` returns as soon as
/// LaunchServices has the request.
#[cfg(not(feature = "playwright-e2e"))]
fn launch(argv: &[String], _dir: &Path) -> Result<(), OpenTerminalError> {
    let (program, args) = argv.split_first().expect("every recipe puts `open` at argv[0]");
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| OpenTerminalError::LaunchRefused {
            errno: e.raw_os_error(),
        })
}

/// E2E variant: record the folder instead of launching a terminal, so a suite
/// run doesn't pile up windows nothing can close. Same store as `open_path` and
/// `open_in_editor`, read back through `e2e_opened_paths`.
///
/// It records the FOLDER, never `argv`: Warp's recipe ends in a URI rather than a
/// path, so a spec reading `argv` would assert something different for one app in
/// the table than for the other seven.
#[cfg(feature = "playwright-e2e")]
fn launch(_argv: &[String], dir: &Path) -> Result<(), OpenTerminalError> {
    crate::open_mock::record(dir.to_string_lossy().into_owned());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(bundle_id: &str) -> TerminalChoice {
        parse_choice(bundle_id).expect("known bundle id")
    }

    #[test]
    fn terminal_opens_the_folder_as_a_document() {
        assert_eq!(
            launch_argv(&choice(TERMINAL_APP_BUNDLE_ID), Path::new("/Users/test_user_10/code")),
            vec!["open", "-b", "com.apple.Terminal", "/Users/test_user_10/code"]
        );
    }

    #[test]
    fn alacritty_takes_the_folder_as_a_flag_on_a_new_instance() {
        assert_eq!(
            launch_argv(&choice("org.alacritty"), Path::new("/Users/test_user_10/code")),
            vec![
                "open",
                "-n",
                "-b",
                "org.alacritty",
                "--args",
                "--working-directory",
                "/Users/test_user_10/code"
            ]
        );
    }

    #[test]
    fn warp_takes_its_own_uri() {
        assert_eq!(
            launch_argv(&choice("dev.warp.Warp-Stable"), Path::new("/Users/test_user_10/code")),
            vec!["open", "warp://action/new_window?path=/Users/test_user_10/code"]
        );
    }

    #[test]
    fn a_custom_app_is_launched_by_path() {
        let choice = parse_choice("/Applications/Terminus.app").expect("an absolute path is a custom pick");
        assert_eq!(
            launch_argv(&choice, Path::new("/Users/test_user_10/code")),
            vec!["open", "-a", "/Applications/Terminus.app", "/Users/test_user_10/code"]
        );
    }

    /// A shell never sees these argvs, so the awkward characters travel literally
    /// everywhere except Warp's URI, where they're percent-encoded.
    #[test]
    fn awkward_folder_names_travel_literally_as_arguments() {
        let dir = Path::new("/Users/test_user_10/Ünnepi \"terv\" & co");
        for terminal in KNOWN_TERMINALS {
            if terminal.recipe == LaunchRecipe::WarpUri {
                continue;
            }
            let argv = launch_argv(&TerminalChoice::Known(terminal), dir);
            assert_eq!(
                argv.last().map(String::as_str),
                Some("/Users/test_user_10/Ünnepi \"terv\" & co"),
                "{} should take the folder verbatim",
                terminal.display_name
            );
        }
    }

    #[test]
    fn warp_percent_encodes_spaces_quotes_ampersands_and_non_ascii() {
        let argv = launch_argv(
            &choice("dev.warp.Warp-Stable"),
            Path::new("/Users/test_user_10/Ünnepi \"terv\" & co"),
        );
        assert_eq!(
            argv,
            vec![
                "open",
                "warp://action/new_window?path=/Users/test_user_10/%C3%9Cnnepi%20%22terv%22%20%26%20co"
            ]
        );
    }

    /// A folder named like a query parameter must not be able to add one.
    #[test]
    fn warp_encoding_cannot_be_escaped_by_a_folder_name() {
        let argv = launch_argv(
            &choice("dev.warp.Warp-Stable"),
            Path::new("/tmp/x?path=/etc&mode=evil#frag%2F+plus"),
        );
        assert_eq!(
            argv[1],
            "warp://action/new_window?path=/tmp/x%3Fpath%3D/etc%26mode%3Devil%23frag%252F%2Bplus"
        );
    }

    #[test]
    fn every_known_terminal_builds_an_argv_that_names_it() {
        for terminal in KNOWN_TERMINALS {
            let argv = launch_argv(&TerminalChoice::Known(terminal), Path::new("/tmp"));
            assert_eq!(argv.first().map(String::as_str), Some("open"));
            assert!(
                argv.iter().any(|a| a.contains(terminal.id) || a.starts_with("warp://")),
                "{} should be named in its own argv",
                terminal.display_name
            );
        }
    }

    #[test]
    fn bundle_ids_are_unique() {
        let mut ids: Vec<&str> = KNOWN_TERMINALS.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two entries share a bundle id");
    }

    #[test]
    fn an_empty_setting_reads_as_terminal() {
        assert_eq!(
            parse_choice(""),
            known_terminal(TERMINAL_APP_BUNDLE_ID).map(TerminalChoice::Known)
        );
    }

    #[test]
    fn an_unknown_bundle_id_names_nothing() {
        assert_eq!(parse_choice("com.example.NotATerminal"), None);
    }

    #[test]
    fn an_absolute_path_is_a_custom_pick() {
        assert_eq!(
            parse_choice("/Applications/Terminus.app"),
            Some(TerminalChoice::CustomApp(PathBuf::from("/Applications/Terminus.app")))
        );
    }

    #[test]
    fn an_installed_choice_is_used_as_is() {
        let (choice, outcome) = resolve_choice("dev.warp.Warp-Stable", |_| true);
        assert_eq!(choice.id(), "dev.warp.Warp-Stable");
        assert_eq!(outcome, OpenTerminalOutcome::Opened);
    }

    #[test]
    fn an_uninstalled_choice_falls_back_to_terminal_and_says_so() {
        let (choice, outcome) = resolve_choice("dev.warp.Warp-Stable", |_| false);
        assert_eq!(choice.id(), TERMINAL_APP_BUNDLE_ID);
        assert_eq!(outcome, OpenTerminalOutcome::AppMissingOpenedTerminalInstead);
    }

    #[test]
    fn a_setting_this_build_does_not_know_falls_back_to_terminal() {
        let (choice, outcome) = resolve_choice("com.example.NotATerminal", |_| true);
        assert_eq!(choice.id(), TERMINAL_APP_BUNDLE_ID);
        assert_eq!(outcome, OpenTerminalOutcome::AppMissingOpenedTerminalInstead);
    }

    #[test]
    fn a_known_bundle_id_still_has_a_name_after_its_app_is_gone() {
        // The whole point: the toast that says an app went missing needs the name,
        // and by then there's no bundle left to read one out of.
        assert_eq!(choice_display_name("dev.warp.Warp-Stable"), Some("Warp".to_string()));
    }

    #[test]
    fn a_custom_pick_is_named_by_its_bundle_stem() {
        assert_eq!(
            choice_display_name("/Applications/Terminus.app"),
            Some("Terminus".to_string())
        );
    }

    #[test]
    fn a_bundle_id_that_left_the_table_has_no_name_to_offer() {
        // ❌ Never the raw bundle id: it would reach a user as if it were a name.
        assert_eq!(choice_display_name("com.example.NotATerminal"), None);
    }
}
