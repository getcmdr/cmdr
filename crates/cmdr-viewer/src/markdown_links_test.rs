//! Where a link in a rendered Markdown document goes: the web or a mail app, a spot in
//! the same document, another file on the document's volume, or nowhere we follow.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::app_path::PathFlavor::Posix;

use super::markdown_links::{MarkdownLink, resolve_link};

fn resolve(href: &str) -> MarkdownLink {
    resolve_link(href, Some(app_path!("/Users/a/proj/docs/README.md")), app_path!("/"))
}

fn file(path: &str, element_id: Option<&str>) -> MarkdownLink {
    MarkdownLink::File {
        path: path.to_string(),
        element_id: element_id.map(str::to_string),
    }
}

fn anchor(element_id: Option<&str>) -> MarkdownLink {
    MarkdownLink::Anchor {
        element_id: element_id.map(str::to_string),
    }
}

#[test]
fn a_relative_link_opens_the_file_beside_the_document() {
    assert_eq!(resolve("setup.md"), file("/Users/a/proj/docs/setup.md", None));
    assert_eq!(
        resolve("./guides/setup.md"),
        file("/Users/a/proj/docs/guides/setup.md", None)
    );
    assert_eq!(resolve("../LICENSE"), file("/Users/a/proj/LICENSE", None));
}

#[test]
fn a_relative_link_keeps_its_fragment_as_the_rendered_heading_id() {
    assert_eq!(
        resolve("../README.md#install"),
        file("/Users/a/proj/README.md", Some("md-install"))
    );
}

#[test]
fn a_fragment_and_a_path_decode_their_escapes_and_the_query_goes() {
    assert_eq!(
        resolve("my%20notes.md?plain=1#%C3%A1rv%C3%ADz"),
        file("/Users/a/proj/docs/my notes.md", Some("md-árvíz"))
    );
}

#[test]
fn a_link_to_the_document_itself_is_an_anchor() {
    assert_eq!(resolve("README.md#usage"), anchor(Some("md-usage")));
    assert_eq!(resolve("./README.md"), anchor(None));
}

#[test]
fn a_bare_fragment_is_already_the_rendered_id() {
    // The renderer rewrote `#usage` to `#md-usage` in the HTML the click came from.
    assert_eq!(resolve("#md-usage"), anchor(Some("md-usage")));
    assert_eq!(resolve("#md-%C3%A1rv"), anchor(Some("md-árv")));
}

#[test]
fn climbing_past_the_volume_root_stops_at_the_root() {
    assert_eq!(resolve("../../../../../../etc/passwd"), file("/etc/passwd", None));
    let on_share = resolve_link(
        "../../../../notes.md",
        Some(app_path!("/Volumes/share/docs/README.md")),
        app_path!("/Volumes/share"),
    );
    assert_eq!(on_share, file("/Volumes/share/notes.md", None));
}

#[test]
fn a_document_on_a_phone_or_in_an_archive_links_within_it() {
    let on_phone = resolve_link(
        "../Docs/b.md",
        Some(app_path!("adb://R58M1/sdcard/Notes/a.md")),
        app_path!("adb://R58M1/"),
    );
    assert_eq!(on_phone, file("adb://R58M1/sdcard/Docs/b.md", None));
    let in_zip = resolve_link("b.md#x", Some(app_path!("/Users/a/x.zip/docs/a.md")), app_path!("/"));
    assert_eq!(in_zip, file("/Users/a/x.zip/docs/b.md", Some("md-x")));
}

#[test]
fn web_and_mail_links_go_out() {
    assert_eq!(
        resolve("https://getcmdr.com/docs?a=1#top"),
        MarkdownLink::External {
            url: "https://getcmdr.com/docs?a=1#top".to_string()
        }
    );
    assert_eq!(
        resolve("HTTP://Example.com"),
        MarkdownLink::External {
            url: "http://example.com/".to_string()
        }
    );
    assert_eq!(
        resolve("mailto:hello@getcmdr.com"),
        MarkdownLink::External {
            url: "mailto:hello@getcmdr.com".to_string()
        }
    );
    assert_eq!(
        resolve("//example.com/a"),
        MarkdownLink::External {
            url: "https://example.com/a".to_string()
        }
    );
}

#[test]
fn absolute_paths_files_and_other_schemes_are_not_followed() {
    for href in [
        "",
        "   ",
        "/docs/setup.md",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "tel:+46700000000",
        "data:text/html,hi",
        "smb://server/share/a.md",
    ] {
        assert_eq!(resolve(href), MarkdownLink::Blocked, "{href:?}");
    }
}

/// The folder's names go into the URL escaped and come back out decoded, whatever they
/// hold: what `url`'s own path conversions answered on macOS and Linux, now on every OS
/// and in the browser alike (they don't exist there, nor on Windows for a `/` path).
#[test]
fn a_folders_names_survive_the_url_round_trip() {
    for (document, href, expected) in [
        ("/docs/sub dir/README.md", "%2e%2e/x.md", "/docs/x.md"),
        ("/naïve/日本/README.md", "../x.md", "/naïve/x.md"),
        ("/100%/#hash?q/README.md", "./a b.md", "/100%/#hash?q/a b.md"),
        ("/a/(b)[c]{d}`e`\\f/README.md", "x.md", "/a/(b)[c]{d}`e`\\f/x.md"),
        ("/a/b/c/README.md", "../../../../x.md", "/x.md"),
        ("/docs/README.md", "caf%C3%A9.md", "/docs/café.md"),
        // `url` reads a `C:` segment as a drive letter, which `..` doesn't climb past.
        ("/x/C:/y.md", "../../x.md", "/x/C:/x.md"),
    ] {
        let resolved = resolve_link(
            href,
            Some(AppPath::from_normalized_as(document, Posix).unwrap()),
            app_path!("/"),
        );
        assert_eq!(resolved, file(expected, None), "{document:?} + {href:?}");
    }
    assert_eq!(
        resolve("sub/%20spaced.md#frag"),
        file("/Users/a/proj/docs/sub/ spaced.md", Some("md-frag"))
    );
}

/// An escaped `/` splits a segment into names, as `Path` read the decoded text.
#[test]
fn an_escaped_slash_separates_names() {
    assert_eq!(resolve("a%2Fb.md"), file("/Users/a/proj/docs/a/b.md", None));
    assert_eq!(resolve("a%2F%2F.%2Fb.md"), file("/Users/a/proj/docs/a/b.md", None));
}

/// A decoded name that is no name resolves nowhere. `std::path` handed the volume each of
/// these: `a/../../etc` (an escaped `/` hid the `..` from the URL's dot rule, so it climbed
/// past the volume root once the OS resolved it), a link to the U+FFFD sibling of a name
/// that isn't UTF-8 (an image got the raw bytes), and a NUL.
#[test]
fn a_decoded_name_that_is_no_name_resolves_nowhere() {
    for href in [
        "a%2F..%2F..%2F..%2Fetc",
        "..%2F..%2F..%2Fetc/passwd",
        "bad%FF.md",
        "x%00.md",
    ] {
        assert_eq!(resolve(href), MarkdownLink::Blocked, "{href:?}");
    }
}

/// A document whose text the app path type refuses has no folder to resolve against.
#[test]
fn a_document_the_type_refuses_links_nowhere_relative() {
    assert_eq!(resolve_link("x.md", None, app_path!("/")), MarkdownLink::Blocked);
    assert_eq!(resolve_link("#md-top", None, app_path!("/")), anchor(Some("md-top")));
}
