//! A recording observer and a fixed overlay, the fakes for the two list seams.

use cmdr_fs::app_path::{AppName, AppPath, PathArithmetic};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::Volume;

use crate::Inner;
use crate::Listings;
use crate::host::{ListingObserver, ListingOverlay, OpenedListing};
use crate::listing_id::ListingId;

/// One open or close as the observer heard it, with whether the engine held
/// the listing at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// An open of listing `id`.
    Opened {
        /// The listing.
        id: String,
        /// Whether the engine's cache held it right then.
        cached: bool,
    },
    /// A close of listing `id`.
    Closed {
        /// The listing.
        id: String,
        /// Whether the engine's cache held it right then.
        cached: bool,
    },
}

/// Records every open and close, and what the engine's cache held when it was
/// told. [`attach`](Self::attach) it to the engine it observes.
pub struct RecordingObserver {
    /// Weak: the engine holds this observer, so a strong handle back would
    /// keep the engine alive forever.
    engine: OnceLock<Weak<Inner>>,
    heard: Mutex<Vec<Heard>>,
}

impl RecordingObserver {
    /// An observer that has heard nothing yet, attached to no engine.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            engine: OnceLock::new(),
            heard: Mutex::new(Vec::new()),
        })
    }

    /// Reads cache membership from `engine` from now on.
    pub fn attach(&self, engine: &Listings) {
        let _ = self.engine.set(Arc::downgrade(&engine.inner));
    }

    /// Everything heard so far, in order.
    pub fn heard(&self) -> Vec<Heard> {
        self.heard.lock_ignore_poison().clone()
    }

    fn cached(&self, id: &ListingId) -> bool {
        self.engine
            .get()
            .and_then(Weak::upgrade)
            .is_some_and(|engine| engine.contains(id.as_str()))
    }
}

impl ListingObserver for RecordingObserver {
    fn id(&self) -> &'static str {
        "recording"
    }

    fn listing_opened(&self, opened: &OpenedListing<'_>) {
        let cached = self.cached(opened.id);
        self.heard.lock_ignore_poison().push(Heard::Opened {
            id: opened.id.to_string(),
            cached,
        });
    }

    fn listing_closed(&self, id: &ListingId) {
        let cached = self.cached(id);
        self.heard.lock_ignore_poison().push(Heard::Closed {
            id: id.to_string(),
            cached,
        });
    }
}

/// Folds one fixed row into every directory on a volume with a given name.
pub struct StaticOverlay {
    volume_name: String,
    row: String,
}

impl StaticOverlay {
    /// Adds a folder row named `row` to every listing on a volume named
    /// `volume_name`.
    pub fn new(volume_name: &str, row: &str) -> Arc<Self> {
        Arc::new(Self {
            volume_name: volume_name.to_string(),
            row: row.to_string(),
        })
    }
}

impl ListingOverlay for StaticOverlay {
    fn id(&self) -> &'static str {
        "static"
    }

    fn applies_to(&self, volume: &dyn Volume, _path: &AppPath, _entries: &[FileEntry]) -> bool {
        volume.name() == self.volume_name
    }

    fn extra_entries(&self, _volume_id: &str, _volume: &dyn Volume, path: &AppPath) -> Vec<FileEntry> {
        vec![FileEntry::new(
            self.row.clone(),
            path.join(AppName::from_app_spelling(&self.row).expect("a plain row name"))
                .expect("a joinable folder")
                .into_string(),
            true,
            false,
        )]
    }
}
