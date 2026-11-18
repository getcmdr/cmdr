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
//! - **`target`** (or `refused`) is the typed app path's answer. Today's grammar
//!   (`path_grammar`) already gives it for the local roots, on every OS, so those rows
//!   are asserted against it here; the scheme rows wait for the type.

#[cfg(unix)]
use std::path::{Component, Path};

use serde_json::Value;

use super::app_paths;
use super::path_grammar::{AppPath, PathFlavor, basename_in_volume, parent_in_volume, shown_path};

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

/// What today's grammar answers for `row`, when it already speaks this root and op.
fn grammar_answer(row: &Row) -> Option<Value> {
    let flavor = match row.flavor.as_str() {
        "posix" => PathFlavor::Posix,
        "windows" => PathFlavor::Windows,
        other => panic!("unknown flavor {other:?}"),
    };
    let parse = |text: &str| AppPath::parse(text, flavor);
    // The grammar has no scheme roots yet: those rows are the type's.
    let base = parse(&row.base)?;
    Some(match row.op.as_str() {
        "parent" => parent_in_volume(&row.base, flavor).map_or(Value::Null, Value::from),
        "fileName" => basename_in_volume(&row.base, flavor)
            .filter(|name| !name.is_empty())
            .map_or(Value::Null, Value::from),
        "names" => Value::from(base.names().to_vec()),
        "join" => Value::from(base.join(arg(row)).to_string()),
        "startsWith" => Value::from(base.starts_with(&parse(arg(row))?)),
        "rebase" => {
            app_paths::rebase(&row.base, arg(row), row.to.as_deref().expect("a `to`")).map_or(Value::Null, Value::from)
        }
        "shown" => Value::from(shown_path(&row.base, flavor)),
        // No `strip_prefix` in today's grammar.
        _ => return None,
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

#[test]
fn todays_grammar_gives_the_target_answer_where_it_speaks_the_root() {
    let mut checked = 0;
    for row in rows().into_iter().filter(|row| row.refused.is_none()) {
        if let Some(answer) = grammar_answer(&row) {
            assert_eq!(answer, row.target, "grammar {}", label(&row));
            checked += 1;
        }
    }
    assert!(
        checked >= 40,
        "the local-root rows are the grammar's ({checked} checked)"
    );
}

/// Every row says what the type answers, and only the refusals the type has.
#[test]
fn every_arithmetic_row_names_a_target_or_a_typed_refusal() {
    for row in rows() {
        match row.refused.as_deref() {
            None => {}
            Some("DotName" | "NotAChildName") => {
                assert!(row.target.is_null(), "a refused row has no target: {}", label(&row))
            }
            Some(other) => panic!("unknown refusal {other:?} in {}", label(&row)),
        }
    }
}
