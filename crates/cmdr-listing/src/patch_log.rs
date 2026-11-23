//! Which names an incremental patch touched while a full re-read of the
//! listing was in flight, so the re-read's landing can stat them again
//! (`DETAILS.md` § "Read generations"). Without it, a re-read that started
//! before a patch and lands after it silently undoes the patch (#1342).
//!
//! Pure data, a leaf: the listing holds one, and `settle.rs` acts on what a
//! landing owes.

use std::collections::HashMap;

use crate::read_generation::ReadGeneration;

/// How many names one listing tracks. Past it, a landing re-reads the whole
/// directory once more instead of statting: a burst that big is cheaper as one
/// read, and correctness never rests on the bound.
pub(crate) const MAX_TRACKED_PATCHES: usize = 1_024;

/// The patched names, each stamped with the last read generation handed out
/// when its patch landed. A read with generation `G` started before every patch
/// stamped `G` or later, so those are the ones its landing may undo.
#[derive(Debug, Default)]
pub(crate) struct PatchLog {
    names: HashMap<String, ReadGeneration>,
    /// The newest stamp a full log turned away.
    dropped: Option<ReadGeneration>,
}

/// What a landing read owes the patches that landed while it was reading.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Recheck {
    /// No patch landed while it read: the normal case, which costs nothing.
    Nothing,
    /// Stat these names again and patch them with what's on disk now.
    Names(Vec<String>),
    /// More patched than the log holds: read the whole directory once more.
    Reread,
}

impl PatchLog {
    /// How many names it tracks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// Notes a patch to `name`, stamped `stamp`.
    pub(crate) fn note(&mut self, name: &str, stamp: ReadGeneration) {
        if let Some(stamped) = self.names.get_mut(name) {
            *stamped = (*stamped).max(stamp);
            return;
        }
        if self.names.len() >= MAX_TRACKED_PATCHES {
            self.dropped = self.dropped.max(Some(stamp));
            return;
        }
        self.names.insert(name.to_owned(), stamp);
    }

    /// What a read with `generation` owes, as it lands. Forgets every name only
    /// that read or an older one could undo; a newer read still in flight keeps
    /// the ones stamped after it started.
    pub(crate) fn landing(&mut self, generation: ReadGeneration) -> Recheck {
        let overflowed = self.dropped.take().is_some_and(|dropped| dropped >= generation);
        let owed: Vec<String> = if overflowed {
            Vec::new()
        } else {
            self.names
                .iter()
                .filter(|(_, stamp)| **stamp >= generation)
                .map(|(name, _)| name.clone())
                .collect()
        };
        self.names.retain(|_, stamp| *stamp > generation);
        if overflowed {
            Recheck::Reread
        } else if owed.is_empty() {
            Recheck::Nothing
        } else {
            Recheck::Names(owed)
        }
    }
}
