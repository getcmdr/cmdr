//! Whether a drag out of a pane can hand other apps real files, decided once per session and
//! shared by every OS arm: macOS picks file URLs or file promises by it, Windows a file data
//! object or one only Cmdr reads.

use std::path::PathBuf;

use crate::file_system::volume::manager::path_routes_over_its_parent;

/// What a drag session can offer other apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionLocality {
    /// Real files another app can open by their native path.
    Local,
    /// Paths with no file behind them in the OS (a phone, search results, inside an archive or a
    /// repo's history): other apps get no file to open.
    Virtual,
}

/// Resolves a source volume id to its drag-session locality.
///
/// Keyed on `Volume::paths_are_os_visible()`: the question a drop target asks is whether a path
/// opens in ANOTHER app, not whether Cmdr reads it through `std::fs`. Local disks and OS-mounted
/// shares (direct SMB included, for as long as the share stays mounted alongside the smb2
/// session) stay local; protocol-only volumes (MTP, search-results), and a direct SMB share whose
/// mount has gone away under it, are virtual. An unknown or absent id (the back-compatible
/// default for callers that don't know their volume) resolves to `Local`.
///
/// ❌ Don't key this on `supports_local_fs_access()`: that asks a different question, and direct
/// SMB answers `false` to it while handing out perfectly openable paths. A virtual drag is
/// refused by most targets, so the mistake looks like "drag to Mail/browser silently does
/// nothing".
fn locality_for_volume(volume_id: Option<&str>) -> SessionLocality {
    let Some(volume_id) = volume_id else {
        return SessionLocality::Local;
    };
    match crate::file_system::volume::manager::get_volume_manager().get(volume_id) {
        Some(volume) if !volume.paths_are_os_visible() => SessionLocality::Virtual,
        _ => SessionLocality::Local,
    }
}

/// The locality of a session dragging `paths` (the rows as sent) from `source_volume_id`.
///
/// A path a ROUTE serves isn't a file the OS can open (it lives inside the `.zip`, or inside a
/// repo's virtual `.git` trees), so it forces a virtual session even though the source volume id
/// is the local parent drive. The `.zip` file itself IS a real local file, and so is a real file
/// under `.git`.
///
/// ❗ One locality for the whole session: a mixed session is impossible by construction (one
/// pane, one volume).
pub(super) fn session_locality(paths: &[PathBuf], source_volume_id: Option<&str>) -> SessionLocality {
    if paths.iter().any(|p| path_routes_over_its_parent(p)) {
        return SessionLocality::Virtual;
    }
    locality_for_volume(source_volume_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_system::volume::Volume;
    use crate::test_support::CapabilityStub;
    use std::sync::Arc;

    /// Registers a stub under a unique id and returns the locality the drag path derives for it.
    /// Ids are per-test so the process-wide manager can't make two tests collide.
    fn locality_of(id: &str, supports_local_fs_access: bool, paths_are_os_visible: bool) -> SessionLocality {
        let manager = crate::file_system::volume::manager::get_volume_manager();
        manager.register(
            id,
            Arc::new(CapabilityStub {
                supports_local_fs_access,
                paths_are_os_visible,
            }) as Arc<dyn Volume>,
        );
        let locality = locality_for_volume(Some(id));
        manager.unregister(id);
        locality
    }

    /// The direct-SMB shape: Cmdr's own I/O goes over smb2 (no `std::fs`), but the share stays
    /// OS-mounted, so other apps CAN open the paths. This must drag as a local session.
    #[test]
    fn os_visible_volume_drags_as_local_even_without_local_fs_access() {
        assert_eq!(locality_of("stub-smb-shaped", false, true), SessionLocality::Local);
    }

    /// The MTP shape: no local paths at all.
    #[test]
    fn volume_with_no_os_visible_paths_drags_as_virtual() {
        assert_eq!(locality_of("stub-mtp-shaped", false, false), SessionLocality::Virtual);
    }

    /// A plain local disk stays local.
    #[test]
    fn local_volume_drags_as_local() {
        assert_eq!(locality_of("stub-local", true, true), SessionLocality::Local);
    }

    /// An id nobody registered resolves to `Local`, the back-compatible default for callers that
    /// don't know their volume.
    #[test]
    fn unknown_volume_id_drags_as_local() {
        assert_eq!(
            locality_for_volume(Some("stub-never-registered")),
            SessionLocality::Local
        );
        assert_eq!(locality_for_volume(None), SessionLocality::Local);
    }

    /// A dragged item that a ROUTE serves has no file to hand out, so the session is virtual
    /// whatever the pane's volume says. Both namespaces, and neither of their real-file
    /// neighbours. macOS only, where it was written: the fixture's paths are POSIX-shaped.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_routed_path_forces_a_virtual_session_and_a_real_one_does_not() {
        use cmdr_git::test_fixtures::{Fixture, cleanup, temp_dir};

        let dir = temp_dir("drag_locality", "snapshot");
        let mut fixture = Fixture::init(dir.clone());
        fixture.commit_file("README.md", b"hello\n", "initial");
        crate::file_system::git::wiring::set_virtual_portal_enabled(true);

        // Inside a repo's history: no inode.
        assert_eq!(
            session_locality(&[dir.join(".git/branches/main/README.md")], Some("root")),
            SessionLocality::Virtual
        );
        // Inside a `.zip`.
        assert_eq!(
            session_locality(&[dir.join("bundle.zip/inner.txt")], Some("root")),
            SessionLocality::Virtual
        );

        // The neighbours that ARE real files on disk stay local: the `.zip` itself, a real entry
        // under `.git`, and an ordinary file.
        for real in [dir.join("bundle.zip"), dir.join(".git/config"), dir.join("README.md")] {
            assert_eq!(
                session_locality(std::slice::from_ref(&real), None),
                SessionLocality::Local,
                "{} is a real file on disk",
                real.display()
            );
        }

        // And with the portal off, a snapshot path is just a missing local path.
        crate::file_system::git::wiring::set_virtual_portal_enabled(false);
        assert_eq!(
            session_locality(&[dir.join(".git/branches/main/README.md")], None),
            SessionLocality::Local
        );
        crate::file_system::git::wiring::set_virtual_portal_enabled(true);

        cleanup(&dir);
    }
}
