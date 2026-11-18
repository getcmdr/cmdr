//! macOS mounts every volume but the boot disk under `/Volumes/`.

pub(crate) fn user_mount_prefixes() -> &'static [&'static str] {
    &["/Volumes/"]
}

/// The boot disk is the whole filesystem at `/`.
pub(crate) fn boot_volume_root() -> String {
    "/".to_string()
}
