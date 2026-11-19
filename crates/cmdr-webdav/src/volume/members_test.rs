//! Which members a `Depth: 1` answer lists, from crafted `multistatus` bodies in each
//! server's own spelling: the self entry left out, every plain name listed, and a member
//! whose name isn't one plain name (`.`, `..`, a `%2F` inside one name) left out and
//! counted, however the server spelled its `href`.

use super::members;
use crate::propfind::parse_multistatus;

/// A `multistatus` body answering `hrefs` (each an `href` as the server sends it, and
/// whether it's a collection), in the minimal shape every server shares.
fn body(hrefs: &[(&str, bool)]) -> String {
    let responses: String = hrefs
        .iter()
        .map(|(href, collection)| {
            let kind = if *collection { "<d:collection/>" } else { "" };
            format!(
                "<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:resourcetype>{kind}</d:resourcetype>\
                 </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            )
        })
        .collect();
    format!(r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">{responses}</d:multistatus>"#)
}

/// The names `members` lists for `hrefs` under `own`, and how many it left out.
fn listed(own: &str, hrefs: &[(&str, bool)]) -> (Vec<String>, usize) {
    let props = parse_multistatus(&body(hrefs)).expect("a multistatus body");
    let found = members(own, &props);
    let names = found.listed.iter().map(|(_, name)| (*name).to_string()).collect();
    (names, found.unaddressable)
}

/// Nextcloud and ownCloud (SabreDAV underneath): server paths, percent-encoded, a
/// collection's with a trailing `/`, the self entry first.
#[test]
fn a_nextcloud_answer_lists_its_members() {
    let own = "/remote.php/dav/files/test_user_1/Photos";
    let (names, skipped) = listed(
        own,
        &[
            ("/remote.php/dav/files/test_user_1/Photos/", true),
            ("/remote.php/dav/files/test_user_1/Photos/2024%20summer/", true),
            ("/remote.php/dav/files/test_user_1/Photos/caf%C3%A9.jpg", false),
        ],
    );
    assert_eq!(names, ["2024 summer", "café.jpg"]);
    assert_eq!(skipped, 0);
}

/// Apache `mod_dav` and nginx's `dav_ext` module: the same path-only shape, the self entry
/// anywhere in the answer (RFC 4918 promises no order).
#[test]
fn an_apache_or_nginx_answer_lists_its_members() {
    let (names, skipped) = listed(
        "/dav/box",
        &[
            ("/dav/box/a%20b.txt", false),
            ("/dav/box/", true),
            ("/dav/box/sub/", true),
        ],
    );
    assert_eq!(names, ["a b.txt", "sub"]);
    assert_eq!(skipped, 0);
}

/// IIS answers with absolute URLs (RFC 4918 § 8.3 allows either).
#[test]
fn an_iis_answer_in_absolute_urls_lists_its_members() {
    let (names, skipped) = listed(
        "/dav/box",
        &[
            ("http://testnas.local/dav/box/", true),
            ("http://testnas.local/dav/box/report.docx", false),
            ("http://testnas.local/dav/box/Old%20files/", true),
        ],
    );
    assert_eq!(names, ["report.docx", "Old files"]);
    assert_eq!(skipped, 0);
}

/// ❗ A member a server names `.` or `..` is no row, in every spelling: percent-encoded or
/// not, relative or ❗ absolute, where a URL parser would resolve the dot segment away and
/// list the member under its PARENT's name. Its siblings still list, and the count says how
/// many were left out.
#[test]
fn a_member_named_dot_or_dot_dot_is_left_out_however_it_is_spelled() {
    for dotted in [
        "/dav/box/%2e%2e",
        "/dav/box/%2E%2E/",
        "/dav/box/..",
        "/dav/box/.%2e/",
        "/dav/box/%2E",
        "/dav/box/./",
        "http://testnas.local/dav/box/%2e%2e",
        "http://testnas.local/dav/box/..",
        "http://testnas.local/dav/box/%2E%2e/",
        "http://testnas.local/dav/box/%2E/",
        "https://testnas.local:8443/dav/box/./",
    ] {
        let (names, skipped) = listed(
            "/dav/box",
            &[("/dav/box/", true), (dotted, true), ("/dav/box/kept.txt", false)],
        );
        assert_eq!(names, ["kept.txt"], "{dotted}");
        assert_eq!(skipped, 1, "{dotted}");
    }
}

/// ❗ A name holding an encoded `/` (`a%2Fb`) is no plain name: decoded, it would list as
/// `b` at `box/b`, a row naming a file that isn't there, or a sibling that is. Left out and
/// counted, ❌ never a row.
#[test]
fn a_member_whose_name_holds_an_encoded_slash_is_left_out() {
    for slashed in [
        "/dav/box/a%2Fb",
        "/dav/box/a%2fb/",
        "http://testnas.local/dav/box/a%2Fb",
    ] {
        let (names, skipped) = listed(
            "/dav/box",
            &[("/dav/box/", true), (slashed, false), ("/dav/box/b", false)],
        );
        assert_eq!(names, ["b"], "{slashed}: only the real `b`");
        assert_eq!(skipped, 1, "{slashed}");
    }
}

/// Names that only look dotted are plain names and list.
#[test]
fn a_name_that_only_starts_or_ends_with_dots_lists() {
    let (names, skipped) = listed(
        "/dav/box",
        &[
            ("/dav/box/", true),
            ("/dav/box/...", false),
            ("/dav/box/.hidden", false),
            ("/dav/box/a.", false),
            ("/dav/box/%2e%2e%2ex", false),
        ],
    );
    assert_eq!(names, ["...", ".hidden", "a.", "...x"]);
    assert_eq!(skipped, 0);
}
