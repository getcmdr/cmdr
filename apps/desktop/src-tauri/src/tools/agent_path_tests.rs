//! The MCP tools' path door: every path-shaped schema property is a declared field, an agent's
//! `..` lands where the volume says (or refuses typed), and a write that leads through a link
//! onto another volume refuses with the place it really leads to. OS-neutral: real links over
//! `AppTestDir`, skipped where this process can't make one.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic, is_escape};
use cmdr_fs::testing::{skip_without_symlinks, symlink};
use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_local::LocalVolume;
use serde_json::{Value, json};

use super::*;
use crate::location::resolve::{IndexSources, LocationInput, LocationMiss, VolumeFact, VolumeFacts, resolve};
use crate::test_support::AppTestDir;
use crate::tools::resources::volumes::VolumeKind;

/// `rel` under `dir`, as an app path.
fn at(dir: &AppPath, rel: &str) -> AppPathBuf {
    let mut out = dir.to_owned();
    for name in rel.split('/').filter(|name| !name.is_empty()) {
        out = out
            .join(AppName::from_app_spelling(name).expect("a name"))
            .expect("joins");
    }
    out
}

/// A local volume rooted at `root`.
fn local_at(root: &AppPath) -> Arc<dyn Volume> {
    Arc::new(LocalVolume::local_folder_at(
        "Test",
        root.to_owned(),
        VolumeHost::detached(),
    ))
}

/// The whole disk as one volume, as the registry hands the boot volume.
fn disk() -> AppHost {
    let root = AppPathBuf::from_native(Path::new(if cfg!(windows) { "C:\\" } else { "/" })).expect("a root");
    let volume = local_at(&root);
    AppHost::over(move |_| Some(Arc::clone(&volume)), Duration::from_secs(10))
}

/// Two volumes, the deepest root holding a path answering for it, over the whole disk.
fn two_volumes(a: &AppPath, b: &AppPath) -> AppHost {
    let volumes = [local_at(a), local_at(b), disk_volume()];
    AppHost::over(
        move |path| {
            volumes
                .iter()
                .filter(|volume| path.is_on_volume(volume.root()))
                .max_by_key(|volume| volume.root().as_str().len())
                .cloned()
        },
        Duration::from_secs(10),
    )
}

fn disk_volume() -> Arc<dyn Volume> {
    local_at(&AppPathBuf::from_native(Path::new(if cfg!(windows) { "C:\\" } else { "/" })).expect("a root"))
}

/// `a/` and `b/` under a scratch dir, `a/link` a folder link to `b/target`, `b/target/inner`
/// holding a file.
fn linked_volumes(name: &str) -> (AppTestDir, AppPathBuf, AppPathBuf) {
    let dir = AppTestDir::new(name);
    fs::create_dir_all(dir.native().join("a").join("real")).expect("a/real");
    fs::create_dir_all(dir.native().join("b").join("target").join("inner")).expect("b/target/inner");
    fs::write(
        dir.native().join("b").join("target").join("inner").join("kept.txt"),
        b"x",
    )
    .expect("kept");
    // Canonical, as the registry's mount roots are: the scratch dir itself may sit behind a
    // link (`/var` is `/private/var` on macOS).
    let native = fs::canonicalize(dir.native()).expect("canonical");
    symlink(native.join("b").join("target"), native.join("a").join("link")).expect("a/link");
    let canonical = AppPathBuf::from_native(&native).expect("an app path");
    let a = at(&canonical, "a");
    let b = at(&canonical, "b");
    (dir, a, b)
}

async fn door(text: &str, using: Use, host: &AppHost) -> Result<String, Refusal> {
    agent_path(text, using, host).await
}

#[tokio::test]
async fn a_path_with_no_dot_dot_crosses_as_its_app_path() {
    let dir = AppTestDir::new("agent_path_plain");
    let plain = at(dir.app_path(), "sub/file.txt");
    assert_eq!(
        door(plain.as_str(), Use::Read, &disk()).await.expect("plain"),
        plain.as_str()
    );
    let slashed = format!("{}/", plain.as_str());
    assert_eq!(
        door(&slashed, Use::Read, &disk()).await.expect("trailing slash"),
        plain.as_str(),
        "a trailing separator goes, so the path names its last name, a link included"
    );
}

#[tokio::test]
async fn a_dot_dot_after_a_folder_link_lands_where_the_os_lands() {
    if skip_without_symlinks() {
        return;
    }
    let (_dir, a, b) = linked_volumes("agent_path_dot_dot");
    let typed = door(&format!("{}/link/../y", a.as_str()), Use::Read, &disk())
        .await
        .expect("resolves");
    // POSIX reads `link/..` as the TARGET's parent; Win32 folds it by text first.
    let expected = if cfg!(windows) { at(&a, "y") } else { at(&b, "y") };
    assert_eq!(typed, expected.as_str());
}

#[tokio::test]
async fn a_dot_dot_above_the_root_is_refused_typed() {
    let text = if cfg!(windows) { "C:/.." } else { "/.." };
    let refusal = door(text, Use::Read, &disk()).await.expect_err("off the root");
    assert_eq!(refusal.token(), "pathOffRoot");
}

#[tokio::test]
async fn a_relative_path_is_refused_typed() {
    let refusal = door("Downloads/x", Use::Read, &disk()).await.expect_err("relative");
    assert_eq!(refusal.token(), "pathRelative");
}

#[tokio::test]
async fn a_literal_escape_range_name_crosses_as_sent_while_escapes_are_not_live() {
    let escape = (0x10_0000..=0x10_FFFF)
        .filter_map(char::from_u32)
        .find(|c| is_escape(*c))
        .expect("an escape-range code point");
    let dir = AppTestDir::new("agent_path_escape");
    let text = format!("{}/caf{escape}.txt", dir.app_path().as_str());
    if !ESCAPES_LIVE {
        assert_eq!(door(&text, Use::Read, &disk()).await.expect("as sent"), text);
        let dotted = format!("{}/x/../caf{escape}.txt", dir.app_path().as_str());
        let refusal = door(&dotted, Use::Read, &disk()).await.expect_err("no typed spelling");
        assert_eq!(refusal.token(), "pathNotAPath");
    }
}

#[tokio::test]
async fn a_write_through_a_link_onto_another_volume_is_refused_with_where_it_leads() {
    if skip_without_symlinks() {
        return;
    }
    let (_dir, a, b) = linked_volumes("agent_path_off_volume");
    let host = two_volumes(&a, &b);
    let through = at(&a, "link/inner");

    // A read follows the link, as the OS does.
    assert_eq!(
        door(through.as_str(), Use::Read, &host).await.expect("read"),
        through.as_str()
    );

    // A destination there writes into `b`, whatever its text says.
    match door(through.as_str(), Use::WriteTarget, &host).await {
        Err(Refusal::LeadsOffVolume { resolved }) => assert_eq!(resolved, at(&b, "target/inner")),
        other => panic!("expected a refusal, got {other:?}"),
    }
    // So is a destination that doesn't exist yet below the link.
    match door(at(&a, "link/new/deeper").as_str(), Use::WriteTarget, &host).await {
        Err(Refusal::LeadsOffVolume { resolved }) => assert_eq!(resolved, at(&b, "target/new/deeper")),
        other => panic!("expected a refusal, got {other:?}"),
    }
    // A source below the link is a file on `b`.
    let refusal = door(at(&a, "link/inner/kept.txt").as_str(), Use::WriteSource, &host)
        .await
        .expect_err("a source on b");
    assert_eq!(refusal.token(), "pathLeadsOffVolume");

    // The link itself is a source on `a`: a trash or a move acts on the link.
    let link = at(&a, "link");
    assert_eq!(
        door(link.as_str(), Use::WriteSource, &host).await.expect("the link"),
        link.as_str()
    );
    // And a plain folder on `a` stays put.
    let real = at(&a, "real");
    assert_eq!(
        door(real.as_str(), Use::WriteTarget, &host).await.expect("on a"),
        real.as_str()
    );
}

/// A volume registered under a root that sits behind a link (`/tmp/x` is `/private/tmp/x` on
/// macOS; a `subst` drive or a mapped share on Windows) still holds its own folders: the
/// physical place is compared against the root's OWN physical place, never its spelling.
/// And a link from it onto a volume nested inside that physical root still refuses.
#[tokio::test]
async fn a_volume_behind_a_link_aliased_root_holds_its_own_writes() {
    if skip_without_symlinks() {
        return;
    }
    let (dir, a, _b) = linked_volumes("agent_path_aliased_root");
    let native = fs::canonicalize(dir.native()).expect("canonical");
    symlink(native.join("a"), native.join("alias")).expect("alias");
    fs::create_dir_all(native.join("a").join("nested")).expect("a/nested");
    symlink(
        native.join("a").join("nested"),
        native.join("a").join("real").join("to_nested"),
    )
    .expect("to_nested");
    let alias = at(&AppPathBuf::from_native(&native).expect("an app path"), "alias");
    let nested = at(&a, "nested");
    let host = two_volumes(&alias, &nested);

    let real = at(&alias, "real/new");
    assert_eq!(
        door(real.as_str(), Use::WriteTarget, &host)
            .await
            .expect("on the aliased volume"),
        real.as_str()
    );
    assert_eq!(
        door(at(&alias, "real").as_str(), Use::WriteSource, &host)
            .await
            .expect("a source there"),
        at(&alias, "real").as_str()
    );
    // Off it through `link`, onto `b` (the disk's, here): refused.
    let refusal = door(at(&alias, "link/x").as_str(), Use::WriteTarget, &host)
        .await
        .expect_err("onto b");
    assert_eq!(refusal.token(), "pathLeadsOffVolume");
    // Onto the volume nested in its physical root: refused, with where it leads.
    match door(at(&alias, "real/to_nested/x").as_str(), Use::WriteTarget, &host).await {
        Err(Refusal::LeadsOffVolume { resolved }) => assert_eq!(resolved, at(&nested, "x")),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A physical place under a name that isn't Unicode compares by its nearest Unicode ancestor,
/// so a link onto another volume's raw-byte folder can't pass as "nothing to compare".
#[test]
fn a_non_unicode_physical_name_compares_by_its_nearest_unicode_ancestor() {
    #[cfg(unix)]
    let (raw, ancestor) = {
        use std::os::unix::ffi::OsStrExt;
        (
            PathBuf::from(std::ffi::OsStr::from_bytes(b"/vol/b/caf\xff/deeper")),
            "/vol/b",
        )
    };
    #[cfg(windows)]
    let (raw, ancestor) = {
        use std::os::windows::ffi::OsStringExt;
        let mut path = PathBuf::from("C:\\vol\\b");
        path.push(std::ffi::OsString::from_wide(&[0x63, 0xD800]));
        path.push("deeper");
        (path, "C:/vol/b")
    };
    assert_eq!(physical_app_path(&raw).expect("an ancestor").as_str(), ancestor);
}

fn local_fact(id: &str, root: &AppPath) -> VolumeFact {
    VolumeFact {
        id: id.to_string(),
        name: id.to_string(),
        kind: VolumeKind::Local,
        root: root.as_str().to_string(),
        live: true,
        connection_state: None,
        earlier_roots: Vec::new(),
        index: IndexSources::none(id),
        share: None,
    }
}

/// The allowed-roots rule a named volume carries (`location/resolve.rs`) judges the RESOLVED
/// place: an agent naming volume `a` and stepping out through `a/link/..` lands where the
/// OS lands, and on POSIX that's `b`, so the call names a place outside `a`.
#[tokio::test]
async fn a_named_volume_judges_the_place_a_dot_dot_through_a_link_lands_on() {
    if skip_without_symlinks() {
        return;
    }
    let (_dir, a, b) = linked_volumes("agent_path_named_volume");
    let host = two_volumes(&a, &b);
    let params =
        json!({ "groups": [{ "destination": { "volumeId": "a", "path": format!("{}/link/../y", a.as_str()) } }] });
    let typed = typed_params_over(fields("propose_suggestions"), &params, &host)
        .await
        .expect("resolves");
    let path = typed["groups"][0]["destination"]["path"]
        .as_str()
        .expect("a path")
        .to_string();

    let facts = VolumeFacts {
        volumes: vec![local_fact("a", &a), local_fact("b", &b)],
        mount_areas: Vec::new(),
    };
    let placed = resolve(
        &facts,
        &LocationInput {
            volume_id: Some("a".into()),
            path: path.clone(),
        },
    );
    if cfg!(windows) {
        assert_eq!(path, at(&a, "y").as_str());
        assert!(placed.is_ok(), "Win32 folds `link/..` by text, so it stays on a");
    } else {
        assert_eq!(path, at(&b, "y").as_str());
        assert!(
            matches!(placed, Err(LocationMiss::PathOutsideVolume { .. })),
            "the text said `a`, the place is on `b`: {placed:?}"
        );
    }
}

#[tokio::test]
async fn nested_fields_resolve_and_everything_else_stays() {
    let dir = AppTestDir::new("agent_path_nested");
    let base = dir.app_path().as_str();
    let params = json!({
        "rationale": "keep ../this text",
        "groups": [
            { "verb": "move", "paths": [format!("{base}/a/../b"), format!("{base}/c/")], "destination": { "path": format!("{base}/d/./e") } },
            { "verb": "trash", "paths": [format!("{base}/f")] }
        ]
    });
    let typed = typed_params_over(fields("propose_suggestions"), &params, &disk())
        .await
        .expect("resolves");
    assert_eq!(typed["rationale"], json!("keep ../this text"));
    assert_eq!(
        typed["groups"][0]["paths"],
        json!([format!("{base}/b"), format!("{base}/c")])
    );
    assert_eq!(typed["groups"][0]["destination"]["path"], json!(format!("{base}/d/e")));
    assert_eq!(typed["groups"][1]["paths"], json!([format!("{base}/f")]));
}

#[tokio::test]
async fn a_refusal_names_the_field_it_came_from() {
    let params = json!({ "paths": ["Downloads"] });
    let err = typed_params_over(fields("inspect_file"), &params, &disk())
        .await
        .expect_err("relative");
    let data = err.data.expect("typed data");
    assert_eq!(data["reason"], json!("pathRelative"));
    assert_eq!(data["field"], json!("/paths/0"));
}

#[tokio::test]
async fn a_scope_resolves_its_paths_and_keeps_its_patterns() {
    let dir = AppTestDir::new("agent_path_scope");
    let base = dir.app_path().as_str();
    let params = json!({ "scope": format!("{base}/x/../y, !node_modules, !{base}/y/./z") });
    let typed = typed_params_over(fields("search"), &params, &disk())
        .await
        .expect("resolves");
    assert_eq!(typed["scope"], json!(format!("{base}/y, !node_modules, !{base}/y/z")));
}

/// The door splits a scope as the search parser does (quotes, `\,`), so no `..` hides in a part
/// it misreads, and it re-spells each part so the parser reads back what the door resolved, a
/// comma in a name included.
#[tokio::test]
async fn a_scope_reads_quotes_and_escapes_as_the_search_parser_does() {
    let dir = AppTestDir::new("agent_path_scope_quoted");
    let base = dir.app_path().as_str();
    let cases = [
        (
            format!("{base}/a\\,b/../c"),
            vec![format!("{base}/c")],
            Vec::<String>::new(),
        ),
        (format!("\"{base}/x,y/../z\""), vec![format!("{base}/z")], Vec::new()),
        (
            format!("'{base}/p,q/./r', !~backup"),
            vec![format!("{base}/p,q/r")],
            vec!["~backup".to_string()],
        ),
    ];
    for (scope, includes, excludes) in cases {
        let params = json!({ "scope": scope });
        let typed = typed_params_over(fields("search"), &params, &disk())
            .await
            .unwrap_or_else(|err| panic!("{scope}: {err:?}"));
        let read = cmdr_search::query::parse_scope(typed["scope"].as_str().expect("a scope"), None);
        assert_eq!(read.include_paths, includes, "{scope}");
        assert_eq!(read.exclude_patterns, excludes, "{scope}");
    }
}

#[tokio::test]
async fn await_reads_its_value_as_a_path_only_for_the_path_condition() {
    let dir = AppTestDir::new("agent_path_await");
    let base = dir.app_path().as_str();
    let path = json!({ "condition": "path", "value": format!("{base}/x/../y") });
    let typed = typed_params_over(fields("await"), &path, &disk())
        .await
        .expect("resolves");
    assert_eq!(typed["value"], json!(format!("{base}/y")));

    let name = json!({ "condition": "has_item", "value": "../weird name" });
    let typed = typed_params_over(fields("await"), &name, &disk())
        .await
        .expect("untouched");
    assert_eq!(*typed, name);
}

/// Property names that hold an app path wherever they appear in a schema.
const PATH_SHAPED: &[&str] = &["path", "paths", "sourcePath", "under", "scope", "parent"];

/// Path-shaped properties that aren't app paths, with why.
const NOT_APP_PATHS: &[(&str, &str)] = &[
    // A relative `.md` path inside the memory folder, jailed by `agent/memory/jail.rs`.
    ("memory_write", "/path"),
    ("memory_edit", "/path"),
    // How much of a disk to take down, an enum.
    ("eject", "/scope"),
    // A server's remote root, on the server's own namespace (checked by the server plan).
    ("propose_server", "/path"),
];

/// Every property in `schema` (pointer form, `*` for array items) whose name is path-shaped.
fn path_shaped_pointers(schema: &Value, at: &str, out: &mut Vec<String>) {
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, sub) in properties {
            let pointer = format!("{at}/{key}");
            if PATH_SHAPED.contains(&key.as_str()) {
                let is_array = sub.get("type").and_then(Value::as_str) == Some("array");
                out.push(if is_array {
                    format!("{pointer}/*")
                } else {
                    pointer.clone()
                });
            }
            path_shaped_pointers(sub, &pointer, out);
        }
    }
    if let Some(items) = schema.get("items") {
        path_shaped_pointers(items, &format!("{at}/*"), out);
    }
    // A branch of a composed schema describes the same value, so its properties sit at `at`.
    for key in ["oneOf", "anyOf", "allOf"] {
        for branch in schema.get(key).and_then(Value::as_array).into_iter().flatten() {
            path_shaped_pointers(branch, at, out);
        }
    }
    // A free-form object's values have no name the door could declare: a schema holding one
    // fails here, so a path can't hide under a key nobody wrote down.
    if let Some(open) = schema.get("additionalProperties").filter(|value| value.is_object()) {
        out.push(format!("{at}/<additionalProperties: {open}>"));
    }
}

/// A new tool (or a new property) holding a path can't slip past the door: it's a declared
/// field, or listed here as not an app path, with why.
#[test]
fn every_path_shaped_schema_property_is_a_declared_field() {
    let mut missing = Vec::new();
    for tool in crate::tools::every_listed_tool() {
        let Some(schema) = crate::tools::tool_schema(&tool.name) else {
            continue;
        };
        let mut pointers = Vec::new();
        path_shaped_pointers(&schema, "", &mut pointers);
        let declared: Vec<&str> = fields(&tool.name).iter().map(|field| field.pointer).collect();
        for pointer in pointers {
            // A nested location object's `path` is declared by its own pointer.
            let covered =
                declared.contains(&pointer.as_str()) || NOT_APP_PATHS.contains(&(tool.name.as_str(), pointer.as_str()));
            if !covered {
                missing.push(format!("{} {pointer}", tool.name));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "path-shaped properties no field declares: {missing:?}"
    );
}
