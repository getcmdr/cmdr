//! Where each top-level source of a copy or move lands, by name.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

/// The name each top-level source of a copy or move takes inside the
/// destination folder: its own, the one leaf a single source's caller chose,
/// or a name per source. The destination argument still names the parent.
///
/// Every route asks [`Self::leaf_of`] (or [`Self::chosen_for`] where another
/// rename may also apply), so no route can honor a name another forgets. The
/// routed start validates it once, before the operation is admitted
/// (`leaf_validation::validate_transfer_leaves`), and nothing downstream checks it
/// again: a source the binding or the queue prunes later simply never asks.
///
/// On the wire it is the optional `destinationName`, a single leaf. The
/// per-source form is never on the wire: only Rust builds it, and serializing
/// one refuses rather than drop its names.
/// `transfer/DETAILS.md` § "Named destinations".
// DEFAULT-OK: every source keeps its own name, which is what a transfer that
// named nothing asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(from = "Option<String>")]
pub struct TransferLeaves {
    form: Form,
}

// DEFAULT-OK: keeping every name is what a transfer that named nothing asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Form {
    /// Every source keeps its own name.
    #[default]
    Own,
    /// The ONE source lands as this leaf.
    Single(String),
    /// Each keyed source lands as its leaf; any other keeps its own name. Keyed
    /// by the source path exactly as the engine walks it ([`TransferLeaves::rebased`]).
    PerSource(HashMap<PathBuf, String>),
}

impl TransferLeaves {
    /// The one source lands as `leaf`.
    pub fn single(leaf: impl Into<String>) -> Self {
        Self {
            form: Form::Single(leaf.into()),
        }
    }

    /// Each source in `names` lands as its leaf, and every other source keeps
    /// its own name. Keys are source paths exactly as the transfer is given
    /// them; a key that names no source refuses at the start, typed. No names
    /// at all is the same as [`Self::default`].
    pub fn per_source(names: impl IntoIterator<Item = (PathBuf, String)>) -> Self {
        let names: HashMap<PathBuf, String> = names.into_iter().collect();
        if names.is_empty() {
            return Self::default();
        }
        Self {
            form: Form::PerSource(names),
        }
    }

    /// The single-source leaf, for the validation that refuses it on a batch.
    pub(crate) fn single_leaf(&self) -> Option<&str> {
        match &self.form {
            Form::Single(leaf) => Some(leaf),
            Form::Own | Form::PerSource(_) => None,
        }
    }

    /// The per-source names, for the validation that checks them; `None` for
    /// the other two forms.
    pub(crate) fn per_source_names(&self) -> Option<&HashMap<PathBuf, String>> {
        match &self.form {
            Form::PerSource(names) => Some(names),
            Form::Own | Form::Single(_) => None,
        }
    }

    /// Whether this names its sources one by one, which only some routes can
    /// honor (`routing.rs`).
    pub(crate) fn is_per_source(&self) -> bool {
        matches!(self.form, Form::PerSource(_))
    }

    /// The same leaves keyed the way the engine will walk the sources: `to`
    /// maps each source path, exactly as it maps the sources themselves. The
    /// both-local facades re-root sources onto their volume (`src_root.join(p)`)
    /// and call this in the same step, so a key can't drift from its source.
    #[must_use]
    pub(crate) fn rebased(self, to: impl Fn(&Path) -> PathBuf) -> Self {
        match self.form {
            Form::PerSource(names) => Self {
                form: Form::PerSource(names.into_iter().map(|(source, leaf)| (to(&source), leaf)).collect()),
            },
            form @ (Form::Own | Form::Single(_)) => Self { form },
        }
    }

    /// The name `source` lands as when its caller chose one, as the app text it
    /// was sent; `None` when it keeps its own. The volume engine reads it by the
    /// app path's rules (`transfer/volume/listed_name.rs::top_level`), so a POSIX
    /// server's `a\b.txt` stays one name on Windows.
    pub(crate) fn chosen_name(&self, source: &Path) -> Option<&str> {
        match &self.form {
            Form::Own => None,
            Form::Single(leaf) => Some(leaf),
            Form::PerSource(names) => names.get(source).map(String::as_str),
        }
    }

    /// [`Self::chosen_name`] for the LOCAL engines, which join it onto a native
    /// path the OS spells.
    pub(crate) fn chosen_for(&self, source: &Path) -> Option<&OsStr> {
        self.chosen_name(source).map(|leaf| leaf.as_ref())
    }

    /// The name `source` lands as: the chosen one, else its own. `None` only
    /// for a path with no final component, which no route transfers by name.
    pub(crate) fn leaf_of<'a>(&'a self, source: &'a Path) -> Option<&'a OsStr> {
        let own = source.file_name()?;
        Some(self.chosen_for(source).unwrap_or(own))
    }
}

// The bindings describe exactly what serde reads and writes: one optional leaf.
impl specta::Type for TransferLeaves {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <Option<String> as specta::Type>::definition(types)
    }
}

impl From<Option<String>> for TransferLeaves {
    fn from(single: Option<String>) -> Self {
        Self {
            form: single.map_or(Form::Own, Form::Single),
        }
    }
}

impl Serialize for TransferLeaves {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.form {
            Form::Own => serializer.serialize_none(),
            Form::Single(leaf) => serializer.serialize_some(leaf),
            // One leaf can't say it, and dropping the names would land every
            // file under its old name with nobody told.
            Form::PerSource(_) => Err(serde::ser::Error::custom(
                "per-source transfer names never go on the wire",
            )),
        }
    }
}

#[cfg(test)]
#[path = "transfer_leaves_wire_tests.rs"]
mod wire_tests;
