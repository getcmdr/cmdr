//! The Windows icon arm against the real Shell, on the Windows CI lane.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW};

use super::{fetch_path_icons, may_read_own_icon, place, shell_image, shortcut};
use crate::config::ICON_SIZE;
use crate::icons::get_icons;
use crate::test_support::TestDir;

const DATA_URL_PREFIX: &str = "data:image/webp;base64,";

fn explorer_exe() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot is set on Windows")).join("explorer.exe")
}

fn set_attributes(path: &Path, attributes: u32) {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated path to a file this test owns.
    let ok = unsafe { SetFileAttributesW(wide.as_ptr(), attributes) };
    assert_ne!(ok, 0, "SetFileAttributesW on the fixture");
}

/// A type lookup asks the Shell about Cmdr's own empty sample, and gets a drawn icon of the
/// asked size back.
#[test]
fn a_type_lookup_returns_a_drawn_icon_of_the_asked_size() {
    let sample = crate::icons::icon_sample_path("txt").expect("a sample for txt");
    let img = shell_image::render(&sample, ICON_SIZE).expect("the Shell draws a .txt icon");
    assert_eq!(img.dimensions(), (ICON_SIZE, ICON_SIZE));
    assert!(img.pixels().any(|px| px[3] > 0), "the icon has visible pixels");
    assert!(
        img.pixels().any(|px| px[3] == 0),
        "the icon keeps its transparent margin"
    );
}

#[test]
fn every_type_id_gets_an_icon() {
    let ids = [
        "ext:txt",
        "ext:exe",
        "ext:cmdrnosuchtype",
        "dir",
        "symlink-dir",
        "file",
        "symlink-file",
    ];
    let icons = get_icons(ids.iter().map(|id| (*id).to_string()).collect(), false);
    for id in ids {
        let url = icons.get(id).unwrap_or_else(|| panic!("no icon for {id}"));
        assert!(
            url.starts_with(DATA_URL_PREFIX) && url.len() > DATA_URL_PREFIX.len(),
            "{id}"
        );
    }
}

/// A program on a local drive shows the icon it holds, not the generic program icon.
#[test]
fn a_local_program_shows_its_own_icon() {
    let explorer = crate::app_text::of_native(&explorer_exe()).expect("explorer.exe as app text");
    assert!(may_read_own_icon(&explorer));
    let own = fetch_path_icons(vec![explorer.clone()]);
    assert_eq!(own[0].0, format!("path:{explorer}"));
    let own = own[0].1.clone().expect("explorer.exe's own icon");
    let generic = get_icons(vec!["ext:exe".to_string()], false);
    assert_ne!(Some(&own), generic.get("ext:exe"), "explorer.exe draws its own icon");
}

/// A program on a share is never asked: `path:` answers its type's icon without reaching the
/// server (the UNC check is lexical, so an unresolvable server name proves it).
#[test]
fn a_program_on_a_share_shows_its_type_icon_without_asking_the_share() {
    let on_share = "//cmdr-test.invalid/share/tool.exe";
    assert!(!may_read_own_icon(on_share));
    let id = format!("path:{on_share}");
    let icons = get_icons(vec![id.clone(), "ext:exe".to_string()], false);
    assert_eq!(icons.get(&id), icons.get("ext:exe"));
    assert!(icons.contains_key(&id));
}

/// A cloud placeholder (here an `OFFLINE` file, the attribute an HSM or Cloud Files stub
/// carries) is never asked, since opening it would start a download.
#[test]
fn a_file_whose_bytes_are_elsewhere_is_never_asked() {
    let dir = TestDir::new("icon_placeholder");
    let file = dir.as_ref().join("tool.exe");
    std::fs::write(&file, b"").expect("write fixture");
    assert!(place::may_read(&file));

    set_attributes(&file, FILE_ATTRIBUTE_OFFLINE);
    assert!(!place::may_read(&file));
    set_attributes(&file, FILE_ATTRIBUTE_NORMAL);
}

#[test]
fn a_place_that_doesnt_exist_is_never_asked() {
    let dir = TestDir::new("icon_missing");
    assert!(!place::may_read(&dir.as_ref().join("inside.zip").join("tool.exe")));
    assert!(!place::may_read(Path::new(r"\\cmdr-test.invalid\share\tool.exe")));
}

/// Past `MAX_PATH` the app path crosses verbatim, which the Shell won't parse: the type icon
/// stands in.
#[test]
fn a_verbatim_path_is_never_asked() {
    let verbatim = PathBuf::from(format!(r"\\?\{}", explorer_exe().display()));
    assert!(place::may_read(&explorer_exe()));
    assert!(!place::may_read(&verbatim));
}

/// A junction is read by its tag, never followed: it could point at a share.
#[test]
fn a_junction_is_never_asked() {
    let dir = TestDir::new("icon_junction");
    let real = dir.as_ref().join("real");
    let link = dir.as_ref().join("link");
    std::fs::create_dir(&real).expect("create the junction's target");
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&link)
        .arg(&real)
        .output()
        .expect("run mklink");
    assert!(made.status.success(), "mklink /J: {made:?}");
    assert!(place::may_read(&real));
    assert!(!place::may_read(&link));
}

/// Writes a shortcut at `lnk` to `target`, with `icon` as its icon location when given.
fn write_shortcut(lnk: &Path, target: &Path, icon: Option<&str>) {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};

    crate::platform::apartment::ensure_on_this_thread();
    // SAFETY: the Shell's in-process `ShellLink`; every string is a live `HSTRING` for its call.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).expect("ShellLink");
        link.SetPath(&HSTRING::from(target.as_os_str())).expect("SetPath");
        if let Some(icon) = icon {
            link.SetIconLocation(&HSTRING::from(icon), 0).expect("SetIconLocation");
        }
        let file: IPersistFile = link.cast().expect("IPersistFile");
        file.Save(&HSTRING::from(lnk.as_os_str()), true).expect("Save");
    }
}

/// A shortcut draws its own icon only when its icon location and target both stay here: one
/// naming `\\host\…` would open an SMB connection on listing.
#[test]
fn a_shortcut_pointing_past_this_machine_is_never_drawn_from_there() {
    let dir = TestDir::new("icon_shortcut");
    let local = dir.as_ref().join("Explorer.lnk");
    write_shortcut(&local, &explorer_exe(), None);
    assert!(shortcut::is_shortcut(&local));
    assert!(shortcut::points_only_here(&local));

    let env_icon = dir.as_ref().join("EnvIcon.lnk");
    write_shortcut(&env_icon, &explorer_exe(), Some(r"%SystemRoot%\explorer.exe"));
    assert!(shortcut::points_only_here(&env_icon));

    let remote_icon = dir.as_ref().join("RemoteIcon.lnk");
    write_shortcut(&remote_icon, &explorer_exe(), Some(r"\\cmdr-test.invalid\share\x.ico"));
    assert!(!shortcut::points_only_here(&remote_icon));

    // Its own icon is then the shortcut type's, still an icon.
    let app = crate::app_text::of_native(&remote_icon).expect("app text");
    assert!(fetch_path_icons(vec![app])[0].1.is_some());
}

#[test]
fn env_vars_in_a_link_expand_and_unknown_ones_stay() {
    let lookup = |name: &str| (name == "SystemRoot").then(|| r"C:\Windows".to_string());
    assert_eq!(
        shortcut::expand_env_vars(r"%SystemRoot%\system32\shell32.dll", lookup),
        r"C:\Windows\system32\shell32.dll"
    );
    assert_eq!(shortcut::expand_env_vars(r"%Nope%\x", lookup), r"%Nope%\x");
    assert_eq!(shortcut::expand_env_vars("100%", lookup), "100%");
    assert_eq!(shortcut::expand_env_vars(r"C:\plain", lookup), r"C:\plain");
}

#[test]
fn premultiplied_bgra_comes_back_straight_rgba() {
    // Half-transparent red-and-some-blue, premultiplied: B 64, G 0, R 128, A 128.
    let rgba = shell_image::bgra_to_straight_rgba(vec![64, 0, 128, 128, 0, 0, 0, 0]);
    assert_eq!(rgba, Some(vec![255, 0, 128, 128, 0, 0, 0, 0]));
}

#[test]
fn bgra_without_any_alpha_is_opaque() {
    let rgba = shell_image::bgra_to_straight_rgba(vec![1, 2, 3, 0, 4, 5, 6, 0]);
    assert_eq!(rgba, Some(vec![3, 2, 1, 255, 6, 5, 4, 255]));
}

#[test]
fn bgra_with_a_color_above_its_alpha_is_already_straight() {
    let rgba = shell_image::bgra_to_straight_rgba(vec![200, 10, 20, 100]);
    assert_eq!(rgba, Some(vec![20, 10, 200, 100]));
}

/// The Shell drawing nothing is no icon, not a black square.
#[test]
fn an_all_zero_buffer_is_no_icon() {
    assert_eq!(shell_image::bgra_to_straight_rgba(vec![0; 16]), None);
}
