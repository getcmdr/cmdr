//! Paths that enter from outside Cmdr, in their volume's own spelling.
//!
//! A Finder drag-in or a paste of files copied in Finder hands the app kernel-mount
//! paths (`/Volumes/<share>/…`), which the macOS kernel spells decomposed. When the
//! share is on a direct connection those paths route to the byte-exact SMB volume,
//! where an accented name the server stores composed would miss. The frontend asks
//! here ONCE, right where the paths enter, so the scan preview, the conflict check,
//! the transfer, and the journal it writes all carry the stored spelling.
//! `crates/cmdr-listing/DETAILS.md` § "A pane path the volume stores another way".

use tokio::time::Duration;

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::deadline::TimedOut;

/// A resolve is a stat plus, on a miss, a parent listing per wrong component. A
/// drop never waits longer than this for it: past the deadline the paths go as
/// given, which on a byte-exact share is at worst a "couldn't find".
const STORED_SPELLINGS_TIMEOUT: Duration = Duration::from_secs(10);

/// `paths` (index-aligned) in the spelling `volume_id` stores them under. A path
/// with no other stored spelling, an ambiguous one, or a volume that isn't
/// registered comes back as given.
#[tauri::command]
#[specta::specta]
pub async fn stored_spellings(volume_id: String, paths: Vec<String>) -> TimedOut<Vec<String>> {
    let as_given = paths.clone();
    let Some(volume) = crate::file_system::volume::manager::get_volume_manager().get(&volume_id) else {
        return TimedOut {
            data: as_given,
            timed_out: false,
        };
    };
    // Each sent path read typed at the edge; text that's no app path (or holds a
    // `..`) has no other spelling to find and goes back as given.
    let typed: Vec<Option<AppPathBuf>> = paths
        .iter()
        .map(|path| crate::commands::wire_path::app_path(path).ok().flatten())
        .collect();
    let asked: Vec<AppPathBuf> = typed.iter().flatten().cloned().collect();
    let resolve = cmdr_listing::stored_spellings(volume.as_ref(), asked);
    match tokio::time::timeout(STORED_SPELLINGS_TIMEOUT, resolve).await {
        Ok(stored) => {
            let mut stored = stored.into_iter();
            let data = as_given
                .into_iter()
                .zip(typed)
                .map(|(given, typed)| match typed {
                    // One answer per typed path, in order; a path with no other
                    // spelling stays exactly as sent.
                    Some(typed) => match stored.next() {
                        Some(spelling) if spelling != typed => spelling.into_string(),
                        _ => given,
                    },
                    None => given,
                })
                .collect();
            TimedOut { data, timed_out: false }
        }
        Err(_) => TimedOut {
            data: as_given,
            timed_out: true,
        },
    }
}
