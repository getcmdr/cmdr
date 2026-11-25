//! The index an eject flight test drives: nothing here touches a real one. Shared by the
//! macOS disk flight's tests, `real_image.rs`, whose resume pin runs the same gate against a
//! real disk image, and the Windows drive flight's tests.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use cmdr_index::{IndexVolumeKind, RemovableStop};

use crate::file_system::volume::drive_release::{IndexDoor, Ticket};
use crate::ignore_poison::IgnorePoison;

/// An index whose kind, intent, and starts a test writes and reads.
// DEFAULT-OK: an index that knows nothing yet, which is what every test starts from.
#[derive(Default)]
pub(in crate::file_system::volume::eject) struct FakeIndex {
    indexing: Mutex<HashSet<String>>,
    intent: Mutex<HashSet<String>>,
    started: Mutex<Vec<String>>,
}

impl FakeIndex {
    /// The volume is indexing, and its intent says it should be.
    pub(in crate::file_system::volume::eject) fn indexes(&self, volume_id: &str) {
        self.indexing.lock_ignore_poison().insert(volume_id.to_string());
        self.intent.lock_ignore_poison().insert(volume_id.to_string());
    }

    /// Every volume a resume started again, sorted.
    pub(in crate::file_system::volume::eject) fn started(&self) -> Vec<String> {
        let mut started = self.started.lock_ignore_poison().clone();
        started.sort();
        started
    }

    /// The stop a release runs: it lets go of whatever this index has.
    pub(in crate::file_system::volume::eject) fn stop(
        self: &Arc<Self>,
    ) -> impl Fn(&str) -> RemovableStop + Send + Sync + 'static {
        let index = Arc::clone(self);
        move |volume_id: &str| {
            if index.indexing.lock_ignore_poison().remove(volume_id) {
                RemovableStop::Released
            } else {
                RemovableStop::NothingToStop
            }
        }
    }
}

impl IndexDoor for FakeIndex {
    fn volume_kind(&self, volume_id: &str) -> Option<IndexVolumeKind> {
        self.indexing
            .lock_ignore_poison()
            .contains(volume_id)
            .then_some(IndexVolumeKind::LocalExternal)
    }

    fn drives_to_resume(&self) -> Vec<String> {
        self.intent.lock_ignore_poison().iter().cloned().collect()
    }

    fn start_resumed(&self, volume_id: String, ticket: Ticket) {
        self.started.lock_ignore_poison().push(volume_id);
        drop(ticket);
    }

    fn is_ejecting(&self, _volume_id: &str) -> bool {
        false
    }

    fn is_listed(&self, _volume_id: &str) -> bool {
        true
    }
}
