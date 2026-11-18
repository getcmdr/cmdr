//! Linux: systemd's machine id (machine-id(5)).

/// Read the systemd machine id, falling back to the D-Bus copy on systems without systemd.
///
/// It changes on reinstall and is shared by cloned VMs, so those count as new or the same
/// device respectively. Accepted (David, 2026-11-05, #980).
pub(crate) fn read_platform_uuid() -> Option<String> {
    use std::path::Path;
    let id = super::super::read_machine_id(&[Path::new("/etc/machine-id"), Path::new("/var/lib/dbus/machine-id")]);
    if id.is_none() {
        log::warn!("No usable machine-id found; license checks will carry no device id");
    }
    id
}
