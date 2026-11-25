//! The Windows half of `open_with.rs`: the apps File Explorer's "Open with" offers, read from
//! the Shell's handlers (`platform::assoc`), and handing the rows to one of them or to Windows'
//! own "Choose another app" dialog.
//!
//! - A candidate's `bundle_id` and `app_path` both carry the handler's id
//!   (`IAssocHandler::GetName`), which is what a click names and [`open_paths_with`] finds the
//!   handler by again. It's a program path for a classic app and an app id for a packaged one, so
//!   ❌ never treat `app_path` as a file here.
//! - A multi-selection offers the apps every selected type offers, the first file's default
//!   marked (`intersect_candidate_lists`).
//! - Per-extension lists, icons included, are cached for [`CACHE_TTL`]: the Shell has no
//!   "the set of apps changed" signal to subscribe to, and a right-click must stay fast.
//!
//! Every Shell call runs on a thread of its own in a COM apartment
//! (`platform::shell::SHELL_THREAD_STACK`, `platform::apartment`). `DETAILS.md` § "Open with".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use super::{
    AppCandidate, AppIcon, OpenWithChoices, OpenWithLaunchError, extension_cache_key, intersect_candidate_lists,
};
use crate::ignore_poison::IgnorePoison;

/// How long one type's list stands. Short: an app installed meanwhile shows up on the next
/// right-click after this, and a cold read of a type's handlers is tens of milliseconds.
const CACHE_TTL: Duration = Duration::from_secs(2 * 60);

/// The icon's pixel side: 16 px rows on a 200% display.
const ICON_SIZE: u32 = 32;

struct CacheEntry {
    candidates: Vec<AppCandidate>,
    fetched_at: Instant,
}

/// Lowercased extension → that type's candidates.
static CACHE: LazyLock<Mutex<HashMap<String, CacheEntry>>> = LazyLock::new(Default::default);

/// The name the Shell listed the handler `id` under, from the lists read so far.
pub(super) fn listed_name(id: &Path) -> Option<String> {
    let id = id.to_string_lossy();
    CACHE
        .lock_ignore_poison()
        .values()
        .flat_map(|entry| entry.candidates.iter())
        .find(|candidate| candidate.bundle_id.eq_ignore_ascii_case(&id))
        .map(|candidate| candidate.display_name.clone())
}

/// `extension`'s candidates, cached. Call it in a COM apartment. A lookup that broke answers
/// none for now and caches nothing, so the next right-click asks again.
fn candidates_for(extension: &str) -> Vec<AppCandidate> {
    if let Some(entry) = CACHE.lock_ignore_poison().get(extension)
        && entry.fetched_at.elapsed() < CACHE_TTL
    {
        return entry.candidates.clone();
    }
    let apps = match crate::platform::assoc::apps_for(extension, Some(ICON_SIZE)) {
        Ok(apps) => apps,
        Err(refusal) => {
            log::warn!(target: "open_with", "The Shell's handler lookup broke: {refusal:?}");
            return Vec::new();
        }
    };
    let candidates: Vec<AppCandidate> = apps
        .into_iter()
        .map(|app| AppCandidate {
            app_path: PathBuf::from(&app.id),
            bundle_id: app.id,
            display_name: app.name,
            icon: app.icon.map(|img| AppIcon {
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
            }),
            is_default: app.is_default,
        })
        .collect();
    CACHE.lock_ignore_poison().insert(
        extension.to_string(),
        CacheEntry {
            candidates: candidates.clone(),
            fetched_at: Instant::now(),
        },
    );
    candidates
}

/// The apps that open every one of `paths`, the first file's default first and marked. A path
/// with no extension has no type to ask about, so a selection holding one offers none. Blocks
/// on a Shell thread; call it off the main thread.
pub fn compute_open_with_choices(paths: Vec<PathBuf>) -> OpenWithChoices {
    let mut extensions: Vec<String> = Vec::new();
    for path in &paths {
        let Some(extension) = extension_cache_key(path) else {
            return OpenWithChoices::default();
        };
        if !extensions.contains(&extension) {
            extensions.push(extension);
        }
    }
    if extensions.is_empty() {
        return OpenWithChoices::default();
    }
    let Some(lists) = crate::platform::shell::blocking_on_shell_thread("open-with-list", move || {
        extensions
            .iter()
            .map(|extension| candidates_for(extension))
            .collect::<Vec<_>>()
    }) else {
        return OpenWithChoices::default();
    };
    let ids: Vec<Vec<PathBuf>> = lists
        .iter()
        .map(|list| {
            list.iter()
                .map(|candidate| PathBuf::from(candidate.bundle_id.to_lowercase()))
                .collect()
        })
        .collect();
    let kept = intersect_candidate_lists(&ids);
    let candidates = lists
        .into_iter()
        .next()
        .unwrap_or_default()
        .into_iter()
        .filter(|candidate| kept.contains(&PathBuf::from(candidate.bundle_id.to_lowercase())))
        .collect();
    OpenWithChoices { candidates }
}

/// Each of `paths` as the Shell's NUL-terminated plain path.
///
/// # Errors
///
/// [`OpenWithLaunchError::NoPlainPath`] when one has no faithful plain spelling, and
/// [`OpenWithLaunchError::NoPath`] for one that names no Windows file at all.
fn shell_items(paths: &[PathBuf]) -> Result<Vec<Vec<u16>>, OpenWithLaunchError> {
    use cmdr_fs::app_path::{AppPathBuf, NativeRefusal};

    if paths.is_empty() {
        return Err(OpenWithLaunchError::NoPath);
    }
    paths
        .iter()
        .map(|path| {
            let app_path = AppPathBuf::from_native_unicode(path).map_err(|_| OpenWithLaunchError::NoPath)?;
            crate::platform::windows_shell_path::shell_path(&app_path).map_err(|why| match why {
                NativeRefusal::NotPlainSafe(_) => OpenWithLaunchError::NoPlainPath,
                _ => OpenWithLaunchError::NoPath,
            })
        })
        .collect()
}

/// Hands `paths` to the handler whose id `app_path` carries, as one launch. The paths are
/// checked here; the Shell call runs on a thread of its own, which logs its answer, so a
/// handler that takes its time never holds the caller (a menu pick on the main thread).
///
/// # Errors
///
/// [`shell_items`]'s, before anything launches.
pub fn open_paths_with(paths: &[PathBuf], app_path: &Path) -> Result<(), OpenWithLaunchError> {
    let items = shell_items(paths)?;
    let extension = paths
        .first()
        .and_then(|path| extension_cache_key(path))
        .ok_or(OpenWithLaunchError::NoPath)?;
    let handler = app_path.to_string_lossy().into_owned();
    #[cfg(feature = "playwright-e2e")]
    {
        let _ = (items, extension, handler);
        for path in paths {
            crate::open_mock::record(path.to_string_lossy().into_owned());
        }
        Ok(())
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        on_own_thread("open-with", move || {
            crate::platform::assoc::invoke(&extension, &handler, &items)
        })
    }
}

/// Shows Windows' "How do you want to open this file?" dialog for `path`, over the window
/// `parent` (a raw `HWND`, 0 for none). The dialog opens the file in what the user picks; it
/// takes one file, so a multi-selection hands it the first. Returns once the dialog is up on its
/// own thread.
///
/// # Errors
///
/// [`shell_items`]'s, before anything shows.
pub fn choose_another_app(path: &Path, parent: isize) -> Result<(), OpenWithLaunchError> {
    let mut items = shell_items(&[path.to_path_buf()])?;
    let item = items.pop().ok_or(OpenWithLaunchError::NoPath)?;
    #[cfg(feature = "playwright-e2e")]
    {
        let _ = (item, parent);
        crate::open_mock::record(path.to_string_lossy().into_owned());
        Ok(())
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        on_own_thread("choose-another-app", move || {
            crate::platform::assoc::choose_another_app(parent, &item)
        })
    }
}

/// Runs a Shell call on a thread of its own, in an apartment, logging how it ended:
/// `ERROR_CANCELLED` (a dismissed dialog, a cancelled UAC prompt) quietly. The thread then serves
/// its apartment's messages for `shell::HANDOFF_LINGER` before leaving: `Invoke` and the dialog
/// have no `SEE_MASK_NOASYNC`, and a DDE or asynchronous handler takes the files through them.
#[cfg(not(feature = "playwright-e2e"))]
fn on_own_thread(
    name: &'static str,
    call: impl FnOnce() -> Result<(), crate::platform::shell::ShellRefusal> + Send + 'static,
) -> Result<(), OpenWithLaunchError> {
    use crate::platform::shell::ShellRefusal;
    use windows_sys::Win32::Foundation::ERROR_CANCELLED;

    let spawned = std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(crate::platform::shell::SHELL_THREAD_STACK)
        .spawn(move || {
            let answer = crate::platform::apartment::in_apartment(|| {
                let answer = call();
                crate::platform::apartment::pump_messages_for(crate::platform::shell::HANDOFF_LINGER);
                answer
            });
            match answer {
                Ok(Ok(())) | Ok(Err(ShellRefusal::Win32(ERROR_CANCELLED))) => {}
                // allowed-log-private: `name` is our own fixed thread name, and `answer` a typed code
                answer => log::warn!(target: "open_with", "{name}: the Shell didn't open the file: {answer:?}"),
            }
        });
    spawned.map(|_| ()).map_err(|e| {
        // allowed-log-private: `name` is our own fixed thread name, and `e` the OS's spawn refusal
        log::warn!(target: "open_with", "{name}: couldn't start a thread: {e}");
        OpenWithLaunchError::NotLaunched
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows ships a handler for plain text on every edition (Notepad), so the list is never
    /// empty, and its first app is the marked default when Windows names one.
    #[test]
    fn a_text_file_lists_at_least_one_app() {
        let choices = compute_open_with_choices(vec![
            PathBuf::from(r"C:\Windows\win.ini"),
            PathBuf::from(r"C:\x\notes.txt"),
        ]);
        assert!(!choices.candidates.is_empty(), "no handler for .ini and .txt");
        assert!(
            choices
                .candidates
                .iter()
                .all(|c| !c.display_name.is_empty() && !c.bundle_id.is_empty())
        );
        assert!(
            choices.candidates.iter().skip(1).all(|c| !c.is_default),
            "only the first can be the default"
        );
    }

    #[test]
    fn a_file_with_no_extension_offers_no_app() {
        assert!(
            compute_open_with_choices(vec![PathBuf::from(r"C:\x\Makefile")])
                .candidates
                .is_empty()
        );
    }

    #[test]
    fn a_path_with_no_plain_spelling_launches_nothing() {
        let long = format!(r"C:\{}\notes.txt", "a".repeat(300));
        assert_eq!(
            open_paths_with(&[PathBuf::from(long)], Path::new("notepad")),
            Err(OpenWithLaunchError::NoPlainPath)
        );
        assert_eq!(
            open_paths_with(&[PathBuf::from(r"C:\x\folder.\notes.txt")], Path::new("notepad")),
            Err(OpenWithLaunchError::NoPlainPath)
        );
        assert_eq!(
            open_paths_with(&[], Path::new("notepad")),
            Err(OpenWithLaunchError::NoPath)
        );
    }
}
