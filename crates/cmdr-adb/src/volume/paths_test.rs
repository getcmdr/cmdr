//! Turning the paths the app addresses this volume with into device paths.
//!
//! The shared rules are `cmdr_fs::volume::remote_paths`' cells; these prove this
//! volume is wired to them with its own prefix, and pin the one refusal that is
//! this backend's own (a `..` above the root).

use std::path::Path;

use cmdr_fs::app_path::{AppPathBuf, PathFlavor};
use cmdr_fs::volume::{VolumeError, anchor_at_root};

use super::{device_file_name, join_device_path};
use crate::volume::AdbVolume;
use crate::volume::testing::{FIXTURE_SERIAL, detached_volume, fixture_app_path};

/// The prefix the fixture volume mints, spelled out so a cell reads as the app
/// would see it.
fn prefix() -> String {
    format!("adb://{FIXTURE_SERIAL}")
}

/// A bare POSIX app path (`/sdcard/DCIM`), a value any host holds.
fn posix(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Posix).expect("a POSIX app path")
}

/// What this volume makes of TEXT a `std::path` caller can still hand it through the
/// transition bridge (relative, `..`, empty): its own climb refusal, then `RemoteRoot`'s
/// `std::path` form, the same rules the typed [`AdbVolume::to_device_path`] runs, which no
/// typed app path can spell.
fn device_of_text(volume: &AdbVolume, text: &str) -> Result<String, VolumeError> {
    let refused = || VolumeError::NotFound(text.to_owned());
    if volume.climbs_above_the_root(text) {
        return Err(refused());
    }
    volume.paths.to_remote_path(Path::new(text)).ok_or_else(refused)
}

#[test]
fn every_spelling_of_the_root_is_the_root() {
    let volume = detached_volume();
    for spelling in ["", ".", "/"] {
        assert_eq!(device_of_text(&volume, spelling).unwrap(), "/", "{spelling:?}");
    }
    let root = cmdr_fs::volume::Volume::root(&volume);
    assert_eq!(root.as_str(), prefix(), "`root()` is the bare prefix");
    assert_eq!(volume.to_device_path(root).unwrap(), "/");
    assert_eq!(volume.to_device_path(&fixture_app_path("/")).unwrap(), "/");
    assert_eq!(fixture_app_path("/").as_str(), format!("{}/", prefix()));
}

#[test]
fn a_prefixed_and_a_relative_path_land_on_the_same_device_path() {
    let volume = detached_volume();
    let prefixed = fixture_app_path("/sdcard/DCIM");
    assert_eq!(volume.to_device_path(&prefixed).unwrap(), "/sdcard/DCIM");
    assert_eq!(device_of_text(&volume, "sdcard/DCIM").unwrap(), "/sdcard/DCIM");
    assert_eq!(
        device_of_text(&volume, &format!("{prefixed}/")).unwrap(),
        "/sdcard/DCIM"
    );
}

/// What a listing hands out is what a pane passes back, so the two directions
/// have to agree, root included.
#[test]
fn the_app_spelling_round_trips_and_the_root_has_no_trailing_slash() {
    let volume = detached_volume();
    assert_eq!(volume.app_text_of("/"), Some(prefix()));
    assert_eq!(
        volume.app_text_of("/sdcard/DCIM"),
        Some(format!("{}/sdcard/DCIM", prefix()))
    );
    assert_eq!(
        volume.display_path_for(&fixture_app_path("/sdcard/DCIM")),
        Some(fixture_app_path("/sdcard/DCIM"))
    );
    assert_eq!(
        volume
            .display_path_for(&fixture_app_path("/"))
            .map(|root| root.as_str().to_owned()),
        Some(prefix()),
        "the listing host hears the root folder as `root()` spells it"
    );
}

/// ❗ **The transfer dialog's box is volume-relative** (`/sdcard`), and the app
/// anchors it against `Volume::root` before the volume sees it. It has to land on
/// the device's `/sdcard`, and a pane's already prefixed path has to come through
/// unchanged.
#[test]
fn a_dialog_path_anchored_at_the_root_lands_on_the_device_path() {
    let volume = detached_volume();
    let root = cmdr_fs::volume::Volume::root(&volume);
    let anchored = anchor_at_root(root, "/sdcard").expect("a volume-relative box anchors");
    assert_eq!(anchored.as_str(), format!("{}/sdcard", prefix()));
    assert_eq!(volume.to_device_path(&anchored).unwrap(), "/sdcard");
    let pane = format!("{}/sdcard", prefix());
    assert_eq!(anchor_at_root(root, &pane).expect("a pane path anchors").as_str(), pane);
}

#[test]
fn dot_dot_is_resolved_lexically() {
    let volume = detached_volume();
    assert_eq!(
        device_of_text(&volume, &format!("{}/sdcard/DCIM/../Pictures", prefix())).unwrap(),
        "/sdcard/Pictures"
    );
    assert_eq!(
        device_of_text(&volume, &format!("{}/sdcard/..", prefix())).unwrap(),
        "/"
    );
}

#[test]
fn a_path_climbing_above_the_root_is_refused_not_anchored() {
    let volume = detached_volume();
    let prefixed_escape = format!("{}/sdcard/../../etc/passwd", prefix());
    for escape in ["..", "../etc", "sdcard/../../etc", prefixed_escape.as_str()] {
        let outcome = device_of_text(&volume, escape);
        assert!(
            matches!(outcome, Err(VolumeError::NotFound(ref p)) if p == escape),
            "{escape}: {outcome:?}"
        );
    }
}

/// A bare `/sdcard` reaching the volume means a caller skipped anchoring, and
/// anchoring it here would hide that. Another device's path is never ours.
#[test]
fn a_bare_device_path_and_another_devices_path_are_refused() {
    let volume = detached_volume();
    for foreign in [
        "/sdcard/DCIM",
        "/etc/passwd",
        "adb://SOMEONE-ELSE/sdcard",
        "mtp://1-2/65537",
    ] {
        assert!(
            matches!(volume.to_device_path(&posix(foreign)), Err(VolumeError::NotFound(_))),
            "{foreign}"
        );
    }
    let longer_serial = format!("{}0/sdcard", prefix());
    assert!(
        volume.to_device_path(&posix(&longer_serial)).is_err(),
        "a serial that merely starts with ours is another device"
    );
}

/// ❗ Device paths are POSIX on every host: a `\` or a `:` a phone lists is part of
/// a name, never a separator or a drive (a `std::path` join on Windows sent the
/// device `\sdcard\…`, which it refused with `EROFS`).
#[test]
fn a_backslash_or_a_colon_stays_inside_one_device_name() {
    let volume = detached_volume();
    let odd = fixture_app_path("/sdcard/a\\b:c.txt");
    assert_eq!(volume.to_device_path(&odd).unwrap(), "/sdcard/a\\b:c.txt");
    assert_eq!(device_file_name("/sdcard/a\\b:c.txt"), Some("a\\b:c.txt"));
    assert_eq!(device_file_name("/"), None);
}

#[test]
fn joining_never_doubles_the_root_slash() {
    assert_eq!(join_device_path("/", "sdcard"), "/sdcard");
    assert_eq!(join_device_path("/sdcard", "DCIM"), "/sdcard/DCIM");
}

/// #1310: a name the device lists with an escape-range code point is reached as listed: the
/// row's text through the app's door (the listed-text one, which the wire and the bridge
/// spell alike), and what the listing-cache patcher spells comes back the same value.
#[test]
fn a_listed_escape_range_name_reaches_the_device_as_listed() {
    let volume = detached_volume();
    let listed = "/sdcard/x\u{10FEFF}/caf\u{10FEC3}\u{10FEA9}";
    let row = volume.app_text_of(listed).expect("a row");
    let sent = AppPathBuf::from_listed_text(&row, PathFlavor::Posix).expect("an app path");
    assert_eq!(volume.to_device_path(&sent).unwrap(), listed);
    assert_eq!(volume.display_path_for(&sent), Some(sent.clone()));
}
