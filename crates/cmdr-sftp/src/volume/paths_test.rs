//! Turning the paths the app addresses this volume with into remote paths.
//!
//! The rules themselves are `cmdr_fs::volume::remote_paths`' cells; these prove
//! this volume is wired to them, with its own prefix and its own root.

use super::*;
use crate::volume::test_support::*;
use crate::volume::testing::app_at;
use cmdr_fs::app_path::PathFlavor;
use cmdr_fs::volume::Volume;

/// A bare POSIX app path (`/srv/data/photos`), a value any host holds.
fn posix(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Posix).expect("a POSIX app path")
}

/// What this volume's `RemoteRoot` makes of TEXT a `std::path` caller can still hand the
/// volume through the transition bridge (relative, `..`, empty): the same rules the typed
/// [`SftpVolume::to_remote_path`] runs, which no typed app path can spell.
fn remote_of_text(vol: &SftpVolume, text: &str) -> Result<String, VolumeError> {
    vol.root
        .to_remote_path(Path::new(text))
        .ok_or_else(|| VolumeError::NotFound(text.to_owned()))
}

/// The prefix this crate's test volume mints, spelled out so a cell reads as the
/// app would see it.
const PREFIX: &str = "sftp://test_user_1@127.0.0.1:12599";

#[test]
fn the_three_spellings_of_the_root_all_mean_the_root() {
    let vol = make_test_volume();
    assert_eq!(remote_of_text(&vol, "").unwrap(), "/srv/data");
    assert_eq!(remote_of_text(&vol, ".").unwrap(), "/srv/data");
    assert_eq!(vol.to_remote_path(&app_at(&vol, "/")).unwrap(), "/srv/data");
}

#[test]
fn a_relative_path_hangs_off_the_root() {
    let vol = make_test_volume();
    assert_eq!(remote_of_text(&vol, "photos").unwrap(), "/srv/data/photos");
    assert_eq!(
        remote_of_text(&vol, "photos/2026/trip.jpg").unwrap(),
        "/srv/data/photos/2026/trip.jpg"
    );
}

/// The two spellings the app uses land on one server path, and the app-facing
/// one comes back out.
#[test]
fn a_prefixed_and_a_relative_path_land_on_the_same_server_path_and_round_trip() {
    let vol = make_test_volume();
    let prefixed = vol
        .to_remote_path(&app_at(&vol, &format!("{PREFIX}/srv/data/photos")))
        .expect("a path under this volume's own prefix is on this volume");
    assert_eq!(prefixed, "/srv/data/photos");
    assert_eq!(prefixed, remote_of_text(&vol, "photos").unwrap());
    assert_eq!(
        vol.display_path_for(&app_at(&vol, "photos")),
        Some(app_at(&vol, &format!("{PREFIX}/srv/data/photos"))),
        "what the listing-cache patcher spells is what a pane holds"
    );
}

/// ❗ **A bare server-absolute path is refused, not anchored.**
///
/// Anchoring turns `/etc/passwd` into `/srv/data/etc/passwd`, which is a real
/// path on a real server and quietly the wrong one. And a bare path that IS
/// under the root buys only the `root_anchored` hole: five app sites join
/// anything not under the root onto it, so `/srv/data/x` would arrive doubled
/// and strip back to a real, wrong server path.
#[test]
fn a_bare_server_absolute_path_is_refused() {
    let vol = make_test_volume();
    for bare in ["/etc/passwd", "/srv", "/srv/data/photos"] {
        assert!(
            matches!(vol.to_remote_path(&posix(bare)), Err(VolumeError::NotFound(_))),
            "a path with no prefix names nothing on this volume: {bare}"
        );
    }
}

#[test]
fn a_prefixed_path_outside_the_root_is_refused() {
    let vol = make_test_volume();
    assert!(matches!(
        vol.to_remote_path(&app_at(&vol, &format!("{PREFIX}/etc/passwd"))),
        Err(VolumeError::NotFound(_))
    ));
}

/// Another account on the same server is another volume, and its paths are not
/// this one's.
#[test]
fn another_accounts_prefix_is_refused() {
    let vol = make_test_volume();
    assert!(matches!(
        vol.to_remote_path(&app_at(&vol, "sftp://test_user_2@127.0.0.1:12599/srv/data/photos")),
        Err(VolumeError::NotFound(_))
    ));
}

#[test]
fn the_root_is_matched_by_whole_components() {
    // The trap: a sibling directory whose name merely starts with the root's.
    // A raw string prefix compare strips `/srv/data` off `/srv/data-1/photos`
    // and sends the server `-1/photos`, which is a real name on a real server.
    let vol = make_test_volume();
    assert!(matches!(
        vol.to_remote_path(&app_at(&vol, &format!("{PREFIX}/srv/data-1/photos"))),
        Err(VolumeError::NotFound(_))
    ));
    assert!(matches!(
        vol.to_remote_path(&app_at(&vol, &format!("{PREFIX}/srv/dataX"))),
        Err(VolumeError::NotFound(_))
    ));
}

#[test]
fn a_volume_rooted_at_the_server_root_reaches_everything() {
    // Rooting at `/` is a legitimate choice, and then every path under the
    // prefix is inside the root by definition.
    let vol = make_test_volume_at("/");
    assert_eq!(
        vol.to_remote_path(&app_at(&vol, &format!("{PREFIX}/etc/passwd")))
            .unwrap(),
        "/etc/passwd"
    );
    assert_eq!(remote_of_text(&vol, "etc/passwd").unwrap(), "/etc/passwd");
    assert_eq!(vol.to_remote_path(&app_at(&vol, "/")).unwrap(), "/");
}

#[test]
fn a_relative_path_cannot_climb_out_of_the_root() {
    // `..` in a relative path is the same escape by another spelling, and the
    // server would happily resolve it.
    let vol = make_test_volume();
    assert!(matches!(
        remote_of_text(&vol, "../secrets"),
        Err(VolumeError::NotFound(_))
    ));
    assert!(matches!(
        remote_of_text(&vol, "photos/../../secrets"),
        Err(VolumeError::NotFound(_))
    ));
}

/// ❗ The root the app sees carries the scheme, which is what makes a restored
/// tab, a favorite, and a copied path resolve back to this server rather than to
/// the boot disk.
#[test]
fn the_volume_root_is_the_app_spelling() {
    let vol = make_test_volume();
    assert_eq!(vol.root().as_str(), format!("{PREFIX}{TEST_ROOT}"));
}

/// ❗ **A name Cmdr creates goes out composed (NFC).** macOS hands out many local
/// names decomposed, and OpenSSH on Linux stores the bytes as sent, where web
/// servers, PHP, and scripts match bytes: a decomposed `café.jpg` would look
/// right and break every link to it. The engine respells a new name through
/// `Volume::spell_new_name`; this pins the answer it reads.
#[test]
fn a_new_name_goes_out_composed() {
    let vol = make_test_volume();
    assert!(vol.composes_new_names());
    assert_eq!(vol.spell_new_name("cafe\u{301}.jpg"), "caf\u{e9}.jpg");
    assert_eq!(vol.spell_new_name("caf\u{e9}.jpg"), "caf\u{e9}.jpg");
}

/// ❗ **A path to an EXISTING entry keeps its exact bytes.** A read, a delete,
/// or an overwrite of a decomposed name the server holds must address THAT
/// name: a composed path would miss it, or hit a composed twin beside it.
#[test]
fn a_path_to_an_existing_entry_keeps_its_exact_bytes() {
    let vol = make_test_volume();
    assert_eq!(
        vol.to_remote_path(&app_at(&vol, "fo\u{301}to\u{301}k/cafe\u{301}.jpg"))
            .unwrap(),
        "/srv/data/fo\u{301}to\u{301}k/cafe\u{301}.jpg"
    );
}

/// #1310: a name the server lists with an escape-range code point reaches it as listed: the
/// row's text through the app's door (the listed-text one, which the wire and the bridge
/// spell alike), and what the listing-cache patcher spells comes back the same value.
#[test]
fn a_listed_escape_range_name_reaches_the_server_as_listed() {
    let vol = make_test_volume();
    let listed = "/srv/data/x\u{10FEFF}/caf\u{10FEC3}\u{10FEA9}";
    let row = vol.app_text_of(listed).expect("a row");
    let sent = AppPathBuf::from_listed_text(&row, PathFlavor::Posix).expect("an app path");
    assert_eq!(vol.to_remote_path(&sent).unwrap(), listed);
    assert_eq!(vol.display_path_for(&sent), Some(sent.clone()));
}
