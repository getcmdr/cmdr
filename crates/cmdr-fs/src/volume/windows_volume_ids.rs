//! Which id a Windows drive, folder mount, or UNC share gets (plan D8): the macOS and Linux
//! ladder, fed what Windows reports. Pure, so it's tested on every OS; the Win32 reads that
//! gather the facts live in the app's `volumes/platform/windows/` arm.
//!
//! A volume's id is the disk's identity and its root is only its address:
//!
//! 1. The system drive is `root` ([`DEFAULT_VOLUME_ID`]), whatever its GUID.
//! 2. A mapped network drive (`Z:`) and the share's own UNC root (`//server/share/`) key on
//!    `(server, port, share)` through [`smb_volume_id`](super::smb_volume_id), so they and Cmdr's direct SMB
//!    connection to the share are ONE volume, as an OS mount and the direct backend are on
//!    macOS. Windows' redirector speaks SMB on 445; a mapped drive names no other port. A
//!    drive mapped to a folder BELOW the share (`net use W: \\server\share\sub`) is rooted at
//!    that folder, so it keys on the folder too ([`smb_folder_volume_id`], #1481).
//! 3. A local drive or folder mount keys on its volume GUID through [`local_volume_id`]. The
//!    mount manager keeps one GUID per partition per machine (`HKLM\SYSTEM\MountedDevices`),
//!    so a stick on another letter keeps its id, and two cloned disks don't collide.
//! 4. Anything else (a `subst` drive, whose `GetVolumeNameForVolumeMountPointW` fails with
//!    `ERROR_INVALID_PARAMETER`) keys on its root path through [`path_volume_id`].

use super::DEFAULT_VOLUME_ID;
use super::ids::{local_volume_id, path_volume_id, smb_folder_volume_id};

/// The port Windows' SMB redirector reaches every mapped drive and UNC share on.
pub const WINDOWS_SMB_PORT: u16 = 445;

/// What Windows says a drive root or folder mount is, the input to [`windows_volume_id`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsVolumeIdentity {
    /// A local volume, with its GUID path (`\\?\Volume{7ee27290-…}\`) as
    /// `GetVolumeNameForVolumeMountPointW` or `FindFirstVolumeW` gave it.
    Local {
        /// The volume GUID path.
        guid_path: String,
    },
    /// A network share: a mapped drive's `WNetGetConnectionW` target (`\\server\share`) or a
    /// UNC root's app path (`//server/share/`).
    Share {
        /// Either spelling of the share.
        unc: String,
    },
    /// Neither: a `subst` drive, or a drive Windows wouldn't name.
    Unnamed,
}

/// The volume id for the drive, folder mount, or share at the app root `root` (`D:/`,
/// `C:/mnt/data`, `//server/share/`), following the ladder in the module docs.
/// `system_root` is the system drive's app root (`C:/`).
pub fn windows_volume_id(root: &str, identity: &WindowsVolumeIdentity, system_root: &str) -> String {
    if same_root(root, system_root) {
        return DEFAULT_VOLUME_ID.to_string();
    }
    match identity {
        WindowsVolumeIdentity::Share { unc } => match unc_target(unc) {
            Some((server, share, folder)) => smb_folder_volume_id(&server, WINDOWS_SMB_PORT, &share, &folder),
            None => path_volume_id(root),
        },
        WindowsVolumeIdentity::Local { guid_path } => match volume_guid(guid_path) {
            Some(guid) => local_volume_id(Some(guid), root),
            None => path_volume_id(root),
        },
        WindowsVolumeIdentity::Unnamed => path_volume_id(root),
    }
}

/// The GUID inside a volume GUID path (`\\?\Volume{7ee27290-b54a-45d8-bcfd-9f3bd4318a54}\`
/// gives `7ee27290-b54a-45d8-bcfd-9f3bd4318a54`), or `None` for anything else.
pub fn volume_guid(guid_path: &str) -> Option<&str> {
    // ASCII-lowercased for the search only: same byte offsets, any spelling of `Volume`.
    let start = guid_path.to_ascii_lowercase().find("volume{")? + "volume{".len();
    let rest = &guid_path[start..];
    let guid = &rest[..rest.find('}')?];
    let well_formed = guid.len() == 36 && guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    well_formed.then_some(guid)
}

/// The server and share of a UNC spelling, either separator, with or without a trailing one
/// or a path below the share (`\\server\share`, `//server/share/x`), or `None` for anything
/// that names no share. `\\?\` and `\\.\` are Win32's device namespaces, never servers.
pub fn unc_server_share(unc: &str) -> Option<(String, String)> {
    let slashed = unc.replace('\\', "/");
    let rest = slashed.strip_prefix("//")?;
    let rest = rest.strip_prefix("?/UNC/").unwrap_or(rest);
    let mut parts = rest.split('/');
    let server = parts.next().filter(|s| !s.is_empty() && *s != "?" && *s != ".")?;
    let share = parts.next().filter(|s| !s.is_empty())?;
    Some((server.to_string(), share.to_string()))
}

/// The app root (`//server/share/`) of a UNC spelling ([`unc_server_share`]'s input).
pub fn unc_app_root(unc: &str) -> Option<String> {
    unc_server_share(unc).map(|(server, share)| format!("//{server}/{share}/"))
}

/// The server, share, and folder below it (`a/b`, empty for the share itself) a UNC spelling
/// names, as [`unc_server_share`] reads it.
fn unc_target(unc: &str) -> Option<(String, String, String)> {
    let (server, share) = unc_server_share(unc)?;
    let slashed = unc.replace('\\', "/");
    let rest = slashed.strip_prefix("//")?;
    let rest = rest.strip_prefix("?/UNC/").unwrap_or(rest);
    let folder = rest
        .split('/')
        .skip(2)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    Some((server, share, folder))
}

/// The app spelling of what a mapped drive maps: `//server/share/` for a whole share,
/// `//server/share/sub/dir` for a folder below it.
fn unc_app_spelling(unc: &str) -> Option<String> {
    let (server, share, folder) = unc_target(unc)?;
    Some(if folder.is_empty() {
        format!("//{server}/{share}/")
    } else {
        format!("//{server}/{share}/{folder}")
    })
}

/// A mapped network drive at `root` (`Z:/`), mapped to `unc` (`WNetGetConnectionW`'s
/// `\\server\share` or `\\server\share\sub\dir`; `None` when Windows wouldn't say). Pure, so
/// what a mapping means is tested on every OS.
///
/// Only a mapping of the share's root records the share's root as its alias
/// ([`WindowsDrive::share_root`]): below a folder mapping, `W:/x` is the folder's `x`, so the
/// share's root is no second address of it, and a UNC path there stays the share's own volume.
pub fn mapped_drive(root: &str, unc: Option<&str>, system_root: &str) -> WindowsDrive {
    let remote = unc.and_then(unc_app_spelling);
    let identity = unc.map_or(WindowsVolumeIdentity::Unnamed, |unc| WindowsVolumeIdentity::Share {
        unc: unc.to_string(),
    });
    WindowsDrive {
        id: windows_volume_id(root, &identity, system_root),
        root: root.to_string(),
        kind: WindowsDriveKind::Network,
        label: remote
            .as_deref()
            .and_then(|remote| remote.rsplit('/').find(|name| !name.is_empty()))
            .map(str::to_string),
        share_root: remote.clone().filter(|remote| remote.ends_with('/')),
        remote,
        is_alias: false,
        fs_type: None,
        read_only: false,
    }
}

/// The folder a `subst` drive points at, from its `QueryDosDeviceW` target (`\??\C:\Windows`),
/// as an app path (`C:/Windows`). `None` for a target that isn't a `subst` (a real drive names
/// a device, `\Device\HarddiskVolume4`).
pub fn subst_target(dos_device: &str) -> Option<String> {
    let native = dos_device.strip_prefix(r"\??\")?;
    let slashed = native.replace('\\', "/");
    let bytes = slashed.as_bytes();
    let is_drive_path = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if !is_drive_path {
        return None;
    }
    let letter = char::from(bytes[0].to_ascii_uppercase());
    let rest = slashed[2..].trim_end_matches('/');
    Some(if rest.is_empty() {
        format!("{letter}:/")
    } else {
        format!("{letter}:{rest}")
    })
}

/// What kind of thing a Windows volume root is, from `GetDriveTypeW` plus the `subst` and
/// folder-mount reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsDriveKind {
    /// A fixed disk (`DRIVE_FIXED`). A USB SSD can report this too (the builder's `D:`).
    Fixed,
    /// A removable drive (`DRIVE_REMOVABLE`): a USB stick, an SD card.
    Removable,
    /// An optical drive (`DRIVE_CDROM`).
    Optical,
    /// A RAM disk (`DRIVE_RAMDISK`).
    Ram,
    /// A mapped network drive or a UNC share root (`DRIVE_REMOTE`).
    Network,
    /// A `subst` drive: a letter for a folder on another drive.
    Subst,
}

/// One volume root Windows lists right now, with the id [`windows_volume_id`] gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsDrive {
    /// Its app root: `D:/`, a folder mount's `C:/mnt/data`, or a share's `//server/share/`.
    pub root: String,
    /// Its volume id.
    pub id: String,
    /// What it is.
    pub kind: WindowsDriveKind,
    /// The volume label (`build`), or for a share its name; `None` when it has none.
    pub label: Option<String>,
    /// For a drive mapped to a whole share, the share's app root (`//server/share/`), which
    /// registers as an alias root of the same volume. `None` for a drive mapped to a folder
    /// below the share ([`mapped_drive`]).
    pub share_root: Option<String>,
    /// For a mapped drive, what it maps, as an app path: `//server/share/` for a whole share,
    /// `//server/share/sub` for a folder below it. `None` when Windows wouldn't say.
    pub remote: Option<String>,
    /// Whether this root is only an alias (a mapped share's UNC spelling), a second address
    /// of a volume whose switcher row is the mapped drive.
    pub is_alias: bool,
    /// The filesystem Windows reports (`NTFS`, `exFAT`), `None` for a share or when unknown.
    pub fs_type: Option<String>,
    /// Whether the volume is mounted read-only (`FILE_READ_ONLY_VOLUME`).
    pub read_only: bool,
}

impl WindowsDrive {
    /// The drive letter, for a drive root (`D:/` gives `D`); `None` for a folder mount or share.
    pub fn letter(&self) -> Option<char> {
        let bytes = self.root.as_bytes();
        (bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..] == b":/").then(|| char::from(bytes[0]))
    }

    /// The name the switcher shows, the way Explorer's This PC reads (the words around it are
    /// the frontend's): `build (D:)` for a labeled drive, `D:` for one with no label, `media
    /// (\\server) (Z:)` for a mapped share and `sub (\\server\share) (W:)` for a mapped folder
    /// (the mapped place's name, then where it sits), `\\server\share` for a bare share, and
    /// the label or last folder name for a folder mount.
    pub fn display_name(&self) -> String {
        let label = self.label.as_deref().filter(|l| !l.trim().is_empty());
        match (self.letter(), self.kind) {
            (Some(letter), WindowsDriveKind::Network) => {
                let remote = self.remote.as_deref().map(|r| r.trim_end_matches('/'));
                match remote.and_then(|remote| remote.rsplit_once('/')) {
                    Some((parent, name)) if !name.is_empty() && parent.len() > 2 => {
                        let parent = parent.replace('/', r"\");
                        format!("{name} ({parent}) ({letter}:)")
                    }
                    _ => format!("{letter}:"),
                }
            }
            (Some(letter), _) => match label {
                Some(label) => format!("{label} ({letter}:)"),
                None => format!("{letter}:"),
            },
            (None, WindowsDriveKind::Network) => match unc_server_share(&self.root) {
                Some((server, share)) => format!(r"\\{server}\{share}"),
                None => self.root.clone(),
            },
            (None, _) => label
                .map(str::to_string)
                .or_else(|| self.root.rsplit('/').find(|name| !name.is_empty()).map(str::to_string))
                .unwrap_or_else(|| self.root.clone()),
        }
    }
}

/// The drive whose root holds the app path `path`, the longest root winning (a folder mount
/// `C:/mnt/data` over `C:/`), by whole components and folding case, as Windows' filesystems
/// do. `None` for a path no listed root holds.
pub fn covering_drive<'a>(drives: &'a [WindowsDrive], path: &str) -> Option<&'a WindowsDrive> {
    let path = path.to_lowercase();
    drives
        .iter()
        .filter(|drive| {
            let root = drive.root.to_lowercase();
            let root = root.trim_end_matches('/');
            path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|drive| drive.root.trim_end_matches('/').len())
}

/// One place the mount manager mounts a local volume: a drive letter's root (`D:/`) or a
/// folder (`C:/mnt/data`), with the GUID path of the volume there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeMountPath<'a> {
    /// The app root: `D:/` or `C:/mnt/data`.
    pub root: &'a str,
    /// The volume GUID path (`\\?\Volume{…}\`) of the volume mounted there.
    pub guid_path: &'a str,
}

/// Whether `candidate` is its volume's ONE root among `mounts` (every letter and folder
/// mount on the machine): its drive letter when it has one, else its shortest folder, ties
/// by name. Every other place the same volume is mounted is a plain folder of the volume
/// holding it, as Explorer's This PC lists it (one row per drive; a folder mount shows as a
/// folder of its parent).
///
/// Why one root (#1385): a volume id names one `Volume` at one root. With a folder mount as
/// a second root of `D:`'s id, a path below it resolved to `D:`'s volume while its text sat
/// on `C:`, so the pane's commit dropped it as a foreign listing and the tab kept the old
/// folder the transfer dialog then offered.
pub fn is_volume_root(candidate: &VolumeMountPath<'_>, mounts: &[VolumeMountPath<'_>]) -> bool {
    let rank = |mount: &VolumeMountPath<'_>| {
        let is_letter = mount.root.len() == 3 && mount.root.ends_with(":/");
        (!is_letter, mount.root.len(), mount.root.to_lowercase())
    };
    let volume = |mount: &VolumeMountPath<'_>| {
        volume_guid(mount.guid_path).map_or_else(|| mount.guid_path.to_lowercase(), str::to_lowercase)
    };
    let own = volume(candidate);
    let own_rank = rank(candidate);
    !mounts
        .iter()
        .any(|other| volume(other) == own && rank(other) < own_rank)
}

/// Whether two app roots name one place: drive letters and folder names fold case on
/// Windows' filesystems, and a trailing `/` doesn't count.
fn same_root(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches('/').to_lowercase();
    trim(a) == trim(b)
}

#[cfg(test)]
#[path = "windows_volume_ids_test.rs"]
mod tests;
