//! Counts a transfer's names that Windows can't use as they are, while its
//! scan walks (`cmdr_fs::validation::windows_portability`), so the dialog can
//! warn before any byte moves. Destination-blind on purpose: the destination
//! can change while the dialog is open, and its `NameRules` decide which
//! classes it warns about.
//!
//! ❗ A counter and a few examples per class, ❌ never a list of names: a
//! million-file tree costs three counters and nine short strings. One string
//! scan per name, no allocation unless the name is one of the first examples.

use std::path::Path;
use std::sync::{Arc, Mutex};

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::validation::{PortabilityWarning, windows_portability};
use cmdr_fs::volume::{CopyScanResult, ScanRecorder};

use super::super::types::NamePortabilityTally;

/// How many names each class keeps as examples.
pub(crate) const NAME_TALLY_EXAMPLES: usize = 3;

/// The running tally, and the recorder a volume walk feeds: it forwards every
/// entry to `inner` (the transfer tree's recorder) and counts its name.
pub(crate) struct NameTally {
    tally: Mutex<NamePortabilityTally>,
    inner: Option<Arc<dyn ScanRecorder>>,
}

impl NameTally {
    /// A tally that forwards each recorded entry to `inner` too.
    pub(crate) fn forwarding_to(inner: Option<Arc<dyn ScanRecorder>>) -> Self {
        Self {
            tally: Mutex::new(NamePortabilityTally::default()),
            inner,
        }
    }

    /// Counts the last component of `path`.
    pub(crate) fn note_path(&self, path: &Path) {
        let Some(name) = path.file_name() else { return };
        let name = name.to_string_lossy();
        let Some(warning) = windows_portability(&name) else {
            return;
        };
        // A counter: a poisoned one still counts.
        let mut tally = self.tally.lock_ignore_poison();
        let class = match warning {
            PortabilityWarning::WindowsCharacter { .. } => &mut tally.windows_characters,
            PortabilityWarning::ReservedName { .. } => &mut tally.reserved_names,
            PortabilityWarning::EndsWithSpaceOrPeriod => &mut tally.ends_with_space_or_period,
        };
        class.count += 1;
        if class.examples.len() < NAME_TALLY_EXAMPLES {
            class.examples.push(name.into_owned());
        }
    }

    /// What the walk counted.
    pub(crate) fn finish(&self) -> NamePortabilityTally {
        self.tally.lock_ignore_poison().clone()
    }
}

impl ScanRecorder for NameTally {
    fn file(&self, path: &Path, size: u64) {
        self.note_path(path);
        if let Some(inner) = &self.inner {
            inner.file(path, size);
        }
    }

    fn dir(&self, path: &Path) {
        self.note_path(path);
        if let Some(inner) = &self.inner {
            inner.dir(path);
        }
    }

    /// A subtree counted without a walk: only its own name is seen. Its
    /// children's names stay uncounted, which undercounts, never invents.
    fn subtree(&self, path: &Path, scan: &CopyScanResult) {
        self.note_path(path);
        if let Some(inner) = &self.inner {
            inner.subtree(path, scan);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_class_counts_every_name_and_keeps_a_few() {
        let tally = NameTally::forwarding_to(None);
        for i in 0..10 {
            tally.note_path(Path::new(&format!("/src/{i}:00.txt")));
        }
        tally.note_path(Path::new("/src/con.txt"));
        tally.note_path(Path::new("/src/notes."));
        tally.note_path(Path::new("/src/fine.txt"));
        let tally = tally.finish();
        assert_eq!(tally.windows_characters.count, 10);
        assert_eq!(tally.windows_characters.examples, ["0:00.txt", "1:00.txt", "2:00.txt"]);
        assert_eq!(tally.reserved_names.count, 1);
        assert_eq!(tally.reserved_names.examples, ["con.txt"]);
        assert_eq!(tally.ends_with_space_or_period.count, 1);
    }

    /// A million names cost a counter each: the examples never grow past
    /// their cap, however many names the walk meets.
    #[test]
    fn a_huge_tree_keeps_only_the_examples() {
        let tally = NameTally::forwarding_to(None);
        for i in 0..100_000 {
            tally.note_path(Path::new(&format!("/src/{i}?")));
        }
        let tally = tally.finish();
        assert_eq!(tally.windows_characters.count, 100_000);
        assert_eq!(tally.windows_characters.examples.len(), NAME_TALLY_EXAMPLES);
    }
}
