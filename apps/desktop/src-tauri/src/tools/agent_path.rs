//! The one door every MCP tool's path input passes, for both callers, before a handler
//! reads it: the dispatch (`registry.rs`'s `execute_tool`) runs [`typed_params`] right after
//! the schema gate, so no handler sees an agent's raw path text.
//!
//! - **Each `..` is the volume's answer**: the text goes through
//!   `AppHost::registry().resolve_typed(…, TypedBy::Agent)`, so `/x/link/../y` names the
//!   place `ls` names, a `..` above a root is `pathOffRoot`, and a volume that can't answer
//!   refuses. Every refusal is a typed tool error (`data.reason`, [`Refusal::token`]),
//!   ❌ never a silent re-anchor and ❌ never classified by its message.
//! - **A write's place is checked where it really is** ([`Use::WriteTarget`],
//!   [`Use::WriteSource`]): a local path whose PHYSICAL place (links followed, by a
//!   `canonicalize` of its longest existing prefix, compared only, ❌ never echoed) sits on
//!   another volume than its text says is `pathLeadsOffVolume`, carrying `resolved`. So a
//!   proposal's own rules (the named volume's root, writable, mount areas) judge the place
//!   the write would land on. A source's last name isn't followed: a trash or a move acts on
//!   the link itself. Reads follow links as the OS does; the content gate
//!   (`sensitive.rs`) realpaths on its own.
//! - **Which fields are paths** is [`fields`], per tool; `tests.rs` walks every schema so a
//!   new path-shaped property can't slip past it.
//!
//! Design: `docs/specs/app-path-plan.md` § "Typed and pasted `..`"; the rules: `DETAILS.md`
//! § "An agent's path".

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{
    AppNameBuf, AppPath, AppPathBuf, ESCAPES_LIVE, NotAnAppPath, PathArithmetic, PathFlavor, is_escape, is_scheme_url,
};
use cmdr_fs::typed_path::{TypedBy, TypedPathHost, TypedPathRefusal};
use cmdr_fs::volume::VolumeError;
use serde_json::{Value, json};

use super::ToolError;
use crate::go_to_path::AppHost;

/// How a tool uses the place a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Use {
    /// Read, listed, or shown: links followed as the OS follows them.
    Read,
    /// A folder or archive a write lands IN (a destination, a rename's parent, a selector's
    /// root): its physical place must stay on the volume its text names.
    WriteTarget,
    /// A source a write acts ON (moved, trashed, renamed): its folder's physical place must
    /// stay on its volume; its own last name is the item, a link included.
    WriteSource,
}

/// What kind of value sits at a field.
#[derive(Debug, Clone, Copy)]
enum Slot {
    /// One path string.
    Path(Use),
    /// A scope string: comma-separated paths, `!` marking an exclude. Parts that aren't
    /// absolute (a bare `node_modules`) are patterns and stay as typed.
    Scope,
    /// A path only when the sibling at `key` equals `equals` (`await`'s `value` under
    /// `condition: "path"`).
    PathWhen { key: &'static str, equals: &'static str },
}

/// A path field: a JSON pointer into the params, `*` standing for every array item.
#[derive(Debug, Clone, Copy)]
struct Field {
    pointer: &'static str,
    slot: Slot,
}

const fn read(pointer: &'static str) -> Field {
    Field {
        pointer,
        slot: Slot::Path(Use::Read),
    }
}

const fn target(pointer: &'static str) -> Field {
    Field {
        pointer,
        slot: Slot::Path(Use::WriteTarget),
    }
}

const fn source(pointer: &'static str) -> Field {
    Field {
        pointer,
        slot: Slot::Path(Use::WriteSource),
    }
}

const fn scope(pointer: &'static str) -> Field {
    Field {
        pointer,
        slot: Slot::Scope,
    }
}

/// The path fields of `tool`'s params. Empty for a tool that takes none.
fn fields(tool: &str) -> &'static [Field] {
    match tool {
        "nav_to_path" | "unlock" | "favorites" | "list_dir" | "about_item" | "folder_importance" | "dialog" => {
            const F: &[Field] = &[read("/path")];
            F
        }
        "important_folders" => {
            const F: &[Field] = &[read("/under")];
            F
        }
        "inspect_file" | "image_facts" => {
            const F: &[Field] = &[read("/paths/*")];
            F
        }
        "search" | "ai_search" | "open_search_dialog" => {
            const F: &[Field] = &[scope("/scope")];
            F
        }
        "await" => {
            const F: &[Field] = &[Field {
                pointer: "/value",
                slot: Slot::PathWhen {
                    key: "/condition",
                    equals: "path",
                },
            }];
            F
        }
        "propose_suggestions" => {
            const F: &[Field] = &[
                target("/groups/*/destination/path"),
                target("/groups/*/parent"),
                target("/groups/*/selector/root/path"),
                source("/groups/*/paths/*"),
                source("/groups/*/renames/*/path"),
            ];
            F
        }
        "propose_rename_plan" => {
            const F: &[Field] = &[source("/renames/*/sourcePath")];
            F
        }
        _ => &[],
    }
}

/// Why an agent's path names no place a tool acts on. Typed: a tool error carries
/// [`Self::token`] as `data.reason`.
#[derive(Debug)]
pub(crate) enum Refusal {
    /// The resolver's own refusal (`OffRoot`, `Unanswered`, a relative path, …).
    Typed(TypedPathRefusal),
    /// While escapes aren't live, a `..` beside an escape-range code point: no typed
    /// spelling exists for it.
    EscapeBesideDotDot,
    /// A write's place leads through a link onto another volume.
    LeadsOffVolume { resolved: AppPathBuf },
}

impl Refusal {
    /// The `data.reason` token an agent branches on.
    pub(crate) fn token(&self) -> &'static str {
        match self {
            Self::Typed(TypedPathRefusal::Empty) => "pathEmpty",
            Self::Typed(TypedPathRefusal::NoBase) => "pathRelative",
            Self::Typed(TypedPathRefusal::NotAPath(_) | TypedPathRefusal::NotAName) | Self::EscapeBesideDotDot => {
                "pathNotAPath"
            }
            Self::Typed(TypedPathRefusal::OffRoot) => "pathOffRoot",
            Self::Typed(TypedPathRefusal::Unanswered(_)) => "pathUnanswered",
            Self::LeadsOffVolume { .. } => "pathLeadsOffVolume",
        }
    }

    /// The tool error for the text at `field`.
    fn into_error(self, field: &str, text: &str) -> ToolError {
        let token = self.token();
        let mut data = json!({ "reason": token, "field": field, "path": text });
        let error = match self {
            Self::Typed(TypedPathRefusal::NoBase) => ToolError::invalid_params(format!(
                "At {field}, '{text}' is a relative path. Send an absolute path, or one starting with ~."
            )),
            Self::Typed(TypedPathRefusal::OffRoot) => ToolError::invalid_params(format!(
                "At {field}, '{text}' climbs above its volume's root with '..'. Send a path on the volume, as cmdr://state or a listing shows it."
            )),
            Self::Typed(TypedPathRefusal::Unanswered(err)) => ToolError::internal(format!(
                "Couldn't tell where '..' in '{text}' (at {field}) leads: the volume didn't answer ({err}). Retry, or send the folder's own path."
            )),
            Self::Typed(why) => ToolError::invalid_params(format!("At {field}, '{text}' names no place: {why}.")),
            Self::EscapeBesideDotDot => ToolError::invalid_params(format!(
                "At {field}, '{text}' holds a '..' beside a name Cmdr can't resolve it through. Send the folder's own path, as a listing shows it."
            )),
            Self::LeadsOffVolume { resolved } => {
                data["resolved"] = json!(resolved.as_str());
                ToolError::invalid_params(format!(
                    "At {field}, '{text}' leads through a link onto another volume, to {resolved}. Propose that place itself, so the user sees where the change lands."
                ))
            }
        };
        error.with_data(data)
    }
}

/// `params` with every path field of `tool` resolved, or the first refusal as a typed tool
/// error naming the field. Borrowed unchanged when nothing moved.
///
/// # Errors
///
/// [`Refusal`], as a tool error.
pub(crate) async fn typed_params<'p>(tool: &str, params: &'p Value) -> Result<Cow<'p, Value>, ToolError> {
    let fields = fields(tool);
    if fields.is_empty() {
        return Ok(Cow::Borrowed(params));
    }
    typed_params_over(fields, params, &AppHost::registry()).await
}

async fn typed_params_over<'p>(
    fields: &[Field],
    params: &'p Value,
    host: &AppHost,
) -> Result<Cow<'p, Value>, ToolError> {
    let mut out = Cow::Borrowed(params);
    for field in fields {
        if let Slot::PathWhen { key, equals } = field.slot
            && params.pointer(key).and_then(Value::as_str) != Some(equals)
        {
            continue;
        }
        for pointer in concrete_pointers(params, field.pointer) {
            let Some(text) = params.pointer(&pointer).and_then(Value::as_str) else {
                continue;
            };
            // An empty value is the handler's to refuse in its own words ("Missing 'path'").
            if text.is_empty() {
                continue;
            }
            let typed = match field.slot {
                Slot::Path(using) => agent_path(text, using, host)
                    .await
                    .map_err(|refusal| refusal.into_error(&pointer, text))?,
                Slot::PathWhen { .. } => agent_path(text, Use::Read, host)
                    .await
                    .map_err(|refusal| refusal.into_error(&pointer, text))?,
                Slot::Scope => typed_scope(text, host, &pointer).await?,
            };
            if typed != text
                && let Some(slot) = out.to_mut().pointer_mut(&pointer)
            {
                *slot = Value::String(typed);
            }
        }
    }
    Ok(out)
}

/// Every pointer `pattern` names in `params`, each `*` expanded over the array there.
fn concrete_pointers(params: &Value, pattern: &str) -> Vec<String> {
    let mut found = vec![String::new()];
    for segment in pattern.split('/').skip(1) {
        let mut next = Vec::new();
        for prefix in found {
            if segment == "*" {
                let len = params.pointer(&prefix).and_then(Value::as_array).map_or(0, Vec::len);
                next.extend((0..len).map(|i| format!("{prefix}/{i}")));
            } else {
                next.push(format!("{prefix}/{segment}"));
            }
        }
        found = next;
    }
    found
}

/// A scope string with each absolute part resolved; patterns and the `!` marks stay. Split and
/// re-spelled by the search parser's own rules (`cmdr_search::query`: quotes, `\` escapes), so
/// a `..` can't hide in a quoted or escaped part, and a resolved name holding a `,` reads back
/// as one part.
async fn typed_scope(text: &str, host: &AppHost, pointer: &str) -> Result<String, ToolError> {
    use cmdr_search::query::{escape_scope_value, parse_scope, split_scope_segments};
    let mut parts = Vec::new();
    for segment in split_scope_segments(text) {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (mark, body) = match trimmed.strip_prefix('!') {
            Some(body) => ("!", body.trim_start()),
            None => ("", trimmed),
        };
        let typed = if looks_absolute(body) {
            agent_path(body, Use::Read, host)
                .await
                .map_err(|refusal| refusal.into_error(pointer, body))?
        } else {
            body.to_string()
        };
        parts.push(format!("{mark}{}", escape_scope_value(&typed)));
    }
    let typed = parts.join(", ");
    // Unchanged in meaning: hand the agent's own spelling on.
    let (before, after) = (parse_scope(text, None), parse_scope(&typed, None));
    if before.include_paths == after.include_paths && before.exclude_patterns == after.exclude_patterns {
        return Ok(text.to_string());
    }
    Ok(typed)
}

/// Whether a scope part is a path rather than a pattern: `/…`, `~` or `~/…`, a scheme, or a
/// drive. A `~name` is a pattern, as the search reads a bare name. (The search parser has
/// already read every `\` as an escape, so no `~\…` reaches here.)
fn looks_absolute(text: &str) -> bool {
    text.starts_with('/')
        || text == "~"
        || text.starts_with("~/")
        || text.starts_with('\\')
        || is_scheme_url(text)
        || (text.len() >= 2 && text.as_bytes()[1] == b':' && text.as_bytes()[0].is_ascii_alphabetic())
}

/// One path a VIEW reads (a `cmdr://` resource's query value, `key` naming it), through the
/// same door as a tool's [`Use::Read`] field: the resolved text, or the typed tool error.
///
/// # Errors
///
/// [`Refusal`], as a tool error naming `key`.
pub(crate) async fn read_path(key: &str, text: &str, host: &AppHost) -> Result<String, ToolError> {
    agent_path(text, Use::Read, host)
        .await
        .map_err(|refusal| refusal.into_error(key, text))
}

/// One volume-relative store path a VIEW reads (`cmdr://importance`'s `under` on a named
/// share's store, `key` naming it): POSIX text, lexical, since no live volume may be there to
/// ask. A `..` has no volume to answer it, so it refuses, as does a `~` or a relative path.
///
/// # Errors
///
/// [`Refusal`], as a tool error naming `key`.
pub(crate) fn store_path(key: &str, text: &str) -> Result<String, ToolError> {
    let parsed = if crate::commands::wire_path::has_dot_dot_name(text) {
        Err(NotAnAppPath::DotName)
    } else {
        AppPathBuf::parse(text, PathFlavor::Posix)
    };
    parsed
        .map(AppPathBuf::into_string)
        .map_err(|why| Refusal::Typed(TypedPathRefusal::NotAPath(why)).into_error(key, text))
}

/// One agent path, resolved: the app path's text the tool takes.
///
/// # Errors
///
/// [`Refusal`].
pub(crate) async fn agent_path(text: &str, using: Use, host: &AppHost) -> Result<String, Refusal> {
    let has_dot_dot = crate::commands::wire_path::has_dot_dot_name(text);
    // While no producer makes a live escape, an escape-range code point is LITERAL, and the
    // resolver's typed spelling would escape it: the handlers read text, so it stays as sent.
    if !ESCAPES_LIVE && text.chars().any(is_escape) {
        if has_dot_dot {
            return Err(Refusal::EscapeBesideDotDot);
        }
        return Ok(host.expand_tilde(text));
    }
    // A POSIX `/x` on Windows is a volume-relative path (a share, a phone), never this
    // host's: read with POSIX rules, and with no volume to ask, a `..` there is refused.
    if PathFlavor::HOST == PathFlavor::Windows && posix_shaped(text) {
        if has_dot_dot {
            return Err(Refusal::Typed(TypedPathRefusal::NotAPath(NotAnAppPath::DotName)));
        }
        return AppPathBuf::parse(text, PathFlavor::Posix)
            .map(AppPathBuf::into_string)
            .map_err(|why| Refusal::Typed(TypedPathRefusal::NotAPath(why)));
    }
    let typed = host
        .resolve_typed(text, None, TypedBy::Agent)
        .await
        .map_err(Refusal::Typed)?;
    if using != Use::Read
        && let Some(resolved) = off_volume(&typed.path, using == Use::WriteTarget, host).await?
    {
        return Err(Refusal::LeadsOffVolume { resolved });
    }
    Ok(typed.path.into_string())
}

/// Where a write INTO `path` really lands when a link leads it onto another volume than its
/// text names, else `None`: the [`Use::WriteTarget`] check a proposal passed at creation,
/// re-run each time its review renders, since a link can appear under a destination while the
/// group waits. Disclosure only: ❌ never a refusal at execution, where an approved group runs
/// as if the person had started it (`agent/suggested_ops/CLAUDE.md`). A walk that can't finish
/// (a hung mount past the host's deadline) is [`Landing::Unchecked`], ❌ never "lands as named":
/// silence there would read as the drive the card names.
pub(crate) async fn write_lands_off_volume(path: &AppPath, host: &AppHost) -> Landing {
    match off_volume(path, true, host).await {
        Ok(None) => Landing::AsNamed,
        Ok(Some(physical)) => Landing::OffVolume(physical),
        Err(_) => Landing::Unchecked,
    }
}

/// Where a write into a destination lands ([`write_lands_off_volume`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Landing {
    /// On the volume its text names (or no local place to walk: a server's links are its own).
    AsNamed,
    /// A link leads it onto another volume: this place.
    OffVolume(AppPathBuf),
    /// The walk didn't finish, so where it lands is unknown.
    Unchecked,
}

/// `/x` but not `//server` or a scheme.
fn posix_shaped(text: &str) -> bool {
    text.starts_with('/') && !text.starts_with("//")
}

/// The physical place of `path` when it sits on another volume than its text does, else
/// `None`. Only a local path has a physical place to read here: a scheme path's links are
/// its server's, and SFTP's `..` already refuses a target off its root.
///
/// Same volume when the registry's roots holding both places match by text, or when the
/// physical place sits under the text volume's OWN physical root with no other registered
/// root nested between: a root registered behind a link (`/tmp/x` is `/private/tmp/x` on
/// macOS; a `subst` drive or a mapped share, which `canonicalize` spells `\\?\UNC\…`, on
/// Windows) then still holds its own folders, and a link onto a volume mounted inside it
/// still refuses. Fails closed: a walk that can't finish (its task gone, the deadline
/// passed) is `Unanswered`.
async fn off_volume(path: &AppPath, follow_last: bool, host: &AppHost) -> Result<Option<AppPathBuf>, Refusal> {
    let Ok(native) = path.to_native() else {
        return Ok(None);
    };
    let text_root = host.root_holding(path);
    let root_native = text_root.as_deref().and_then(|root| root.to_native().ok());
    let unanswered = |err: VolumeError| Refusal::Typed(TypedPathRefusal::Unanswered(err));
    let (physical, physical_root) = host
        .within(async move {
            let walk = tokio::task::spawn_blocking(move || {
                let root = root_native.and_then(|root| std::fs::canonicalize(root).ok());
                (physical_native(&native, follow_last), root)
            });
            walk.await.map_err(|err| VolumeError::IoError {
                message: format!("the link walk stopped: {err}").into(),
                raw_os_error: None,
            })
        })
        .await
        .map_err(unanswered)?;
    let Some(physical) = physical.and_then(|p| physical_app_path(&p)) else {
        return Ok(None);
    };
    let held_by = host.root_holding(&physical);
    if held_by == text_root {
        return Ok(None);
    }
    if let Some(physical_root) = physical_root.and_then(|p| physical_app_path(&p))
        && physical.is_on_volume(&physical_root)
        && !held_by.is_some_and(|held| held != physical_root && held.is_on_volume(&physical_root))
    {
        return Ok(None);
    }
    Ok(Some(physical))
}

/// `native` with every link in its longest existing prefix followed (`canonicalize`), the
/// missing rest joined on as named; `follow_last: false` keeps the last name as the item
/// itself. `None` when nothing of it exists.
fn physical_native(native: &Path, follow_last: bool) -> Option<PathBuf> {
    let (start, last) = if follow_last {
        (native, None)
    } else {
        (native.parent()?, native.file_name())
    };
    let mut probe = start;
    let mut missing = Vec::new();
    let mut physical = loop {
        match std::fs::canonicalize(probe) {
            Ok(found) => break found,
            Err(_) => {
                missing.push(probe.file_name()?);
                probe = probe.parent()?;
            }
        }
    };
    for name in missing.into_iter().rev().chain(last) {
        physical.push(name);
    }
    Some(physical)
}

/// A physical native path as an app path, for the comparison only. A name that isn't Unicode
/// (a share's or an ext4 disk's raw bytes) has no app spelling while escapes aren't live, so
/// its nearest Unicode ancestor stands in: every registered root is an app path, so the
/// volume holding that ancestor is the one holding the place, and a link onto another
/// volume's non-Unicode folder can't pass as "no comparison".
fn physical_app_path(physical: &Path) -> Option<AppPathBuf> {
    let mut place = physical;
    loop {
        if let Some(found) = unicode_app_path(place) {
            return Some(found);
        }
        place = place.parent()?;
    }
}

fn unicode_app_path(physical: &Path) -> Option<AppPathBuf> {
    let mut names = Vec::new();
    let mut base = physical;
    // A verbatim root (`\\?\C:\`) parses whole; its names join one by one.
    while AppPathBuf::from_native_unicode(base).is_err() {
        names.push(AppNameBuf::from_native_os_unicode(base.file_name()?).ok()?);
        base = base.parent()?;
    }
    let mut out = AppPathBuf::from_native_unicode(base).ok()?;
    for name in names.iter().rev() {
        out = out.join(name.as_app_name()).ok()?;
    }
    Some(out)
}

#[cfg(test)]
#[path = "agent_path_tests.rs"]
mod tests;
