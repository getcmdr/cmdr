//! Go to path over a real scratch disk, and the typed resolver's app-side cells: the
//! registry's timeout on a hung mount and an agent's refusal above the root. The
//! symlink cells are `wire_shape_tests.rs`.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathArithmetic};
use cmdr_fs::typed_path::{TypedBy, TypedPathRefusal};
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{Volume, VolumeError};
use cmdr_local::LocalVolume;

use super::*;
use crate::test_support::{AppTestDir, WedgedVolume};

/// The local disk as the registry would hand it, without the registry.
pub(super) fn disk() -> AppHost {
    let root = AppPathBuf::from_native(Path::new(if cfg!(windows) { "C:\\" } else { "/" })).expect("a root");
    let volume: Arc<dyn Volume> = Arc::new(LocalVolume::local_folder_at("Test", root, VolumeHost::detached()));
    AppHost::over(move |_| Some(volume.clone()), Duration::from_secs(10))
}

pub(super) async fn go(input: &str, base: &AppPath) -> GoToPathResolution {
    resolve_over(input, base.as_str(), &disk()).await
}

/// `path` as a person types or pastes it: spelled the way `TEMP` spells the scratch root,
/// so on a Windows `TEMP` holding an 8.3 short name (`C:\Users\RUNNER~1\…`, the hosted CI
/// runner's) the cells here type that short spelling, which listings never show. That's
/// #1337's case, and these cells fail on it until typed text is spelled long; elsewhere
/// the two spellings are one.
fn native(path: &Path) -> String {
    let typed = match path.strip_prefix(cmdr_fs::testing::scratch_root()) {
        // allowed-fixed-temp-dir: no fixture here, only the OS's own spelling of the scratch root
        Ok(rest) => std::env::temp_dir().join(rest),
        Err(_) => path.to_path_buf(),
    };
    typed.to_string_lossy().into_owned()
}

fn app(dir: &AppTestDir, rel: &str) -> String {
    let mut out = dir.app_path().to_owned();
    for name in rel.split('/').filter(|name| !name.is_empty()) {
        out = out
            .join(cmdr_fs::app_path::AppName::from_app_spelling(name).expect("a name"))
            .expect("joins");
    }
    out.into_string()
}

fn folder(path: String) -> GoToPathResolution {
    GoToPathResolution::Directory {
        path,
        crossed_link: false,
    }
}

#[tokio::test]
async fn existing_dir_resolves_to_directory() {
    let dir = AppTestDir::new("go_to_dir");
    fs::create_dir(dir.native().join("sub")).expect("create sub");
    let res = go(&native(&dir.native().join("sub")), dir.app_path()).await;
    assert_eq!(res, folder(app(&dir, "sub")));
}

#[tokio::test]
async fn typing_a_path_into_a_zip_enters_it_as_a_directory() {
    let dir = AppTestDir::new("go_to_zip");
    let zip = dir.native().join("bundle.zip");
    // A zip start-of-file signature is enough for the boundary magic check.
    fs::write(&zip, b"PK\x03\x04rest-of-archive").expect("write zip");

    // An inner path that doesn't exist on disk still enters the archive.
    let res = go(&native(&zip.join("docs")), dir.app_path()).await;
    assert_eq!(res, folder(app(&dir, "bundle.zip/docs")));

    // Typing the `.zip` path itself also enters it (lists the archive root).
    let res_root = go(&native(&zip), dir.app_path()).await;
    assert!(matches!(res_root, GoToPathResolution::Directory { .. }));
}

#[tokio::test]
async fn a_directory_named_like_a_zip_does_not_enter_as_an_archive() {
    let dir = AppTestDir::new("go_to_fake_zip");
    fs::create_dir(dir.native().join("foo.zip")).expect("create dir named foo.zip");

    // A non-existent child: the archive branch would wrongly return
    // `Directory`; a real directory correctly falls to `NearestAncestor`.
    let res = go(&native(&dir.native().join("foo.zip/ghost")), dir.app_path()).await;
    assert!(matches!(res, GoToPathResolution::NearestAncestor { .. }));
}

#[tokio::test]
async fn existing_file_resolves_to_file_with_parent_and_name() {
    let dir = AppTestDir::new("go_to_file");
    fs::write(dir.native().join("notes.txt"), b"hi").expect("write file");

    let res = go(&native(&dir.native().join("notes.txt")), dir.app_path()).await;
    assert_eq!(
        res,
        GoToPathResolution::File {
            path: app(&dir, "notes.txt"),
            parent_dir: app(&dir, ""),
            file_name: "notes.txt".to_string(),
            crossed_link: false,
        }
    );
}

#[tokio::test]
async fn tilde_expands_to_home() {
    // `~` alone expands to the home dir, which exists, so it resolves to a Directory.
    let home = AppPathBuf::from_native(&dirs::home_dir().expect("home dir")).expect("an app path");
    let base = AppTestDir::new("go_to_tilde");
    assert_eq!(go("~", base.app_path()).await, folder(home.into_string()));
}

#[tokio::test]
async fn relative_input_joins_against_base_dir() {
    let dir = AppTestDir::new("go_to_relative");
    fs::create_dir(dir.native().join("child")).expect("create child");
    assert_eq!(go("child", dir.app_path()).await, folder(app(&dir, "child")));
}

#[tokio::test]
async fn a_leading_dot_and_dot_dot_step_up_from_the_pane() {
    let dir = AppTestDir::new("go_to_dots");
    fs::create_dir_all(dir.native().join("a/b")).expect("create a/b");

    // From base `a/b`, `./../.` lands back on `a`.
    let base = AppPathBuf::from_wire(&app(&dir, "a/b")).expect("an app path");
    assert_eq!(go("./../.", &base).await, folder(app(&dir, "a")));
}

#[tokio::test]
async fn dotdot_past_a_nonexistent_middle_segment_is_no_link() {
    // `<dir>/nope/..` is `<dir>`, which exists, though `nope` never did: a missing name is
    // no link, so the volume answers the lexical parent (canonicalize would have failed).
    let dir = AppTestDir::new("go_to_nope_dot_dot");
    let input = format!("{}/nope/..", native(dir.native()));
    assert_eq!(go(&input, dir.app_path()).await, folder(app(&dir, "")));
}

#[tokio::test]
async fn deep_nonexistent_path_falls_back_to_nearest_ancestor() {
    let dir = AppTestDir::new("go_to_ancestor");
    let res = go(&native(&dir.native().join("nope/a.txt")), dir.app_path()).await;
    assert_eq!(
        res,
        GoToPathResolution::NearestAncestor {
            requested: app(&dir, "nope/a.txt"),
            ancestor_dir: app(&dir, ""),
        }
    );
}

/// The answer is an app path, so a `..` above the root clamps there for a person: a pane
/// never holds `/..`.
#[cfg(unix)]
#[tokio::test]
async fn dotdot_above_the_root_answers_the_root() {
    let base = AppTestDir::new("go_to_root");
    assert_eq!(go("/..", base.app_path()).await, folder("/".to_string()));
}

#[cfg(unix)]
#[tokio::test]
async fn totally_nonexistent_absolute_path_falls_back_to_root() {
    let base = AppTestDir::new("go_to_nowhere");
    assert_eq!(
        go("/totally-nonexistent-xyz-123", base.app_path()).await,
        GoToPathResolution::NearestAncestor {
            requested: "/totally-nonexistent-xyz-123".to_string(),
            ancestor_dir: "/".to_string(),
        }
    );
}

#[tokio::test]
async fn empty_and_blank_input_are_invalid() {
    let base = AppTestDir::new("go_to_blank");
    assert!(matches!(
        go("", base.app_path()).await,
        GoToPathResolution::Invalid { .. }
    ));
    assert!(matches!(
        go("   ", base.app_path()).await,
        GoToPathResolution::Invalid { .. }
    ));
}

/// A hung mount refuses within the timeout, ❌ never blocks and never guesses lexically.
#[tokio::test]
async fn a_hung_mount_refuses_a_dot_dot_in_time() {
    let wedged: Arc<dyn Volume> = Arc::new(WedgedVolume::new("Hung share"));
    let host = AppHost::over(move |_| Some(wedged.clone()), Duration::from_millis(50));
    let started = std::time::Instant::now();
    let answer = host.resolve_typed("sftp://h:22/x/../y", None, TypedBy::Person).await;
    assert!(
        matches!(
            answer,
            Err(TypedPathRefusal::Unanswered(VolumeError::ConnectionTimeout(_)))
        ),
        "got {answer:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );

    // The dialog reads it as nothing to go to.
    let wedged: Arc<dyn Volume> = Arc::new(WedgedVolume::new("Hung share"));
    let host = AppHost::over(move |_| Some(wedged.clone()), Duration::from_millis(50));
    assert!(matches!(
        resolve_over("sftp://h:22/x/../y", "/", &host).await,
        GoToPathResolution::Invalid { .. }
    ));
}

/// A stat that misses the deadline is "couldn't check", ❌ never "doesn't exist": reading it as
/// missing walked every ancestor past the spent deadline to the root, and the jump landed there
/// with a "doesn't exist" toast about a path that may well exist.
#[tokio::test]
async fn a_hung_mount_with_no_dot_dot_is_nothing_to_go_to_never_the_root() {
    let wedged: Arc<dyn Volume> = Arc::new(WedgedVolume::new("Hung share"));
    let host = AppHost::over(move |_| Some(wedged.clone()), Duration::from_millis(50));
    let answer = resolve_over("sftp://h:22/x/y", "/", &host).await;
    assert!(matches!(answer, GoToPathResolution::Invalid { .. }), "got {answer:?}");
}

/// What the MCP tools' path door calls: an agent's `..` above the root is refused, typed.
#[tokio::test]
async fn an_agent_above_the_root_is_refused_off_root() {
    let answer = disk().resolve_typed("sftp://h:22/../x", None, TypedBy::Agent).await;
    assert!(matches!(answer, Err(TypedPathRefusal::OffRoot)), "got {answer:?}");
}

/// A path whose volume isn't registered (a server nobody connected) can't be asked.
#[tokio::test]
async fn a_dot_dot_on_no_volume_is_refused() {
    let host = AppHost::over(|_| None, Duration::from_secs(1));
    let answer = host.resolve_typed("sftp://h:22/a/../b", None, TypedBy::Person).await;
    assert!(
        matches!(answer, Err(TypedPathRefusal::Unanswered(VolumeError::NotConnected(_)))),
        "got {answer:?}"
    );
}

#[test]
fn resolution_serializes_with_camelcase_kind_and_fields() {
    let res = GoToPathResolution::File {
        path: "/Users/x/a.txt".to_string(),
        parent_dir: "/Users/x".to_string(),
        file_name: "a.txt".to_string(),
        crossed_link: true,
    };
    let json = serde_json::to_string(&res).unwrap();
    assert!(json.contains("\"kind\":\"file\""), "got {json}");
    assert!(json.contains("\"path\""), "got {json}");
    assert!(json.contains("\"parentDir\""), "got {json}");
    assert!(json.contains("\"fileName\""), "got {json}");
    assert!(json.contains("\"crossedLink\":true"), "got {json}");

    let ancestor = GoToPathResolution::NearestAncestor {
        requested: "/tmp/nope".to_string(),
        ancestor_dir: "/tmp".to_string(),
    };
    let json = serde_json::to_string(&ancestor).unwrap();
    assert!(json.contains("\"kind\":\"nearestAncestor\""), "got {json}");
    assert!(json.contains("\"ancestorDir\""), "got {json}");
}
