//! Windows gives each volume its own drive letter (or a folder mount anywhere), so no prefix
//! names one yet.

pub(crate) fn user_mount_prefixes() -> &'static [&'static str] {
    &[]
}

/// The system drive (`%SystemDrive%`, which Windows sets for every process), as its app root.
pub(crate) fn boot_volume_root() -> String {
    super::super::system_drive_root_from(std::env::var("SystemDrive").ok().as_deref())
}
