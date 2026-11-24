//! Windows: "Open with" is drawn in the page ([`page_open_with_submenu`]), from the Shell's
//! handlers (`file_system::open_with`), each with its icon, and "Other…" is Windows' own "Choose
//! another app" dialog ([`OpenWithTarget::ChooseAnother`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tauri::AppHandle;

use super::super::{OPEN_WITH_ID_PREFIX, OPEN_WITH_OTHER_ID, OPEN_WITH_SUBMENU_ID};
use crate::file_system::open_with::{AppCandidate, OpenWithLaunchError};

/// A Shell handler to open the files in, or Windows' "Choose another app" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenWithTarget {
    /// A listed handler, by its id (`IAssocHandler::GetName`), carried as a path.
    App(PathBuf),
    /// The dialog, over the window whose raw `HWND` this is (0: none).
    ChooseAnother { parent: isize },
}

impl OpenWithTarget {
    pub(crate) fn app(app_path: PathBuf) -> Self {
        Self::App(app_path)
    }

    pub(crate) fn app_path(&self) -> Option<&Path> {
        match self {
            Self::App(app_path) => Some(app_path),
            Self::ChooseAnother { .. } => None,
        }
    }

    /// No app for the dialog yet: it picks its own.
    pub(crate) fn app_name(&self) -> Option<String> {
        self.app_path().map(crate::file_system::open_with::app_display_name)
    }

    /// The dialog takes one file, so it gets the first of `paths`.
    pub(crate) fn launch(&self, paths: &[PathBuf]) -> Result<(), OpenWithLaunchError> {
        match self {
            Self::App(app_path) => crate::file_system::open_with::open_paths_with(paths, app_path),
            Self::ChooseAnother { parent } => {
                let first = paths.first().ok_or(OpenWithLaunchError::NoPath)?;
                crate::file_system::open_with::choose_another_app(first, *parent)
            }
        }
    }
}

/// "Choose another app" on Windows: its own dialog, over the main window, as File Explorer's
/// sits over its own.
pub(crate) fn other_target(app: &AppHandle<tauri::Wry>) -> Option<OpenWithTarget> {
    use tauri::Manager as _;

    let parent = app
        .get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .map_or(0, |hwnd| hwnd.0 as isize);
    Some(OpenWithTarget::ChooseAnother { parent })
}

/// The "Open with" submenu as a page draws it (Windows' in-page menus): one row per candidate,
/// each with its app's icon, the default marked, then "Choose another app". Each row's id goes
/// into `live`, so a pick of it may run, and the answer carries the `id → app` map a pick
/// resolves its app through, as [`build_open_with_submenu`]'s does.
///
/// `offer_another` leaves "Choose another app" out for rows only a route serves (a file inside
/// an archive): Windows' dialog needs the file itself, and a copy refused there would have no
/// app to name.
pub(crate) fn page_open_with_submenu(
    candidates: &[AppCandidate],
    offer_another: bool,
    live: &mut std::collections::HashSet<String>,
) -> (cmdr_menu::page::Row, HashMap<String, PathBuf>) {
    use cmdr_menu::page::{Row, RowLabel};

    let mut rows = Vec::with_capacity(candidates.len() + 2);
    let mut bundle_to_path = HashMap::new();
    for candidate in candidates {
        let id = format!("{OPEN_WITH_ID_PREFIX}{}", candidate.bundle_id);
        let label = if candidate.is_default {
            RowLabel::Key {
                key: "menu.context.openWithDefault".to_string(),
                args: HashMap::from([("app".to_string(), candidate.display_name.clone())]),
            }
        } else {
            RowLabel::Text {
                text: candidate.display_name.clone(),
            }
        };
        let icon = candidate
            .icon
            .as_ref()
            .and_then(|icon| crate::icons::rgba_to_data_url(&icon.rgba, icon.width, icon.height));
        live.insert(id.clone());
        rows.push(Row::Item {
            id,
            label,
            shortcut: None,
            aria_keyshortcuts: None,
            enabled: true,
            icon,
        });
        bundle_to_path.insert(candidate.bundle_id.clone(), candidate.app_path.clone());
    }
    if offer_another {
        if !rows.is_empty() {
            rows.push(Row::Separator);
        }
        live.insert(OPEN_WITH_OTHER_ID.to_string());
        rows.push(Row::Item {
            id: OPEN_WITH_OTHER_ID.to_string(),
            label: RowLabel::Key {
                key: "menu.context.openWithChooseAnother".to_string(),
                args: HashMap::new(),
            },
            shortcut: None,
            aria_keyshortcuts: None,
            enabled: true,
            icon: None,
        });
    }
    let submenu = Row::Submenu {
        id: OPEN_WITH_SUBMENU_ID.to_string(),
        label: RowLabel::Key {
            key: "menu.context.openWith".to_string(),
            args: HashMap::new(),
        },
        enabled: !rows.is_empty(),
        rows,
    };
    (submenu, bundle_to_path)
}
