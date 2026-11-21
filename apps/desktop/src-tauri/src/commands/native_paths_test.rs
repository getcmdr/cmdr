use cmdr_fs::app_path::{AppPathBuf, NotAnAppPath, PathFlavor, is_escape};

use super::{NativePathCrossing, answer, app_paths_from_native};

/// The door's answer for `text` under `flavor`'s rules, through the same crossing
/// `from_native_unicode` makes on that OS (no escape made), so Windows' runs on any host.
fn cross(text: &str, flavor: PathFlavor) -> NativePathCrossing {
    let crossed = match flavor {
        PathFlavor::Posix => AppPathBuf::from_native_posix(text.as_bytes(), flavor),
        PathFlavor::Windows => AppPathBuf::from_native_windows(&text.encode_utf16().collect::<Vec<u16>>(), flavor),
    }
    .and_then(|app| {
        if app.as_str().chars().any(is_escape) {
            Err(NotAnAppPath::NotUnicode)
        } else {
            Ok(app)
        }
    });
    answer(text, crossed)
}

fn app(path: &str) -> NativePathCrossing {
    NativePathCrossing::App { path: path.to_owned() }
}

#[test]
fn a_windows_drive_path_crosses_to_its_app_text() {
    assert_eq!(
        cross(r"C:\Users\test_user_1\Documents\a.txt", PathFlavor::Windows),
        app("C:/Users/test_user_1/Documents/a.txt")
    );
    assert_eq!(cross(r"d:\photos", PathFlavor::Windows), app("D:/photos"));
    assert_eq!(cross(r"C:\", PathFlavor::Windows), app("C:/"));
}

#[test]
fn a_windows_unc_and_verbatim_path_cross_to_their_app_text() {
    assert_eq!(
        cross(r"\\testnas.local\share\x", PathFlavor::Windows),
        app("//testnas.local/share/x")
    );
    assert_eq!(cross(r"\\?\C:\Users\x", PathFlavor::Windows), app("C:/Users/x"));
    assert_eq!(
        cross(r"\\?\UNC\testnas.local\share\x", PathFlavor::Windows),
        app("//testnas.local/share/x")
    );
}

#[test]
fn text_no_app_path_names_is_refused_under_windows_rules() {
    for text in [
        r"\\?\Volume{12345678}\x",
        r"\\.\PhysicalDrive0",
        "C:foo",
        "C:",
        r"foo\bar",
        "",
        "/Users/x",
    ] {
        assert_eq!(
            cross(text, PathFlavor::Windows),
            NativePathCrossing::Refused,
            "{text:?}"
        );
    }
}

#[test]
fn a_literal_escape_range_code_point_is_refused_since_no_escape_is_made() {
    assert_eq!(
        cross("C:\\x\\a\u{10FE80}b", PathFlavor::Windows),
        NativePathCrossing::Refused
    );
    assert_eq!(cross("/x/a\u{10FE80}b", PathFlavor::Posix), NativePathCrossing::Refused);
}

#[test]
fn a_posix_path_is_its_own_app_text() {
    for text in [
        "/Users/test_user_1/Documents/a.txt",
        "/",
        "/Volumes/share/x y/ünïcode.txt",
        "/x/a\\b",
    ] {
        assert_eq!(cross(text, PathFlavor::Posix), app(text), "{text:?}");
    }
}

#[test]
fn a_relative_or_dot_dot_posix_path_is_refused() {
    for text in ["relative/x", "", "/x/../y"] {
        assert_eq!(cross(text, PathFlavor::Posix), NativePathCrossing::Refused, "{text:?}");
    }
}

#[tokio::test]
async fn the_command_answers_each_path_in_order_under_the_hosts_rules() {
    let (good, expected) = if PathFlavor::HOST == PathFlavor::Windows {
        (r"C:\Users\test_user_1\a.txt", "C:/Users/test_user_1/a.txt")
    } else {
        ("/Users/test_user_1/a.txt", "/Users/test_user_1/a.txt")
    };
    let answers = app_paths_from_native(vec![good.to_owned(), "relative".to_owned(), good.to_owned()]).await;
    assert_eq!(answers, vec![app(expected), NativePathCrossing::Refused, app(expected)]);
}
