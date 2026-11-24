//! Windows: the file menu's "Open with", from the Shell's handlers, read on a Shell thread and
//! waited on for at most [`OPEN_WITH_WAIT`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use cmdr_menu::context::Slot;
use cmdr_menu::page::Row;
use tauri::Runtime;

use crate::file_system::open_with::{OpenWithChoices, compute_open_with_choices};
use crate::ignore_poison::IgnorePoison;
use crate::menu::MenuState;

/// How long a file menu waits for the "Open with" apps. A warm type answers from the cache at
/// once; a cold one is tens of milliseconds, its icons included. Past this the menu opens with
/// only "Choose another app", and the log says so.
const OPEN_WITH_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

/// The "Open with" apps for a file menu's rows, and the `id → handler` map its drawing made.
pub(crate) struct PageSlots {
    open_with: Option<OpenWith>,
    apps: Option<HashMap<String, PathBuf>>,
}

struct OpenWith {
    choices: OpenWithChoices,
    /// `false` for rows only a route serves (a file inside an archive): Windows' dialog needs
    /// the file itself.
    offer_another: bool,
}

impl PageSlots {
    pub(crate) fn fill(&mut self, slot: Slot, live: &mut HashSet<String>) -> Vec<Row> {
        match (slot, &self.open_with) {
            (Slot::OpenWith, Some(open_with)) => {
                let (submenu, apps) = crate::menu::open_with::platform::page_open_with_submenu(
                    &open_with.choices.candidates,
                    open_with.offer_another,
                    live,
                );
                self.apps = Some(apps);
                vec![submenu]
            }
            _ => Vec::new(),
        }
    }

    /// A pick resolves its app through these, as a native pick does (`open_with::handle_pick`).
    pub(crate) fn keep<R: Runtime>(self, menu_state: &MenuState<R>) {
        if let Some(apps) = self.apps {
            menu_state.context.lock_ignore_poison().open_with_apps = apps;
        }
    }
}

pub(crate) async fn read(file_rows: Option<Vec<String>>) -> PageSlots {
    let open_with = match file_rows {
        Some(rows) => Some(read_open_with(&rows).await),
        None => None,
    };
    PageSlots { open_with, apps: None }
}

async fn read_open_with(rows: &[String]) -> OpenWith {
    let native: Vec<PathBuf> = rows.iter().filter_map(|row| crate::app_text::native(row)).collect();
    let offer_another = !crate::file_viewer::open_with_extract::any_needs_extraction(&native);
    // Only the extensions matter to the Shell's handler lists, so an archive's insides ask as
    // they are, with no stand-in file.
    let asked = tokio::time::timeout(
        OPEN_WITH_WAIT,
        tokio::task::spawn_blocking(move || compute_open_with_choices(native)),
    )
    .await;
    let choices = match asked {
        Ok(Ok(choices)) => choices,
        Ok(Err(e)) => {
            log::warn!(target: "open_with", "The Open with list didn't come back: {e}");
            OpenWithChoices::default()
        }
        Err(_) => {
            log::info!(target: "open_with", "The Open with list took over {OPEN_WITH_WAIT:?}; opening the menu without it");
            OpenWithChoices::default()
        }
    };
    OpenWith { choices, offer_another }
}
