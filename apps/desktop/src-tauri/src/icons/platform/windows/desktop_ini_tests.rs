//! `desktop.ini` folders: how the file is read (as `GetPrivateProfileString` would), which icon
//! files pass the vetting, and that a refused folder keeps the plain folder icon. The real-file
//! cases run on the Windows CI lane's `C:` (a fixed drive).

use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY};

use super::desktop_ini::{self, Parsed};
use super::tests::{explorer_exe, set_attributes};
use super::{draws_own_folder_icon, fetch_path_icons};
use crate::icons::get_icons;
use crate::test_support::TestDir;

fn app(path: &Path) -> String {
    crate::app_text::of_native(path).expect("app text")
}

fn icon(file: &str, index: i32) -> Parsed {
    Parsed::Icon {
        file: file.to_string(),
        index,
    }
}

#[test]
fn the_first_key_of_the_first_section_wins() {
    let text = "; a comment\r\n[.ShellClassInfo]\r\nIconResource=\\\\attacker.example\\s\\a.ico,0\r\nIconResource=C:\\Windows\\explorer.exe,0\r\n";
    assert_eq!(desktop_ini::parse(text), icon(r"\\attacker.example\s\a.ico", 0));

    let two_sections = "[.ShellClassInfo]\nIconResource=\\\\attacker.example\\s\\a.ico,0\n[.ShellClassInfo]\nIconResource=C:\\Windows\\explorer.exe,0\n";
    assert_eq!(desktop_ini::parse(two_sections), icon(r"\\attacker.example\s\a.ico", 0));

    let other_section = "[ViewState]\nIconResource=x.ico,0\n[.ShellClassInfo]\nIconResource=y.ico,-3\n";
    assert_eq!(desktop_ini::parse(other_section), icon("y.ico", -3));
}

/// An empty first `IconResource=` (`,0`) locks the key: the Shell reads it as nothing, so a
/// later line never names the icon.
#[test]
fn an_empty_first_key_names_no_icon() {
    let text = "[.ShellClassInfo]\nIconResource=,0\nIconResource=C:\\Windows\\explorer.exe,0\n";
    assert_eq!(desktop_ini::parse(text), Parsed::None);
}

#[test]
fn icon_resource_wins_over_icon_file_and_index() {
    let old = "[.shellclassinfo]\nICONFILE=\"%SystemRoot%\\system32\\shell32.dll\"\nIconIndex=4\n";
    assert_eq!(desktop_ini::parse(old), icon(r"%SystemRoot%\system32\shell32.dll", 4));
    let both = "[.ShellClassInfo]\nIconFile=a.ico\nIconResource=b.ico,2\n";
    assert_eq!(desktop_ini::parse(both), icon("b.ico", 2));
    let name_only = "[.ShellClassInfo]\nLocalizedResourceName=@%SystemRoot%\\system32\\shell32.dll,-21770\n";
    assert_eq!(desktop_ini::parse(name_only), Parsed::None);
}

#[test]
fn a_handler_key_anywhere_refuses_the_file() {
    for key in ["CLSID", "CLSID2", "UICLSID", "clsid2"] {
        let text = format!("[.ShellClassInfo]\nIconResource=x.ico,0\n{key}={{0AFACED1-E828-11D1-9187-B532F1E9575D}}\n");
        assert_eq!(desktop_ini::parse(&text), Parsed::Refused, "{key}");
    }
    let elsewhere = "[.ShellClassInfo]\nIconResource=x.ico,0\n[Other]\nCLSID={BD84B380-8CA2-1069-AB1D-08000948F534}\n";
    assert_eq!(desktop_ini::parse(elsewhere), Parsed::Refused);
}

#[test]
fn desktop_ini_reads_utf16_and_utf8() {
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(
        "[.ShellClassInfo]\nIconFile=é.ico"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    assert_eq!(desktop_ini::parse(&desktop_ini::decode(&utf16)), icon("é.ico", 0));
    assert_eq!(desktop_ini::decode(b"\xEF\xBB\xBFab"), "ab");
}

#[test]
fn a_path_under_a_root_matches_ignoring_case_only_by_whole_names() {
    let root = Path::new(r"C:\Windows");
    assert!(desktop_ini::is_under(
        Path::new(r"c:\windows\system32\imageres.dll"),
        root
    ));
    assert!(!desktop_ini::is_under(Path::new(r"C:\WindowsApps\x.dll"), root));
    assert!(!desktop_ini::is_under(Path::new(r"D:\Windows\x.dll"), root));
}

/// Writes `desktop.ini` into `folder` and marks the folder customized (read-only), as Explorer's
/// Customize tab does (and as 7-Zip restores from an archive).
fn customize(folder: &Path, ini: &str) {
    std::fs::write(folder.join("desktop.ini"), ini).expect("write desktop.ini");
    set_attributes(folder, FILE_ATTRIBUTE_READONLY);
}

fn customized(dir: &TestDir, name: &str, ini: &str) -> std::path::PathBuf {
    let folder = dir.as_ref().join(name);
    std::fs::create_dir(&folder).expect("create folder");
    customize(&folder, ini);
    folder
}

fn uncustomize(folder: &Path) {
    set_attributes(folder, FILE_ATTRIBUTE_NORMAL);
}

#[test]
fn a_folder_naming_a_system_icon_draws_it_untinted() {
    let dir = TestDir::new("icon_desktop_ini");
    let folder = customized(
        &dir,
        "Custom",
        "[.ShellClassInfo]\r\nIconResource=%SystemRoot%\\explorer.exe,0\r\n",
    );
    assert!(draws_own_folder_icon(&folder));
    // The visible-folder ask reports it as artwork, which the frontend never gold-tints.
    let reported = crate::icons::custom_folder_icon_ids(vec![app(&folder)]);
    assert_eq!(reported.len(), 1);
    assert!(!reported[0].follows_accent);
    let own = fetch_path_icons(vec![app(&folder)]).pop().and_then(|(_, url)| url);
    let generic = get_icons(vec!["dir".to_string()], false);
    assert!(own.is_some(), "the customized folder draws its own icon");
    assert_ne!(own.as_ref(), generic.get("dir"));
    uncustomize(&folder);
}

#[test]
fn a_folder_naming_an_icon_file_inside_it_draws_it() {
    let dir = TestDir::new("icon_desktop_ini_inside");
    let folder = dir.as_ref().join("Inside");
    std::fs::create_dir_all(folder.join("art")).expect("create folder");
    std::fs::copy(explorer_exe(), folder.join("art").join("tool.exe")).expect("copy an icon file");
    customize(&folder, "[.ShellClassInfo]\nIconResource=art\\tool.exe,0\n");
    assert!(draws_own_folder_icon(&folder));
    uncustomize(&folder);
}

#[test]
fn a_refused_folder_keeps_the_plain_folder_icon() {
    let dir = TestDir::new("icon_desktop_ini_not");

    let unmarked = dir.as_ref().join("Unmarked");
    std::fs::create_dir(&unmarked).expect("create folder");
    std::fs::write(
        unmarked.join("desktop.ini"),
        "[.ShellClassInfo]\nIconResource=%SystemRoot%\\explorer.exe,0\n",
    )
    .expect("write desktop.ini");
    assert!(!draws_own_folder_icon(&unmarked), "no read-only or system attribute");

    let cases = [
        ("Remote", "[.ShellClassInfo]\nIconResource=\\\\cmdr-test.invalid\\share\\x.ico,0\n".to_string()),
        (
            "RemoteFirst",
            "[.ShellClassInfo]\nIconResource=\\\\cmdr-test.invalid\\s\\a.ico,0\nIconResource=%SystemRoot%\\explorer.exe,0\n".to_string(),
        ),
        ("Up", "[.ShellClassInfo]\nIconResource=..\\x.ico,0\n".to_string()),
        ("Elsewhere", format!("[.ShellClassInfo]\nIconResource={},0\n", dir.as_ref().join("x.exe").display())),
        ("Shortcut", "[.ShellClassInfo]\nIconResource=%SystemRoot%\\x.lnk,0\n".to_string()),
        (
            "Handler",
            "[.ShellClassInfo]\nIconResource=%SystemRoot%\\explorer.exe,0\nCLSID2={0AFACED1-E828-11D1-9187-B532F1E9575D}\n".to_string(),
        ),
        ("NamedOnly", "[.ShellClassInfo]\nLocalizedResourceName=Something\n".to_string()),
        (
            "PastTheCap",
            format!(
                "{}[.ShellClassInfo]\nIconResource=%SystemRoot%\\explorer.exe,0\n",
                ";\n".repeat(usize::try_from(desktop_ini::MAX_LEN).expect("fits") / 2 + 1)
            ),
        ),
    ];
    for (name, ini) in &cases {
        let folder = customized(&dir, name, ini);
        assert!(!draws_own_folder_icon(&folder), "{name}");
        assert_eq!(
            fetch_path_icons(vec![app(&folder)])[0].1,
            None,
            "{name}: the generic folder"
        );
        uncustomize(&folder);
    }
}

/// A relative icon path through a junction is refused before anything past the junction is
/// read: the junction could point at a share.
#[test]
fn an_icon_behind_a_junction_is_refused() {
    let dir = TestDir::new("icon_desktop_ini_junction");
    let real = dir.as_ref().join("real");
    std::fs::create_dir(&real).expect("create the junction's target");
    std::fs::copy(explorer_exe(), real.join("tool.exe")).expect("copy an icon file");
    let folder = dir.as_ref().join("Linked");
    std::fs::create_dir(&folder).expect("create folder");
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(folder.join("d"))
        .arg(&real)
        .output()
        .expect("run mklink");
    assert!(made.status.success(), "mklink /J: {made:?}");
    customize(&folder, "[.ShellClassInfo]\nIconResource=d\\tool.exe,0\n");
    assert!(!draws_own_folder_icon(&folder));
    uncustomize(&folder);
}
