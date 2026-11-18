//! Path arithmetic, pinned from the shared case table's `arithmetic` rows
//! (`path_grammar_cases.json`): what `std::path` answers today on macOS and Linux for every
//! root shape the app holds, and the answer the typed app path is built to give.
//!
//! Two columns, two jobs:
//!
//! - **`std`** is today's behavior, the byte-identity proof: code that still does its path
//!   arithmetic through `std::path` gets exactly these answers on macOS and Linux. A
//!   conversion away from `std::path` must keep every row's answer, or flip it on purpose
//!   in its own commit (the rows with a `note`). ❌ Never edit a `std` cell to green a
//!   conversion. Unix only: on Windows `std::path` applies Windows rules, the very bug
//!   #1225 removes.
//! - **`target`** (or `refused`) is the typed app path's answer (`crate::app_path`),
//!   asserted against the type itself on every OS, every row.

#[cfg(unix)]
use std::path::{Component, Path};

use serde_json::Value;

use super::app_paths;
use crate::app_path::{AppName, AppPathBuf, PathFlavor};

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Row {
    op: String,
    base: String,
    arg: Option<String>,
    to: Option<String>,
    flavor: String,
    #[serde(default)]
    target: Value,
    refused: Option<String>,
    // Today's `std::path` answer; read on Unix only (see the module doc).
    #[cfg(unix)]
    std: Value,
}

fn rows() -> Vec<Row> {
    #[derive(serde::Deserialize)]
    struct Table {
        arithmetic: Vec<Row>,
    }
    let table: Table = serde_json::from_str(include_str!("path_grammar_cases.json")).expect("the shared cases parse");
    table.arithmetic
}

#[cfg(unix)]
fn text(path: &Path) -> Value {
    Value::from(path.to_str().expect("the table's paths are UTF-8"))
}

fn arg(row: &Row) -> &str {
    row.arg.as_deref().expect("this op takes an `arg`")
}

/// What `std::path` answers for `row` on this OS.
#[cfg(unix)]
fn std_answer(row: &Row) -> Value {
    let base = Path::new(&row.base);
    match row.op.as_str() {
        "parent" => base.parent().map_or(Value::Null, text),
        "fileName" => base
            .file_name()
            .map_or(Value::Null, |name| Value::from(name.to_str().expect("UTF-8"))),
        "names" => Value::from(
            base.components()
                .map(|component| match component {
                    Component::RootDir => "/".to_string(),
                    other => other.as_os_str().to_str().expect("UTF-8").to_string(),
                })
                .collect::<Vec<_>>(),
        ),
        "join" => text(&base.join(arg(row))),
        "startsWith" => Value::from(base.starts_with(arg(row))),
        "stripPrefix" => base.strip_prefix(arg(row)).map_or(Value::Null, text),
        // Today's spelling of these two isn't `std::path`'s: `rebase` is string arithmetic
        // and a shown path is the path's own text (`Path::display`).
        "rebase" => {
            app_paths::rebase(&row.base, arg(row), row.to.as_deref().expect("a `to`")).map_or(Value::Null, Value::from)
        }
        "shown" => Value::from(base.display().to_string()),
        other => panic!("unknown op {other:?}"),
    }
}

/// What the typed app path answers for `row`: its value, or the name of its typed refusal.
fn type_answer(row: &Row) -> Result<Value, String> {
    let flavor = match row.flavor.as_str() {
        "posix" => PathFlavor::Posix,
        "windows" => PathFlavor::Windows,
        other => panic!("unknown flavor {other:?}"),
    };
    let parse = |text: &str| AppPathBuf::parse(text, flavor).map_err(|refusal| format!("{refusal:?}"));
    let base = parse(&row.base)?;
    Ok(match row.op.as_str() {
        "parent" => base.parent().map_or(Value::Null, |parent| Value::from(parent.as_str())),
        "fileName" => base.file_name().map_or(Value::Null, Value::from),
        "names" => Value::from(base.names().collect::<Vec<_>>()),
        "join" => {
            let name = AppName::new(arg(row)).map_err(|_| "NotAChildName".to_string())?;
            Value::from(base.join(name).map_err(|refusal| format!("{refusal:?}"))?.as_str())
        }
        "startsWith" => Value::from(base.starts_with(&parse(arg(row))?)),
        "stripPrefix" => base
            .strip_prefix(&parse(arg(row))?)
            .map_or(Value::Null, |rel| Value::from(rel.as_str())),
        "rebase" => base
            .rebase(&parse(arg(row))?, &parse(row.to.as_deref().expect("a `to`"))?)
            .map_or(Value::Null, |path| Value::from(path.as_str())),
        "shown" => Value::from(base.shown().as_ref()),
        other => panic!("unknown op {other:?}"),
    })
}

fn label(row: &Row) -> String {
    format!("{} {:?} {:?} ({})", row.op, row.base, row.arg, row.flavor)
}

#[test]
#[cfg(unix)]
fn std_path_answers_every_arithmetic_row_as_pinned() {
    for row in rows() {
        assert_eq!(std_answer(&row), row.std, "std::path {}", label(&row));
    }
}

/// The typed app path gives every row's `target`, or refuses it as `refused` names.
#[test]
fn the_typed_app_path_gives_every_rows_target_or_its_refusal() {
    let mut wrong = Vec::new();
    for row in rows() {
        let expected = row.refused.clone().map_or_else(|| Ok(row.target.clone()), Err);
        let answer = type_answer(&row);
        if answer != expected {
            wrong.push(format!("{}: got {answer:?}, pinned {expected:?}", label(&row)));
        }
    }
    assert!(
        wrong.is_empty(),
        "the type departs from the table:\n{}",
        wrong.join("\n")
    );
}

/// Every row says what the type answers, and only the refusals the type has.
#[test]
fn every_arithmetic_row_names_a_target_or_a_typed_refusal() {
    for row in rows() {
        match row.refused.as_deref() {
            None => {}
            Some("DotName" | "NotAChildName" | "Backslash") => {
                assert!(row.target.is_null(), "a refused row has no target: {}", label(&row))
            }
            Some(other) => panic!("unknown refusal {other:?} in {}", label(&row)),
        }
    }
}
