//! The icon each switcher row shows, without touching a drive that could hang or that Cmdr
//! shouldn't read. Only the system drive asks the Shell (its Windows logo), on a thread of its
//! own with a bounded wait; every other drive shows its kind's stock icon with no call to it,
//! and a favorite asks for its `special:` icon only when it sits on the system drive. Why:
//! `icons/DETAILS.md` § "Drive icons".

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::windows_volume_ids::WindowsDriveKind;

use crate::icons::platform::StockDriveIcon;
use crate::volumes::{DEFAULT_VOLUME_ID, LocationCategory, LocationInfo};

/// How long the list waits for the system drive's and the favorites' Shell icons. A healthy
/// system drive answers in milliseconds; a late answer republishes the list when it lands.
const SHELL_ICON_WAIT: Duration = Duration::from_millis(400);

/// The system drive's Shell icon, once read this run.
static SYSTEM_DRIVE_ICON: Mutex<Option<String>> = Mutex::new(None);

/// Whether a lookup of the system drive's icon is running, so a burst of lists starts one.
static SYSTEM_LOOKUP_RUNNING: AtomicBool = AtomicBool::new(false);

/// A drive kind's stock icon, the only icon a drive other than the system drive gets. External
/// disks and mounted VHDXs report as fixed, and the Shell would read their `autorun.inf` or
/// root `desktop.ini`, which can name a share; a disc would spin up; a share can hang.
pub(super) fn stock_icon_for(kind: WindowsDriveKind) -> StockDriveIcon {
    match kind {
        WindowsDriveKind::Fixed | WindowsDriveKind::Subst => StockDriveIcon::Fixed,
        WindowsDriveKind::Ram => StockDriveIcon::Ram,
        WindowsDriveKind::Removable => StockDriveIcon::Removable,
        WindowsDriveKind::Optical => StockDriveIcon::Optical,
        WindowsDriveKind::Network => StockDriveIcon::Network,
    }
}

/// Fills each row's `icon`. `kinds` holds each drive row's kind by its root.
pub(super) fn fill_icons(rows: &mut [LocationInfo], kinds: &HashMap<String, WindowsDriveKind>) {
    let system_root = cmdr_fs::mount_layout::boot_volume_root();
    let favorite_ids: Vec<Option<String>> = rows
        .iter()
        .map(|row| {
            (row.category == LocationCategory::Favorite && is_under_root(&row.path, &system_root))
                .then(|| crate::icons::special_folders::icon_id_for_path(crate::volumes::os_mount_path(&row.path)))
                .flatten()
        })
        .collect();
    let wanted: Vec<String> = favorite_ids.iter().flatten().cloned().collect();
    let favorites = if wanted.is_empty() {
        HashMap::new()
    } else {
        crate::icons::get_icons_within(wanted, false, SHELL_ICON_WAIT)
    };
    for (row, favorite) in rows.iter_mut().zip(favorite_ids) {
        let stock = kinds.get(&row.path).map(|&kind| stock_icon_for(kind));
        row.icon = if row.id == DEFAULT_VOLUME_ID {
            system_drive_icon(SHELL_ICON_WAIT)
                .or_else(|| crate::icons::platform::stock_drive_icon(StockDriveIcon::Fixed))
        } else if let Some(stock) = stock {
            crate::icons::platform::stock_drive_icon(stock)
        } else {
            favorite.and_then(|id| favorites.get(&id).cloned())
        };
    }
}

/// Whether app path `path` lies on the drive rooted at `root` (`C:/`).
fn is_under_root(path: &str, root: &str) -> bool {
    path.get(..root.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(root))
}

/// The system drive's Shell icon, waiting at most `wait` for a lookup this call starts. One
/// that lands later is kept and republishes the volume list, so the row picks it up.
pub(super) fn system_drive_icon(wait: Duration) -> Option<String> {
    if let Some(icon) = SYSTEM_DRIVE_ICON.lock_ignore_poison().clone() {
        return Some(icon);
    }
    if SYSTEM_LOOKUP_RUNNING.swap(true, Ordering::SeqCst) {
        return None;
    }
    let (sender, receiver) = mpsc::channel();
    let gave_up = Arc::new(AtomicBool::new(false));
    let late = Arc::clone(&gave_up);
    let spawned = std::thread::Builder::new()
        .name("system_drive_icon".into())
        .stack_size(crate::icons::platform::PLACE_LOOKUP_STACK)
        .spawn(move || {
            let icon = crate::icons::platform::system_drive_icon();
            if let Some(icon) = &icon {
                *SYSTEM_DRIVE_ICON.lock_ignore_poison() = Some(icon.clone());
            }
            SYSTEM_LOOKUP_RUNNING.store(false, Ordering::SeqCst);
            if icon.is_some() && late.load(Ordering::SeqCst) {
                crate::volume_broadcast::emit_volumes_changed();
            }
            // The receiver is gone once the caller stopped waiting: the icon is kept anyway.
            let _ = sender.send(icon);
        });
    if let Err(e) = spawned {
        SYSTEM_LOOKUP_RUNNING.store(false, Ordering::SeqCst);
        log::warn!(target: "volumes", "Couldn't start the system drive's icon lookup: {e}");
        return None;
    }
    match receiver.recv_timeout(wait) {
        Ok(icon) => icon,
        Err(_) => {
            gave_up.store(true, Ordering::SeqCst);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drive_but_the_system_drive_gets_its_kinds_stock_icon() {
        for (kind, stock) in [
            (WindowsDriveKind::Fixed, StockDriveIcon::Fixed),
            (WindowsDriveKind::Subst, StockDriveIcon::Fixed),
            (WindowsDriveKind::Ram, StockDriveIcon::Ram),
            (WindowsDriveKind::Network, StockDriveIcon::Network),
            (WindowsDriveKind::Removable, StockDriveIcon::Removable),
            (WindowsDriveKind::Optical, StockDriveIcon::Optical),
        ] {
            assert_eq!(stock_icon_for(kind), stock, "{kind:?}");
        }
    }

    /// A share's row gets the network drive icon with no call to the server: the server name
    /// can't resolve, and the row still answers at once with the stock pixels.
    #[test]
    fn a_network_drive_gets_its_stock_icon_with_no_lookup() {
        let mut rows = vec![super::super::row(
            "smb-test",
            "share",
            std::path::Path::new("//cmdr-test.invalid/share/"),
            LocationCategory::Network,
        )];
        let kinds = HashMap::from([(rows[0].path.clone(), WindowsDriveKind::Network)]);
        let started = std::time::Instant::now();
        fill_icons(&mut rows, &kinds);
        assert!(started.elapsed() < SHELL_ICON_WAIT, "no wait on the share");
        assert!(rows[0].icon.is_some());
        assert_eq!(
            rows[0].icon,
            crate::icons::platform::stock_drive_icon(StockDriveIcon::Network)
        );
    }

    /// The system drive's row carries the Shell's own icon, not the stock fallback.
    #[test]
    fn the_system_drive_row_carries_the_shells_icon() {
        let shells = system_drive_icon(Duration::from_secs(30)).expect("the Shell draws the system drive");
        assert_ne!(
            Some(&shells),
            crate::icons::platform::stock_drive_icon(StockDriveIcon::Fixed).as_ref()
        );
        let locations = super::super::list_locations();
        let system = locations
            .iter()
            .find(|l| l.id == DEFAULT_VOLUME_ID)
            .expect("the system drive is listed");
        assert_eq!(system.icon.as_ref(), Some(&shells));
    }

    #[test]
    fn a_path_on_another_drive_isnt_under_the_system_root() {
        assert!(is_under_root("C:/Users/test_user_1/Desktop", "C:/"));
        assert!(is_under_root("c:/Users", "C:/"));
        assert!(!is_under_root("D:/Users/test_user_1/Desktop", "C:/"));
        assert!(!is_under_root("//testnas.local/home/Desktop", "C:/"));
    }
}
