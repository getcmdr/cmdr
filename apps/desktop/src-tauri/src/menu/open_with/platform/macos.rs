//! macOS: "Open with" is a native submenu (muda), filled from LaunchServices' candidates, with
//! "Other…" an `NSOpenPanel` pick of an app. The first candidate, the OS default, reads
//! "{app} (default)", its words around the name dimmed by `context_menu_icons.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tauri::menu::{MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

use super::super::{OPEN_WITH_ID_PREFIX, OPEN_WITH_OTHER_ID, OPEN_WITH_SUBMENU_ID};
use crate::file_system::open_with::{AppCandidate, OpenWithLaunchError};
use crate::intl::menu_t;
use crate::menu::context_menu_slots::Placeheld;

/// An app to open the files in: macOS has no open-with dialog of its own, so "Other…" picks one
/// in a panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenWithTarget {
    app_path: PathBuf,
}

impl OpenWithTarget {
    pub(crate) fn app(app_path: PathBuf) -> Self {
        Self { app_path }
    }

    pub(crate) fn app_path(&self) -> Option<&Path> {
        Some(&self.app_path)
    }

    pub(crate) fn app_name(&self) -> Option<String> {
        Some(crate::file_system::open_with::app_display_name(&self.app_path))
    }

    pub(crate) fn launch(&self, paths: &[PathBuf]) -> Result<(), OpenWithLaunchError> {
        crate::file_system::open_with::open_paths_with(paths, &self.app_path)
    }
}

/// "Other…" on macOS: an `NSOpenPanel` pick, `None` when cancelled. Runs on the main thread,
/// where muda delivers the click.
pub(crate) fn other_target(_app: &AppHandle<tauri::Wry>) -> Option<OpenWithTarget> {
    crate::file_system::open_with::pick_app_via_open_panel().map(OpenWithTarget::app)
}

/// Builds the "Open with" submenu and returns it alongside a `bundle_id → app_path`
/// map that the caller stores in `MenuState` so click events can resolve the launch
/// target.
pub(crate) fn build_open_with_submenu<R: Runtime>(
    app: &AppHandle<R>,
    candidates: &[AppCandidate],
) -> tauri::Result<(Submenu<R>, HashMap<String, PathBuf>)> {
    let submenu = Submenu::with_id(app, OPEN_WITH_SUBMENU_ID, menu_t("menu.context.openWith"), true)?;
    // An empty intersection (a mixed selection, or no apps registered) still gets "Other…",
    // like Finder, so the user can pick an app by hand.
    let (items, bundle_to_path) = candidate_items(app, candidates)?;
    for item in &items {
        submenu.append(item)?;
    }
    if !items.is_empty() {
        submenu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    submenu.append(&other_item(app)?)?;
    Ok((submenu, bundle_to_path))
}

/// The "Open with" submenu for a menu that went up before LaunchServices answered: a
/// disabled "Finding apps…" line, a separator, and "Other…", which works without the list.
pub(crate) fn build_pending_open_with_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Placeheld<R>> {
    let submenu = Submenu::with_id(app, OPEN_WITH_SUBMENU_ID, menu_t("menu.context.openWith"), true)?;
    let placeholder = MenuItem::new(app, menu_t("menu.context.openWithLoading"), false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    submenu.append(&placeholder)?;
    submenu.append(&separator)?;
    submenu.append(&other_item(app)?)?;
    Ok(Placeheld {
        submenu,
        placeholder,
        separator,
    })
}

/// Swaps the late candidate list into a pending submenu, which may be open on screen, leaving
/// the shape [`build_open_with_submenu`] would have built. Answers the `bundle_id → app_path`
/// map for `MenuState`.
///
/// ❗ Runs while the menu tracks, so it touches only the submenu and its items, never the
/// context menu itself: muda holds that one borrowed for the whole popup.
pub(crate) fn fill_open_with_submenu<R: Runtime>(
    app: &AppHandle<R>,
    pending: &Placeheld<R>,
    candidates: &[AppCandidate],
) -> tauri::Result<HashMap<String, PathBuf>> {
    let (items, bundle_to_path) = candidate_items(app, candidates)?;
    pending.submenu.remove(&pending.placeholder)?;
    if items.is_empty() {
        pending.submenu.remove(&pending.separator)?;
    }
    for (position, item) in items.iter().enumerate() {
        pending.submenu.insert(item, position)?;
    }
    Ok(bundle_to_path)
}

/// The candidates' items, and the `bundle_id → app_path` map a click resolves its app through.
type CandidateItems<R> = (Vec<MenuItem<R>>, HashMap<String, PathBuf>);

/// One item per candidate, the OS default first and marked.
fn candidate_items<R: Runtime>(app: &AppHandle<R>, candidates: &[AppCandidate]) -> tauri::Result<CandidateItems<R>> {
    let mut items = Vec::with_capacity(candidates.len());
    let mut bundle_to_path: HashMap<String, PathBuf> = HashMap::new();
    for candidate in candidates {
        let label = if candidate.is_default {
            default_label(&candidate.display_name)
                .into_iter()
                .map(|part| part.text)
                .collect()
        } else {
            candidate.display_name.clone()
        };
        let id = format!("{OPEN_WITH_ID_PREFIX}{}", candidate.bundle_id);
        items.push(MenuItem::with_id(app, &id, &label, true, None::<&str>)?);
        bundle_to_path.insert(candidate.bundle_id.clone(), candidate.app_path.clone());
    }
    Ok((items, bundle_to_path))
}

/// One stretch of the OS default's label: the app's name, or the words around it that say it's
/// the default, which draw dimmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LabelPart {
    pub text: String,
    pub dim: bool,
}

/// The OS default's label for `app` in the active language, in parts.
pub(crate) fn default_label(app: &str) -> Vec<LabelPart> {
    default_label_parts(&menu_t("menu.context.openWithDefault"), app)
}

/// `template` with each `{app}` as an undimmed part holding `app`, and the words between them
/// dimmed. The parts join into exactly what `menu_t_with` would make of it. A template with no
/// `{app}` (a translation that dropped it) comes back whole and undimmed.
fn default_label_parts(template: &str, app: &str) -> Vec<LabelPart> {
    const TOKEN: &str = "{app}";
    if !template.contains(TOKEN) {
        return vec![LabelPart {
            text: template.to_string(),
            dim: false,
        }];
    }
    let mut parts = Vec::new();
    for (index, words) in template.split(TOKEN).enumerate() {
        if index > 0 && !app.is_empty() {
            parts.push(LabelPart {
                text: app.to_string(),
                dim: false,
            });
        }
        if !words.is_empty() {
            parts.push(LabelPart {
                text: words.to_string(),
                dim: true,
            });
        }
    }
    parts
}

fn other_item<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<MenuItem<R>> {
    MenuItem::with_id(
        app,
        OPEN_WITH_OTHER_ID,
        menu_t("menu.context.openWithOther"),
        true,
        None::<&str>,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: &str, dim: bool) -> LabelPart {
        LabelPart {
            text: text.to_string(),
            dim,
        }
    }

    #[test]
    fn the_words_around_the_app_name_dim_and_the_name_does_not() {
        assert_eq!(
            default_label_parts("{app} (default)", "Preview"),
            vec![part("Preview", false), part(" (default)", true)]
        );
    }

    /// A language may put its words before the name, or on both sides.
    #[test]
    fn words_on_either_side_of_the_name_dim() {
        assert_eq!(
            default_label_parts("Standard: {app} ✓", "Vorschau"),
            vec![part("Standard: ", true), part("Vorschau", false), part(" ✓", true)]
        );
    }

    /// A translation that dropped `{app}` is a bug, but its label still shows as it's written,
    /// undimmed, rather than as one gray line.
    #[test]
    fn a_template_without_the_name_shows_plain() {
        assert_eq!(default_label_parts("Default", "Preview"), vec![part("Default", false)]);
    }

    /// The parts always join into exactly the label `menu_t_with` makes (a plain `{app}`
    /// replace), which is the title the icon pass matches the item by.
    #[test]
    fn the_parts_join_into_the_plain_label() {
        for template in ["{app} (default)", "{app}（默认）", "Default", "{app} and {app}"] {
            let joined: String = default_label_parts(template, "Preview")
                .into_iter()
                .map(|part| part.text)
                .collect();
            assert_eq!(joined, template.replace("{app}", "Preview"));
        }
    }
}
