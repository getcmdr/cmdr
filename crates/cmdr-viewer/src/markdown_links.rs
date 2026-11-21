//! Links in a rendered Markdown document, and the relative-reference rule they share
//! with its images.
//!
//! [`resolve_link`] is pure: from an `href` alone it decides whether a click goes to
//! the web or a mail app, to a spot in the same document, to another file on the
//! document's volume (which the viewer opens in the same window), or nowhere.
//! [`resolve_relative`] is the confinement both links and images go through: a
//! reference resolves against the document's folder as a URL rooted at the volume, so
//! it can never name a file off that volume. `DETAILS.md` § "Markdown rendering".

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic, RelativeAppPath};
use serde::Serialize;

use super::markdown_render::ID_PREFIX;

/// What following one link in a rendered Markdown document means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MarkdownLink {
    /// An `http(s)` or `mailto` URL, for the user's browser or mail app.
    External { url: String },
    /// A spot in the same document: the element with this id, or the top when `None`.
    Anchor { element_id: Option<String> },
    /// Another file on the document's volume, opened in the same viewer window, then
    /// scrolled to `element_id` if the link named one.
    File { path: String, element_id: Option<String> },
    /// Nothing the viewer follows: an absolute path, `file:`, another scheme, or empty.
    Blocked,
}

/// A relative reference resolved on the document's volume.
pub(super) struct RelativeTarget {
    pub(super) path: AppPathBuf,
    /// The fragment, percent-decoded, as written (no `md-` prefix).
    pub(super) fragment: Option<String>,
}

/// Resolves a link `href` clicked in `document` (a path on the volume rooted at
/// `volume_root`). Pure: no I/O, so a missing target is the open's to report. `document`
/// is `None` for a path the app path type refuses (a `..` in it), whose relative links go
/// nowhere.
///
/// A bare `#fragment` arrives already rewritten to the rendered id (`#md-install`,
/// `markdown_render.rs`); a fragment on a relative link gets the prefix here. Absolute
/// paths and `file:` are blocked for the same reason they are for images: on GitHub
/// `/docs/a.md` means the repository root, which the viewer doesn't know.
pub(super) fn resolve_link(href: &str, document: Option<&AppPath>, volume_root: &AppPath) -> MarkdownLink {
    let href = href.trim();
    if let Some(fragment) = href.strip_prefix('#') {
        return MarkdownLink::Anchor {
            element_id: non_empty(decode(fragment)),
        };
    }
    if href.is_empty() || (href.starts_with('/') && !href.starts_with("//")) {
        return MarkdownLink::Blocked;
    }
    if let Some(rest) = href.strip_prefix("//") {
        return external(&format!("https://{rest}"));
    }
    match url::Url::parse(href) {
        Ok(url) if matches!(url.scheme(), "http" | "https" | "mailto") => {
            MarkdownLink::External { url: url.to_string() }
        }
        Ok(_) => MarkdownLink::Blocked,
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            match document.and_then(|document| resolve_relative(href, document, volume_root)) {
                Some(target) => {
                    let element_id = target.fragment.and_then(non_empty).map(|id| format!("{ID_PREFIX}{id}"));
                    if Some(&*target.path) == document {
                        MarkdownLink::Anchor { element_id }
                    } else {
                        MarkdownLink::File {
                            path: target.path.into_string(),
                            element_id,
                        }
                    }
                }
                None => MarkdownLink::Blocked,
            }
        }
        Err(_) => MarkdownLink::Blocked,
    }
}

fn external(url: &str) -> MarkdownLink {
    match url::Url::parse(url) {
        Ok(url) => MarkdownLink::External { url: url.to_string() },
        Err(_) => MarkdownLink::Blocked,
    }
}

/// Joins a relative reference onto the document's folder as a URL rooted at the volume,
/// so URL resolution does the dot segments, the query, and the escapes, and its own rule
/// (a `..` at the root stays at the root) is the confinement. `None` when the reference
/// names the volume root itself, the document isn't under `volume_root`, or a decoded name
/// is no name ([`names_of`]).
///
/// App-path arithmetic, the same on every OS and in the browser: the folder's names become
/// the URL's segments, and the joined URL's segments become names under the root again, so
/// no host `std::path` reading (on Windows, `\` and `C:`) touches a path on a phone, in a
/// zip, or on a share.
pub(super) fn resolve_relative(reference: &str, document: &AppPath, volume_root: &AppPath) -> Option<RelativeTarget> {
    let folder = document.parent()?.strip_prefix(volume_root)?;
    let base = folder_url(&folder)?;
    let joined = base.join(reference).ok().filter(|joined| joined.scheme() == "file")?;
    let relative = names_of(&joined)?;
    if relative.is_empty() {
        return None;
    }
    Some(RelativeTarget {
        path: volume_root.join_relative(RelativeAppPath::new(&relative).ok()?).ok()?,
        fragment: joined.fragment().map(decode),
    })
}

/// `folder` (relative to the volume root) as a `file:` directory URL, each name a segment
/// that `url` percent-encodes itself: the URL `Url::from_directory_path` built from the
/// same names, so the URL's own rules (a `C:` segment's drive-letter quirk too) answer as
/// they did, and the joined URL's segments decode back to the same names.
fn folder_url(folder: &RelativeAppPath) -> Option<url::Url> {
    let mut url = url::Url::parse("file:///").ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty();
        segments.extend(folder.names());
        segments.push("");
    }
    Some(url)
}

/// A joined `file:` URL's path, decoded, as `/`-joined names under the volume root.
///
/// The names `url::Url::to_file_path` and `Path::components` read from it: `None` for a
/// URL naming a host, and an empty or `.` name drops (an escaped `/` splits a segment). And
/// `None` for a decoded name that is no name, where `std::path` handed the volume something
/// else: `..` (an escaped `/` let `..%2F..` climb past the root, which the URL's own dot
/// rule doesn't see), bytes that aren't UTF-8 (a link named their U+FFFD sibling, an image
/// the raw bytes, and an app path holds neither until escapes go live), a NUL, or escapes
/// that aren't canonical. A canonical escape-range code point passes as the text it was
/// (a link's answer is that text); an image refuses it ([`resolve_image_src`]).
///
/// [`resolve_image_src`]: super::markdown_images::resolve_image_src
fn names_of(url: &url::Url) -> Option<String> {
    if url.host().is_some_and(|host| host != url::Host::Domain("localhost")) {
        return None;
    }
    let mut relative = String::new();
    for segment in url.path_segments()? {
        let decoded = String::from_utf8(urlencoding::decode_binary(segment.as_bytes()).into_owned()).ok()?;
        for name in decoded.split('/').filter(|name| !matches!(*name, "" | ".")) {
            AppName::from_app_spelling(name).ok()?;
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str(name);
        }
    }
    Some(relative)
}

fn decode(escaped: &str) -> String {
    String::from_utf8_lossy(&urlencoding::decode_binary(escaped.as_bytes())).into_owned()
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}
