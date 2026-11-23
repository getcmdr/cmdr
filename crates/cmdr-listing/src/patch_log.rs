//! Which names an incremental patch touched while a full re-read of the
//! listing was in flight, and what each patch saw, so the re-read's landing can
//! check them again (`DETAILS.md` § "Read generations"). Without it, a re-read
//! that started before a patch and lands after it silently undoes the patch
//! (#1342).
//!
//! Pure data, a leaf: the listing holds one, and `settle.rs` acts on what a
//! landing owes.

use std::collections::{HashMap, HashSet};

use cmdr_fs::entry::FileEntry;

use crate::read_generation::ReadGeneration;

/// How many names one listing tracks, and so the most one landing re-checks
/// one by one. Past it, the landing re-reads the folder once more: on a network
/// volume each check is a round trip, and a burst that big is cheaper as one
/// read. Correctness never rests on the bound.
pub(crate) const MAX_TRACKED_PATCHES: usize = 32;

/// What a patch saw for one name.
#[derive(Clone, Debug)]
pub(crate) enum Observed {
    /// The item is there, as this entry. Its `path` is what a check stats.
    Present(Box<FileEntry>),
    /// The item is gone. `path` is the row's own path when the listing held
    /// it, or the watcher's when that's plain UTF-8; `None` when neither, and
    /// then a check can only trust this.
    Gone { path: Option<String> },
}

impl Observed {
    /// The path a check stats, if there's one to stat.
    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Self::Present(entry) => Some(&entry.path),
            Self::Gone { path } => path.as_deref(),
        }
    }
}

#[derive(Debug)]
struct Noted {
    stamp: ReadGeneration,
    observed: Observed,
}

/// The patched names, each stamped with the last read generation handed out
/// when its patch landed. A read with generation `G` started before every patch
/// stamped `G` or later, so those are the ones its landing may undo.
#[derive(Debug, Default)]
pub(crate) struct PatchLog {
    names: HashMap<String, Noted>,
    /// The newest stamp a patch couldn't be noted under (the log was full, or
    /// its name wasn't plain UTF-8): a landing at or before it re-reads.
    unnoted: Option<ReadGeneration>,
}

/// One name a landing owes a check, and what its last patch saw.
#[derive(Debug)]
pub(crate) struct Owed {
    pub(crate) name: String,
    pub(crate) observed: Observed,
}

/// What a landing read owes the patches that landed while it was reading.
#[derive(Debug)]
pub(crate) enum Recheck {
    /// No patch landed while it read: the normal case, which costs nothing.
    Nothing,
    /// Check these names again and patch them with what's on disk now.
    Names(Vec<Owed>),
    /// Too many to check one by one, a name it can't check, or two spellings of
    /// one name: read the whole folder once more.
    Reread,
}

impl Recheck {
    /// Whether the landing owes nothing.
    pub(crate) fn is_nothing(&self) -> bool {
        matches!(self, Self::Nothing)
    }
}

impl PatchLog {
    /// How many names it tracks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// Forgets everything: no read is in flight any more, so nothing is owed.
    pub(crate) fn clear(&mut self) {
        self.names.clear();
        self.unnoted = None;
    }

    /// Notes a patch to `name` that saw `observed`, stamped `stamp`. A later
    /// patch to the same name replaces an earlier one's observation.
    pub(crate) fn note(&mut self, name: &str, stamp: ReadGeneration, observed: Observed) {
        if let Some(noted) = self.names.get_mut(name) {
            noted.stamp = noted.stamp.max(stamp);
            noted.observed = observed;
            return;
        }
        if self.names.len() >= MAX_TRACKED_PATCHES {
            self.note_unnamed(stamp);
            return;
        }
        self.names.insert(name.to_owned(), Noted { stamp, observed });
    }

    /// Notes a patch whose name can't be checked on its own (not plain UTF-8),
    /// so a landing it might undo re-reads instead.
    pub(crate) fn note_unnamed(&mut self, stamp: ReadGeneration) {
        self.unnoted = self.unnoted.max(Some(stamp));
    }

    /// Whether a patch to `name` landed at or after `since`: a check that
    /// started at `since` saw an older disk than that patch did, so it must
    /// not write over it.
    pub(crate) fn patched_since(&self, name: &str, since: ReadGeneration) -> bool {
        self.names.get(name).is_some_and(|noted| noted.stamp >= since)
    }

    /// What a read with `generation` owes, as it lands. Forgets every name only
    /// that read or an older one could undo; a newer read still in flight keeps
    /// the ones stamped after it started.
    pub(crate) fn landing(&mut self, generation: ReadGeneration) -> Recheck {
        let unnoted = self.unnoted.take().is_some_and(|stamp| stamp >= generation);
        let owed: Vec<Owed> = self
            .names
            .iter()
            .filter(|(_, noted)| noted.stamp >= generation)
            .map(|(name, noted)| Owed {
                name: name.clone(),
                observed: noted.observed.clone(),
            })
            .collect();
        self.names.retain(|_, noted| noted.stamp > generation);
        if unnoted || spells_one_name_twice(&owed) {
            Recheck::Reread
        } else if owed.is_empty() {
            Recheck::Nothing
        } else {
            Recheck::Names(owed)
        }
    }
}

/// Whether two owed names differ only in case: a case-only rename on a
/// case-insensitive volume, where a stat of either spelling finds the item and
/// checking them one by one would leave two rows.
fn spells_one_name_twice(owed: &[Owed]) -> bool {
    let mut folded = HashSet::with_capacity(owed.len());
    owed.iter().any(|owed| !folded.insert(owed.name.to_lowercase()))
}
