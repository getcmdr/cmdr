//! "Open with": the submenu of apps that open the right-clicked files, and what a pick does.
//!
//! Each listed app's item has a stable ID of the form `open-with:<id>`; `handle_pick`
//! prefix-matches that and resolves the app through `MenuState.context.open_with_apps`, so no
//! path is ever encoded in a menu id. [`OPEN_WITH_OTHER_ID`] is "Other…".
//!
//! How the submenu is drawn is per OS (`platform/`): a native muda submenu on macOS, filled from
//! LaunchServices off the main thread (`context_menu_facts.rs`, with a "Finding apps…" stand-in
//! the live menu swaps out), its icons and dimmed "(default)" words through
//! `context_menu_icons.rs`; page rows on Windows, icons as data URLs, with Windows' own "Choose
//! another app" dialog as "Other…" ([`OpenWithTarget`]).
//!
//! The launch ([`launch_with`]) is shared: the volume check, the read-only copy for rows only a
//! route serves, and the toasts that say why nothing launched.

use std::path::PathBuf;

use tauri::{AppHandle, Runtime};

use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;

pub(crate) mod platform;

/// Menu item ID prefix for "Open with" candidate apps. Followed by the app's bundle ID.
pub const OPEN_WITH_ID_PREFIX: &str = "open-with:";

/// The "Open with" submenu's own ID, which `context_menu_icons.rs` finds it by to put the
/// app icons on its items. Outside the `open-with:` family, which `handle_menu_event`
/// prefix-routes as launch targets.
pub const OPEN_WITH_SUBMENU_ID: &str = "open-with-submenu";

/// Menu item ID for "Open with → Other…" (NSOpenPanel picker) and Windows' "Choose another
/// app" (its own dialog).
pub const OPEN_WITH_OTHER_ID: &str = "open-with-other";

/// What an "Open with" pick launches: an app (a listed one, or one picked in macOS's panel), or
/// the OS's own open-with dialog (Windows' "Choose another app"), per OS in `platform/`.
pub(crate) use platform::OpenWithTarget;

/// Logs why an "Open with" launch of `target` went no further, the app's path kept private.
fn log_not_launched(target: &OpenWithTarget, why: std::fmt::Arguments<'_>) {
    match target.app_path() {
        Some(app_path) => log::info!("Open with {}: {why}", LogPrivate::path(app_path)),
        None => log::info!("Open with the OS's dialog: {why}"),
    }
}

/// Runs an "Open with" pick (`open-with:<id>`, or [`OPEN_WITH_OTHER_ID`]) on the rows the menu
/// went up over, and answers whether `id` was one. A listed app resolves through
/// `MenuState.context.open_with_apps` (on Windows the key is the Shell handler's id, and the pick
/// comes from the page's menu); "Other…" is macOS's app panel, Windows' "Choose another app"
/// dialog.
pub(crate) fn handle_pick(app: &AppHandle<tauri::Wry>, id: &str) -> bool {
    use crate::ignore_poison::IgnorePoison;
    use tauri::Manager as _;

    let listed = id.strip_prefix(OPEN_WITH_ID_PREFIX);
    if listed.is_none() && id != OPEN_WITH_OTHER_ID {
        return false;
    }
    let menu_state = app.state::<super::MenuState<tauri::Wry>>();
    let ctx = menu_state.context.lock_ignore_poison();
    let app_path = listed.and_then(|bundle_id| ctx.open_with_apps.get(bundle_id).cloned());
    let rows = ctx.paths.clone();
    let listing_id = ctx.listing_id.clone();
    drop(ctx);
    if rows.is_empty() {
        log::warn!("Open with: no rows for {id}");
        return true;
    }
    let target = match (listed, app_path) {
        (Some(_), Some(app_path)) => OpenWithTarget::app(app_path),
        (Some(bundle_id), None) => {
            log::warn!("Open with: no app for {bundle_id}");
            return true;
        }
        (None, _) => match platform::other_target(app) {
            Some(target) => target,
            None => return true,
        },
    };
    launch_with(app, &rows, target, &listing_id);
    true
}

/// Launches `target` on the rows an "Open with" click acts on, for both the listed apps
/// and "Other…".
///
/// A row only a route serves (a file inside an archive) has no file an app can open, so
/// it's pulled into a fresh read-only copy first (`file_viewer/open_with_extract.rs`). That
/// pull streams the file, so it runs off the main thread, and the launch hops back on.
/// Every other launch stays on the main thread, synchronous, as it always was.
///
/// A pull that can't finish (too big, a locked or damaged archive) launches nothing and
/// tells the main window why (`OpenWithCopyRefused`), which shows it as a toast. So does
/// a row on a volume whose paths no other app can open right now, like a direct SMB
/// share whose macOS mount is gone (`OpenWithNotOnThisMac`): `listing_id` is the pane's
/// listing the menu went up over, which names that volume.
///
/// On Windows a row whose path has no plain spelling another app reads (`NoPlainPath`) launches
/// nothing and says so (`OpenWithNoPlainPath`).
pub(crate) fn launch_with<R: Runtime>(app: &AppHandle<R>, rows: &[String], target: OpenWithTarget, listing_id: &str) {
    use crate::file_system::default_app::listing_paths_reach_other_apps;
    use crate::file_viewer::open_with_extract;

    // The menu's rows are app text a pane sent, and another app opens them: each crosses to its
    // native file here. A row with none (a phone's, a server's) drops out, and the pane's volume
    // check below says why nothing launched.
    let paths: Vec<PathBuf> = rows.iter().filter_map(|row| crate::app_text::native(row)).collect();

    if !open_with_extract::any_needs_extraction(&paths) {
        if !listing_paths_reach_other_apps(listing_id) {
            log_not_launched(
                &target,
                format_args!("not launching, the pane's volume has no OS mount behind its paths"),
            );
            announce_not_on_this_mac(app, target.app_name());
            return;
        }
        if paths.is_empty() {
            log_not_launched(&target, format_args!("no row has a file here"));
            return;
        }
        if let Err(e) = target.launch(&paths) {
            log_not_launched(&target, format_args!("didn't launch: {e}"));
            announce_launch_refused(app, e, &paths);
        }
        return;
    }
    // The menu offers no "Choose another app" over such rows (`page_open_with_submenu`).
    let Some(app_path) = target.app_path().map(std::path::Path::to_path_buf) else {
        log_not_launched(&target, format_args!("a row needs a copy, and the dialog takes none"));
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let copies = match open_with_extract::launch_paths(&paths) {
            Ok(copies) => copies,
            Err(refused) => {
                log::warn!(
                    "Open with: couldn't copy {} out for {}: {}",
                    LogPrivate::path(&refused.path),
                    LogPrivate::path(&app_path),
                    LogError(&refused.error)
                );
                announce_refused_copy(&app, &refused, &app_path);
                return;
            }
        };
        let notice = open_with_extract::OpenWithCopyLaunched::new(
            &paths,
            &copies,
            crate::file_system::open_with::app_display_name(&app_path),
        );
        let launcher = app.clone();
        let hopped =
            app.run_on_main_thread(
                move || match crate::file_system::open_with::open_paths_with(&copies, &app_path) {
                    Ok(()) => announce_launched_copy(&launcher, notice),
                    Err(e) => log::warn!("Open with failed for {}: {e}", LogPrivate::path(&app_path)),
                },
            );
        if let Err(e) = hopped {
            log::warn!("Open with: couldn't reach the main thread to launch: {e}");
        }
    });
}

/// Tells the main window the app got read-only copies, whose changes stay in the copy.
fn announce_launched_copy<R: Runtime>(
    app: &AppHandle<R>,
    notice: Option<crate::file_viewer::open_with_extract::OpenWithCopyLaunched>,
) {
    use tauri_specta::Event;

    if let Some(notice) = notice
        && let Err(e) = notice.emit(app)
    {
        log::warn!("Open with: couldn't tell the main window the app got a copy: {e}");
    }
}

/// Tells the main window an "Open with" launch didn't happen because the file has no
/// path another app can open. `app_name` is `None` for Windows' "Choose another app".
fn announce_not_on_this_mac<R: Runtime>(app: &AppHandle<R>, app_name: Option<String>) {
    use crate::file_system::default_app::OpenWithNotOnThisMac;
    use tauri_specta::Event;

    if let Err(e) = (OpenWithNotOnThisMac { app_name }).emit(app) {
        log::warn!("Open with: couldn't tell the main window the file isn't on this computer: {e}");
    }
}

/// Tells the main window why a launch the OS was never asked for didn't happen, where that's
/// something to say: a path with no plain spelling (Windows). The rest is in the log.
fn announce_launch_refused<R: Runtime>(
    app: &AppHandle<R>,
    why: crate::file_system::open_with::OpenWithLaunchError,
    paths: &[PathBuf],
) {
    use crate::file_system::default_app::OpenWithNoPlainPath;
    use crate::file_system::open_with::OpenWithLaunchError;
    use tauri_specta::Event;

    if why != OpenWithLaunchError::NoPlainPath {
        return;
    }
    let file_name = paths
        .first()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Err(e) = (OpenWithNoPlainPath { file_name }).emit(app) {
        log::warn!("Open with: couldn't tell the main window the path is too awkward to hand over: {e}");
    }
}

/// Tells the main window an "Open with" launch didn't happen, and why.
fn announce_refused_copy<R: Runtime>(
    app: &AppHandle<R>,
    refused: &crate::file_viewer::open_with_extract::RefusedCopy,
    app_path: &std::path::Path,
) {
    use crate::file_viewer::open_with_extract::OpenWithCopyRefused;
    use tauri_specta::Event;

    if let Err(e) =
        OpenWithCopyRefused::new(refused, crate::file_system::open_with::app_display_name(app_path)).emit(app)
    {
        log::warn!("Open with: couldn't tell the main window the copy was refused: {e}");
    }
}
