//! Tests for `binary_search.rs`: each OS's candidate list, built from a fixed environment, so
//! every OS's answer is checked on every OS.

use std::path::PathBuf;

use super::*;

fn env() -> SearchEnv {
    SearchEnv {
        path_dirs: vec![PathBuf::from("/p1"), PathBuf::from("/p2")],
        android_home: Some(PathBuf::from("/ah")),
        android_sdk_root: Some(PathBuf::from("/asr")),
        home: Some(PathBuf::from("/home/test_user_1")),
        local_app_data: Some(PathBuf::from("/lad")),
    }
}

fn shown(candidates: Vec<PathBuf>) -> Vec<String> {
    candidates
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn macos_searches_the_path_the_sdk_variables_its_default_sdk_and_homebrew() {
    assert_eq!(
        shown(candidates(&AdbSearch::MACOS, &env())),
        [
            "/p1/adb",
            "/p2/adb",
            "/ah/platform-tools/adb",
            "/asr/platform-tools/adb",
            "/home/test_user_1/Library/Android/sdk/platform-tools/adb",
            "/opt/homebrew/bin/adb",
            "/usr/local/bin/adb",
        ]
    );
}

/// Android Studio's default SDK on Linux is `~/Android/Sdk` (#1108).
#[test]
fn linux_searches_android_studios_default_sdk_under_home() {
    assert_eq!(
        shown(candidates(&AdbSearch::LINUX, &env())),
        [
            "/p1/adb",
            "/p2/adb",
            "/ah/platform-tools/adb",
            "/asr/platform-tools/adb",
            "/home/test_user_1/Android/Sdk/platform-tools/adb",
            "/usr/local/bin/adb",
        ]
    );
}

/// Windows names the binary `adb.exe` everywhere, and Android Studio puts its SDK under
/// `%LOCALAPPDATA%`, not the home folder (#1108). The per-user package managers' folders come
/// last, for an `adb` installed after Cmdr started.
#[test]
fn windows_searches_adb_exe_the_sdk_under_local_app_data_and_the_package_managers() {
    assert_eq!(
        shown(candidates(&AdbSearch::WINDOWS, &env())),
        [
            "/p1/adb.exe",
            "/p2/adb.exe",
            "/ah/platform-tools/adb.exe",
            "/asr/platform-tools/adb.exe",
            "/lad/Android/Sdk/platform-tools/adb.exe",
            "/lad/Microsoft/WinGet/Links/adb.exe",
            "/home/test_user_1/scoop/shims/adb.exe",
        ]
    );
}

/// ❗ Cmdr runs whatever `adb` the search finds, at launch, with nothing verified, so a searched
/// folder must be one only the current user or an administrator controls (`docs/security.md`
/// § "Helper tools"). On a default Windows install any standard user can create folders under
/// `C:\ProgramData` and owns what they make there, so `C:\ProgramData\chocolatey\bin` (with
/// Chocolatey not installed, the usual machine) is another account's to plant an `adb.exe` in.
/// This pins every fixed folder to a per-user base; a machine-wide one needs an admin-owned base.
#[test]
fn windows_searches_only_folders_the_user_or_an_admin_controls() {
    let search = AdbSearch::WINDOWS;
    let bases: Vec<DirBase> = std::iter::once(search.default_sdk.0)
        .chain(search.fixed_dirs.iter().map(|&(base, _)| base))
        .collect();
    assert!(
        bases
            .iter()
            .all(|base| matches!(base, DirBase::Home | DirBase::LocalAppData)),
        "{bases:?}"
    );
    assert_eq!(
        search.fixed_dirs,
        [
            (DirBase::LocalAppData, "Microsoft/WinGet/Links"),
            (DirBase::Home, "scoop/shims"),
        ]
    );
}

#[test]
fn a_missing_variable_drops_only_its_own_candidates() {
    let bare = SearchEnv {
        path_dirs: Vec::new(),
        android_home: None,
        android_sdk_root: None,
        home: None,
        local_app_data: None,
    };
    assert_eq!(shown(candidates(&AdbSearch::LINUX, &bare)), ["/usr/local/bin/adb"]);
    assert!(candidates(&AdbSearch::WINDOWS, &bare).is_empty());
}
