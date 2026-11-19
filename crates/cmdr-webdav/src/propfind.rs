//! Reading a PROPFIND `multistatus` answer (RFC 4918 § 9.1) into plain values.
//!
//! Namespace-aware on purpose: the `DAV:` namespace arrives as `D:` from
//! Apache, `d:` from Nextcloud and ownCloud, `a:` from some NAS firmware, and
//! as the default namespace from others. Only the local name plus the
//! resolved namespace decides what an element is.

use percent_encoding::percent_decode_str;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

/// The namespace every WebDAV property lives in.
const DAV: &str = "DAV:";

/// ownCloud's namespace, which Nextcloud inherited at the fork and still uses.
/// The one property read from it is `size`; `crates/cmdr-webdav/DETAILS.md`
/// § What a real server answers carries why a non-standard property earns its
/// place here.
const OC: &str = "http://owncloud.org/ns";

/// How an `oc:` name is held on the element stack. DAV local names never
/// contain a colon, so the prefix keeps the two namespaces from colliding.
const OC_PREFIX: &str = "oc:";

/// Nextcloud's own namespace. The one property read from it is
/// `creation_time`, and only to learn whether the server keeps creation dates
/// (`DETAILS.md` § "Dates").
const NC: &str = "http://nextcloud.org/ns";

/// How an `nc:` name is held on the element stack, like [`OC_PREFIX`].
const NC_PREFIX: &str = "nc:";

/// One `response` element, with the properties this backend asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PropfindEntry {
    /// The response's `href`, percent-DECODED and reduced to its path when the
    /// server answered with an absolute URL. Dot segments stay as sent (a member a
    /// server names `..` must still read as `..`, ❌ never resolved to its parent).
    pub href: String,
    /// Whether the `href`'s last segment decoded to text holding a `/` (an encoded
    /// `%2F`): the decoded path splits there, so its last name isn't the member's.
    pub name_holds_slash: bool,
    /// Whether `resourcetype` carried a `collection`.
    pub is_collection: bool,
    /// `getcontentlength`, absent on collections and on servers that omit it.
    pub size: Option<u64>,
    /// `getlastmodified` as Unix seconds.
    pub modified_at: Option<u64>,
    /// `creationdate` as Unix seconds.
    pub created_at: Option<u64>,
    /// `quota-available-bytes` (RFC 4331). Negative on Nextcloud for
    /// "unlimited", hence signed.
    pub quota_available: Option<i64>,
    /// `quota-used-bytes` (RFC 4331).
    pub quota_used: Option<i64>,
    /// `oc:size`, the bytes a collection holds counting everything under it.
    /// Nextcloud and ownCloud only; absent everywhere else. Unsigned because
    /// those servers report a real count here rather than RFC 4331's sentinels,
    /// so a negative value is malformed and parses to `None`.
    pub oc_size: Option<u64>,
    /// `nc:creation_time`, Unix seconds (`0` for none recorded). Nextcloud only,
    /// and asked for only by the connect probe: its presence is how a server
    /// says it keeps creation dates.
    pub nc_creation_time: Option<u64>,
}

/// The body didn't parse as a `multistatus`.
#[derive(Debug)]
pub(crate) struct NotMultistatus;

/// Parses a `multistatus` body. Properties are taken only from `propstat`
/// blocks whose `status` is 2xx: a 404 propstat lists what the server does NOT
/// know, and reading a size out of it would invent one.
pub(crate) fn parse_multistatus(body: &str) -> Result<Vec<PropfindEntry>, NotMultistatus> {
    let mut reader = NsReader::from_str(body);
    let mut entries = Vec::new();
    let mut saw_multistatus = false;
    // Where the cursor is, as a stack of DAV: local names.
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<PropfindEntry> = None;
    let mut pending = PropfindEntry::default();
    let mut pending_status_ok = true;
    let mut text = String::new();

    loop {
        let (ns, event) = reader.read_resolved_event().map_err(|_| NotMultistatus)?;
        match event {
            Event::Start(start) => {
                let local = stack_name(&ns, start.local_name().as_ref());
                match local.as_str() {
                    "multistatus" => saw_multistatus = true,
                    "response" => {
                        current = Some(PropfindEntry::default());
                    }
                    "propstat" => {
                        pending = PropfindEntry::default();
                        pending_status_ok = true;
                    }
                    "collection" if stack.last().is_some_and(|parent| parent == "resourcetype") => {
                        pending.is_collection = true;
                    }
                    _ => {}
                }
                text.clear();
                stack.push(local);
            }
            Event::Empty(start) => {
                let in_dav = matches!(ns, ResolveResult::Bound(namespace) if namespace.as_ref() == DAV);
                if in_dav
                    && start.local_name().as_ref() == "collection"
                    && stack.last().is_some_and(|parent| parent == "resourcetype")
                {
                    pending.is_collection = true;
                }
            }
            // A text node never holds an entity: the reader splits `a&amp;b`
            // into `Text(a)`, `GeneralRef(amp)`, `Text(b)`, and the text derefs
            // to the raw `str` between them (verified on `quick-xml` 0.42.0, the
            // `an_escaped_entity_is_unescaped_exactly_once` test, 2026-10-12). Unescaping the
            // text would find nothing; resolving the reference is what keeps
            // the `&`.
            Event::Text(t) => text.push_str(&t),
            Event::GeneralRef(r) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(resolved) = resolve_predefined_entity(&r) {
                    text.push_str(resolved);
                }
            }
            Event::CData(c) => text.push_str(&c),
            Event::End(_) => {
                let Some(local) = stack.pop() else {
                    return Err(NotMultistatus);
                };
                let value = text.trim().to_string();
                match local.as_str() {
                    "href" if stack.last().is_some_and(|parent| parent == "response") => {
                        if let Some(entry) = current.as_mut() {
                            entry.href = decode_href(&value);
                            entry.name_holds_slash = href_name_holds_slash(&value);
                        }
                    }
                    "status" if stack.last().is_some_and(|parent| parent == "propstat") => {
                        pending_status_ok = status_is_success(&value);
                    }
                    "getcontentlength" => pending.size = value.parse().ok(),
                    "getlastmodified" => {
                        pending.modified_at = httpdate::parse_http_date(&value).ok().and_then(unix_secs);
                    }
                    // ❗ Nextcloud answers 1970-01-01 for an item with no creation
                    // time recorded (everything not uploaded with `X-OC-CTime`):
                    // that's "unknown", or a copy off the server would stamp it.
                    "creationdate" => pending.created_at = parse_rfc3339(&value).filter(|&secs| secs != 0),
                    "quota-available-bytes" => pending.quota_available = value.parse().ok(),
                    "quota-used-bytes" => pending.quota_used = value.parse().ok(),
                    "oc:size" => pending.oc_size = value.parse().ok(),
                    "nc:creation_time" => pending.nc_creation_time = value.parse().ok(),
                    "propstat" => {
                        if pending_status_ok && let Some(entry) = current.as_mut() {
                            merge(entry, &pending);
                        }
                    }
                    "response" => {
                        if let Some(entry) = current.take() {
                            entries.push(entry);
                        }
                    }
                    _ => {}
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_multistatus {
        return Err(NotMultistatus);
    }
    Ok(entries)
}

/// Whether a PROPPATCH answer (a `multistatus`, RFC 4918 § 9.2) applied every
/// property it was sent: true when each `propstat` it holds is 2xx. A PROPPATCH
/// is atomic, so one refusal means none took (the rest come back `424`).
pub(crate) fn every_propstat_succeeded(body: &str) -> Result<bool, NotMultistatus> {
    let mut reader = NsReader::from_str(body);
    let mut saw_multistatus = false;
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut all = true;
    loop {
        let (ns, event) = reader.read_resolved_event().map_err(|_| NotMultistatus)?;
        match event {
            Event::Start(start) => {
                let local = stack_name(&ns, start.local_name().as_ref());
                if local == "multistatus" {
                    saw_multistatus = true;
                }
                text.clear();
                stack.push(local);
            }
            Event::Text(t) => text.push_str(&t),
            Event::End(_) => {
                let Some(local) = stack.pop() else {
                    return Err(NotMultistatus);
                };
                if local == "status" && stack.last().is_some_and(|parent| parent == "propstat") {
                    all &= status_is_success(text.trim());
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if saw_multistatus { Ok(all) } else { Err(NotMultistatus) }
}

/// Folds one 2xx `propstat`'s values into the response they belong to.
fn merge(entry: &mut PropfindEntry, from: &PropfindEntry) {
    entry.is_collection |= from.is_collection;
    entry.size = entry.size.or(from.size);
    entry.modified_at = entry.modified_at.or(from.modified_at);
    entry.created_at = entry.created_at.or(from.created_at);
    entry.quota_available = entry.quota_available.or(from.quota_available);
    entry.quota_used = entry.quota_used.or(from.quota_used);
    entry.oc_size = entry.oc_size.or(from.oc_size);
    entry.nc_creation_time = entry.nc_creation_time.or(from.nc_creation_time);
}

/// How an element is held on the stack: its local name for a `DAV:` element,
/// the same prefixed with `oc:` for an ownCloud one and `nc:` for a Nextcloud one, and empty for a namespace
/// this parser reads nothing from.
fn stack_name(ns: &ResolveResult, local: &str) -> String {
    match ns {
        ResolveResult::Bound(namespace) if namespace.as_ref() == DAV => local.to_string(),
        ResolveResult::Bound(namespace) if namespace.as_ref() == OC => format!("{OC_PREFIX}{local}"),
        ResolveResult::Bound(namespace) if namespace.as_ref() == NC => format!("{NC_PREFIX}{local}"),
        _ => String::new(),
    }
}

/// `HTTP/1.1 200 OK` → whether the code is 2xx. The code is parsed as a number;
/// the reason phrase is never looked at.
fn status_is_success(line: &str) -> bool {
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .is_some_and(|code| (200..300).contains(&code))
}

/// The path half of an `href`, percent-decoded. Servers may answer with an
/// absolute URL (RFC 4918 § 8.3 allows either).
///
/// ❗ Dot segments stay as the server sent them, absolute URL or not: a URL parser
/// resolves `box/%2e%2e` to `box`'s parent, which would list a member a server
/// named `..` under the parent's name. The listing drops a `.` or `..` member by
/// its own name (`volume::mapping::members`).
pub(crate) fn decode_href(href: &str) -> String {
    percent_decode_str(href_path(href)).decode_utf8_lossy().into_owned()
}

/// Whether `href`'s last segment, decoded, holds a `/` (an encoded `%2F`), so the
/// decoded path's last name isn't the member's own.
pub(crate) fn href_name_holds_slash(href: &str) -> bool {
    let path = href_path(href).trim_end_matches('/');
    let last = path.rsplit('/').next().unwrap_or_default();
    percent_decode_str(last).any(|byte| byte == b'/')
}

/// An `href`'s path as the server spelled it, still encoded: an absolute URL's
/// path after its authority (its query and fragment off), a path as it is.
fn href_path(href: &str) -> &str {
    let Some(scheme_end) = href.find("://") else {
        return href;
    };
    let scheme = &href[..scheme_end];
    let is_scheme = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !is_scheme {
        return href;
    }
    let after_authority = &href[scheme_end + 3..];
    let Some(path_at) = after_authority.find(['/', '?', '#']) else {
        return "/";
    };
    let path = &after_authority[path_at..];
    let path = path.split(['?', '#']).next().unwrap_or_default();
    if path.is_empty() { "/" } else { path }
}

fn unix_secs(time: std::time::SystemTime) -> Option<u64> {
    time.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// `2024-03-01T12:34:56Z` (or with a `+HH:MM` offset, or fractional seconds)
/// as Unix seconds. Anything that doesn't fit the shape is `None`.
fn parse_rfc3339(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 19 || bytes[10] != b'T' {
        return None;
    }
    let num = |from: usize, to: usize| value.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let rest = &value[19..];
    let rest = rest.find(['Z', '+', '-']).map_or("", |at| &rest[at..]);
    let offset_secs = match rest.as_bytes().first() {
        None | Some(b'Z') => 0,
        Some(sign) => {
            let oh = rest.get(1..3)?.parse::<i64>().ok()?;
            let om = rest.get(4..6)?.parse::<i64>().ok()?;
            let total = oh * 3600 + om * 60;
            if *sign == b'-' { -total } else { total }
        }
    };
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset_secs;
    u64::try_from(secs).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
#[path = "propfind_test.rs"]
mod propfind_test;
