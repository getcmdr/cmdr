//! One eject at a time per volume, and one per physical disk. A request for a
//! volume whose eject is still running JOINS it and gets the same answer, with no
//! second teardown: a slow `diskutil` (10.5 s in one user's log) invites repeat
//! clicks, and every caller (the header chip, a dropdown row, the native menu,
//! MCP) lands in [`super::eject`].
//!
//! A disk flight takes its SIBLINGS into the set under its own flight
//! ([`join_or_own_disk`](disk::join_or_own_disk)), so an eject of a partition whose disk is already coming
//! down joins in [`join_or_start`], before anything else runs. A sibling whose own
//! flight had already started instead resolves to the same [`DiskKey`] and awaits
//! the disk flight from inside its own.
//!
//! The volumes with an eject in flight are the EJECTING set, pushed to the
//! frontend as `volumes-ejecting-changed` on every change and bootstrapped through
//! `get_ejecting_volume_ids`, the same shape as the busy set's
//! `volumes-busy-changed` (`crates/cmdr-operations/src/status_cache.rs`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

use futures_util::FutureExt;
use futures_util::future::{BoxFuture, Shared};

use super::{EjectError, EjectOutcome};
use crate::ignore_poison::IgnorePoison;

/// An eject in flight, awaitable by every caller that joined it. A joiner reads
/// the same [`EjectOutcome`] the flight's own caller does: one teardown ran, so
/// there is one answer for what it did.
pub(super) type Flight = Shared<BoxFuture<'static, Result<EjectOutcome, EjectError>>>;

/// One physical disk, by the registry entry ID of its whole media
/// (`IORegistryEntryGetRegistryEntryID`). Unique while the disk is attached, so two
/// volumes of one disk resolve to the same key and one teardown serves both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct DiskKey(pub(super) u64);

#[derive(Clone)]
struct InFlight {
    /// Tells this flight's own landing apart from a newer flight's for the same volume.
    flight_id: u64,
    flight: Flight,
}

/// What has an eject in flight: volumes by ID, and the physical disks a flight owns.
// DEFAULT-OK: nothing is being ejected before the first request.
#[derive(Default)]
struct InFlightSet {
    volumes: HashMap<String, InFlight>,
    #[cfg_attr(
        all(not(test), target_os = "linux"),
        expect(dead_code, reason = "Linux ejects one volume at a time")
    )]
    disks: HashMap<DiskKey, DiskClaim>,
}

/// The flight holding a physical disk, and how much of it that flight takes down.
struct DiskClaim {
    flight: InFlight,
    /// The one volume this flight takes down, leaving the disk up. `None` for a whole-disk flight.
    alone: Option<String>,
}

static IN_FLIGHT: LazyLock<Mutex<InFlightSet>> = LazyLock::new(Mutex::default);

/// Told whenever an eject lands: the write engine, whose queue holds an operation back while a
/// volume it touches is being ejected (`OperationVolumes::on_eject_ended`).
type EjectEnded = std::sync::Arc<dyn Fn() + Send + Sync>;
static EJECT_ENDED: LazyLock<Mutex<Vec<EjectEnded>>> = LazyLock::new(Mutex::default);

/// Adds `listener` to those told whenever an eject lands.
pub(crate) fn on_eject_ended(listener: Box<dyn Fn() + Send + Sync>) {
    EJECT_ENDED.lock_ignore_poison().push(std::sync::Arc::from(listener));
}
static NEXT_FLIGHT_ID: AtomicU64 = AtomicU64::new(0);

/// Typed `volumes-ejecting-changed` Tauri event. Wraps the ID list in a struct
/// because `tauri_specta::Event` payloads must be named types; the struct name
/// kebab-cases to the wire name.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct VolumesEjectingChanged {
    /// IDs of volumes whose eject is still running (sorted).
    pub volume_ids: Vec<String>,
}

/// App handle for emitting `volumes-ejecting-changed`. Absent in unit tests, where
/// the set is still queryable through [`ejecting_volume_ids`].
static EJECTING_APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Stores the app handle used to broadcast `volumes-ejecting-changed`. Call once at
/// app setup, before the frontend can ask for an eject.
pub fn init_ejecting_volume_emitter(app: &tauri::AppHandle) {
    let _ = EJECTING_APP.set(app.clone());
}

/// The volumes whose eject is still running (sorted). Backs the
/// `get_ejecting_volume_ids` bootstrap and the native menus' disabled Eject item.
pub fn ejecting_volume_ids() -> Vec<String> {
    sorted_ids(&IN_FLIGHT.lock_ignore_poison())
}

/// Whether an eject of `volume_id` is in flight. The drive-release gate reads it
/// under its own lock, so this lock is never held while taking that one.
pub(crate) fn is_ejecting(volume_id: &str) -> bool {
    IN_FLIGHT.lock_ignore_poison().volumes.contains_key(volume_id)
}

/// Joins the eject in flight for `volume_id`, or starts one from `start`.
///
/// Synchronous on purpose: the join-or-start decision is made under the lock
/// before this returns, so no second request can slip in between and start a
/// second teardown. `start` is called only when nothing is in flight, and its
/// future runs on its own task, so it finishes even if every caller stops
/// waiting (an unmount can't be taken back halfway), and a panic inside it
/// answers [`EjectError::Unexpected`] instead of stranding the volume in the set.
pub(super) fn join_or_start<F, Fut>(volume_id: &str, start: F) -> Flight
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<EjectOutcome, EjectError>> + Send + 'static,
{
    let mut in_flight = IN_FLIGHT.lock_ignore_poison();
    if let Some(running) = in_flight.volumes.get(volume_id) {
        log::debug!(target: "eject", "An eject of {volume_id} is already running; joining it");
        return running.flight.clone();
    }

    let flight_id = NEXT_FLIGHT_ID.fetch_add(1, Ordering::Relaxed);
    let landing = Landing {
        volume_id: volume_id.to_string(),
        flight_id,
    };
    let work = start();
    let task = tokio::spawn(async move {
        // Dropped the moment the teardown ends, a panic included, and before any
        // caller sees the answer, so a caller never reads a stale "ejecting".
        let _landing = landing;
        work.await
    });
    let flight = async move {
        task.await.unwrap_or_else(|join_err| {
            Err(EjectError::Unexpected {
                detail: format!("the eject task failed: {join_err}"),
            })
        })
    }
    .boxed()
    .shared();

    in_flight.volumes.insert(
        volume_id.to_string(),
        InFlight {
            flight_id,
            flight: flight.clone(),
        },
    );
    emit_changed(&in_flight);
    flight
}

/// The per-disk half: a flight that owns a whole disk (macOS's disk flight, Windows' drive flight)
/// and the siblings it adopts. ❗ One expectation for the module, since each OS's eject uses a
/// different part of it: macOS takes one volume of a disk alone, Windows holds the asked volume
/// past a timeout, and Linux uses none of it.
pub(super) mod disk {
    #![cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "macOS takes a volume alone, Windows holds one past a timeout, Linux uses none of it"
        )
    )]

    use std::sync::atomic::Ordering;

    use super::{DiskClaim, DiskKey, Flight, IN_FLIGHT, InFlight, InFlightSet, Landing, NEXT_FLIGHT_ID, emit_changed};
    use crate::ignore_poison::IgnorePoison;

    /// Whether an eject OTHER than `flight_id`'s is taking `volume_id` down.
    ///
    /// A disk flight asks this before it hands a sibling's index back: its own adopted
    /// ids read as ejecting until it lands, so a plain [`is_ejecting`](super::is_ejecting) would have the
    /// flight refuse its own resume.
    pub(in crate::file_system::volume::eject) fn is_ejecting_by_another(volume_id: &str, flight_id: u64) -> bool {
        IN_FLIGHT
            .lock_ignore_poison()
            .volumes
            .get(volume_id)
            .is_some_and(|running| running.flight_id != flight_id)
    }

    /// Whether Cmdr's own eject is taking `volume_id` down and leaving the rest of its disk up. The
    /// unmount approver reads it: such an ask is about this one volume, so its siblings keep their
    /// indexes.
    pub(crate) fn is_ejecting_alone(volume_id: &str) -> bool {
        IN_FLIGHT
            .lock_ignore_poison()
            .disks
            .values()
            .any(|claim| claim.alone.as_deref() == Some(volume_id))
    }

    /// What a flight found when it asked to own its physical disk.
    pub(in crate::file_system::volume::eject) enum DiskFlight {
        /// This flight owns the disk: it took every sibling into the ejecting set and
        /// runs the one teardown. Dropping the ownership hands the adopted ids back.
        Owner(DiskOwnership),
        /// Another flight is taking the whole disk down. Await it and answer what it answers, so one
        /// `diskutil eject` serves every volume of the disk.
        Joined(Flight),
        /// Another flight is taking ONE volume of the disk down. Its answer isn't this flight's, and
        /// its landing changes what this flight should do, so await it and decide again. ❗ That wait
        /// is what stops two siblings ejected together from each reading the other as still mounted
        /// and leaving an empty disk powered on.
        DecideAgainAfter(Flight),
    }

    /// A flight's claim on one physical disk, plus the siblings it adopted.
    pub(in crate::file_system::volume::eject) struct DiskOwnership {
        key: DiskKey,
        flight_id: u64,
        /// One landing per adopted sibling. The flight's OWN landing belongs to its task.
        landings: Vec<Landing>,
    }

    impl DiskOwnership {
        /// The owning flight's ID, which tells this flight's hold on a volume apart from
        /// a later flight's ([`is_ejecting_by_another`]).
        pub(in crate::file_system::volume::eject) fn flight_id(&self) -> u64 {
            self.flight_id
        }

        /// The siblings this ownership took into the ejecting set, for the log.
        pub(in crate::file_system::volume::eject) fn adopted(&self) -> Vec<String> {
            self.landings.iter().map(|landing| landing.volume_id.clone()).collect()
        }

        /// Keeps `volume_id` (the one asked about, whose own entry leaves with its flight's task) in
        /// the ejecting set for as long as this ownership lives, past its flight's answer. A later
        /// eject of it, or of any sibling, joins this flight (hearing its answer) instead of starting a
        /// teardown, and no operation starts on it.
        ///
        /// ❗ For a teardown that timed out but may still land (Windows' `CM_Request_Device_EjectW`
        /// can't be cancelled): a second eject starting under it would let go and take back under the
        /// first request, and the first one's take-back would then veto the second.
        pub(in crate::file_system::volume::eject) fn hold_past_landing(&mut self, volume_id: &str) {
            let mut in_flight = IN_FLIGHT.lock_ignore_poison();
            let Some(running) = in_flight.volumes.get(volume_id).cloned() else {
                return;
            };
            // A fresh id, so the flight's own landing no longer matches and leaves the entry alone.
            let flight_id = NEXT_FLIGHT_ID.fetch_add(1, Ordering::Relaxed);
            in_flight.volumes.insert(
                volume_id.to_string(),
                InFlight {
                    flight_id,
                    flight: running.flight,
                },
            );
            self.landings.push(Landing {
                volume_id: volume_id.to_string(),
                flight_id,
            });
        }
    }

    impl Drop for DiskOwnership {
        fn drop(&mut self) {
            let mut in_flight = IN_FLIGHT.lock_ignore_poison();
            if in_flight
                .disks
                .get(&self.key)
                .is_some_and(|owner| owner.flight.flight_id == self.flight_id)
            {
                in_flight.disks.remove(&self.key);
            }
            // The landings drop after this body, each taking the lock again to leave the
            // set and wake the gate.
        }
    }

    /// Claims `key` for the flight running for `volume_id`, taking `sibling_ids` into the
    /// ejecting set under it, or joins the flight that already owns the disk.
    ///
    /// Synchronous on purpose: the decision is made under the lock before this returns,
    /// so two volumes of one disk can't each start a teardown. A sibling that already
    /// has a flight of its own keeps it: that flight resolves to this same key and joins
    /// here in turn.
    pub(in crate::file_system::volume::eject) fn join_or_own_disk(
        key: DiskKey,
        volume_id: &str,
        sibling_ids: &[String],
    ) -> DiskFlight {
        let mut in_flight = IN_FLIGHT.lock_ignore_poison();
        let Some(mine) = in_flight.volumes.get(volume_id).cloned() else {
            // Only a caller that reached the pipeline without a flight of its own, which
            // nothing does today. Owning nothing is still correct: it runs its own teardown.
            log::warn!(
                target: "eject",
                "The eject of {volume_id} isn't in the ejecting set, so its disk flight adopts no siblings"
            );
            return DiskFlight::Owner(DiskOwnership {
                key,
                flight_id: NEXT_FLIGHT_ID.fetch_add(1, Ordering::Relaxed),
                landings: Vec::new(),
            });
        };
        if let Some(answer) = another_flights_claim(&in_flight, key, mine.flight_id, volume_id) {
            return answer;
        }

        in_flight.disks.insert(
            key,
            DiskClaim {
                flight: mine.clone(),
                alone: None,
            },
        );
        let mut landings = Vec::new();
        for sibling_id in sibling_ids {
            if sibling_id == volume_id || in_flight.volumes.contains_key(sibling_id) {
                continue;
            }
            in_flight.volumes.insert(sibling_id.clone(), mine.clone());
            landings.push(Landing {
                volume_id: sibling_id.clone(),
                flight_id: mine.flight_id,
            });
        }
        emit_changed(&in_flight);
        DiskFlight::Owner(DiskOwnership {
            key,
            flight_id: mine.flight_id,
            landings,
        })
    }

    /// What a flight does about another flight's claim on its disk, `None` when there's no such claim.
    fn another_flights_claim(in_flight: &InFlightSet, key: DiskKey, mine: u64, volume_id: &str) -> Option<DiskFlight> {
        let owner = in_flight
            .disks
            .get(&key)
            .filter(|owner| owner.flight.flight_id != mine)?;
        let running = owner.flight.flight.clone();
        if owner.alone.is_some() {
            log::info!(target: "eject", "A sibling of {volume_id} is ejecting; deciding what to do once it lands");
            return Some(DiskFlight::DecideAgainAfter(running));
        }
        log::info!(target: "eject", "The disk under {volume_id} is already coming down; joining that flight");
        Some(DiskFlight::Joined(running))
    }

    /// Claims `key` for the flight running for `volume_id`, which takes that one volume down and leaves
    /// the disk up. It adopts nobody, so a sibling's row stays usable.
    pub(in crate::file_system::volume::eject) fn own_disk_for_one_volume(key: DiskKey, volume_id: &str) -> DiskFlight {
        let mut in_flight = IN_FLIGHT.lock_ignore_poison();
        let Some(mine) = in_flight.volumes.get(volume_id).cloned() else {
            log::warn!(target: "eject", "The eject of {volume_id} isn't in the ejecting set");
            return DiskFlight::Owner(DiskOwnership {
                key,
                flight_id: NEXT_FLIGHT_ID.fetch_add(1, Ordering::Relaxed),
                landings: Vec::new(),
            });
        };
        if let Some(answer) = another_flights_claim(&in_flight, key, mine.flight_id, volume_id) {
            return answer;
        }
        let flight_id = mine.flight_id;
        in_flight.disks.insert(
            key,
            DiskClaim {
                flight: mine,
                alone: Some(volume_id.to_string()),
            },
        );
        DiskFlight::Owner(DiskOwnership {
            key,
            flight_id,
            landings: Vec::new(),
        })
    }
}

/// Takes its flight out of the ejecting set when the teardown ends.
struct Landing {
    volume_id: String,
    flight_id: u64,
}

impl Drop for Landing {
    fn drop(&mut self) {
        let landed = {
            let mut in_flight = IN_FLIGHT.lock_ignore_poison();
            let ours = in_flight
                .volumes
                .get(&self.volume_id)
                .is_some_and(|running| running.flight_id == self.flight_id);
            if ours {
                in_flight.volumes.remove(&self.volume_id);
                emit_changed(&in_flight);
            }
            ours
        };
        // A person's start waiting out this eject reads the set again. Outside the
        // lock: the gate takes its own lock first and reads this set under it.
        if landed {
            crate::file_system::volume::drive_release::notify_ejecting_changed();
            // An operation the eject held back in the queue starts now (and meets the drive
            // gone if it went). Outside the lock too: admission reads this set under its own.
            let listeners = EJECT_ENDED.lock_ignore_poison().clone();
            for listener in listeners {
                listener();
            }
        }
    }
}

fn sorted_ids(in_flight: &InFlightSet) -> Vec<String> {
    let mut ids: Vec<String> = in_flight.volumes.keys().cloned().collect();
    ids.sort();
    ids
}

/// Broadcasts the set. Called with the lock held, so two changes can't reach the
/// frontend out of order.
fn emit_changed(in_flight: &InFlightSet) {
    let Some(app) = EJECTING_APP.get() else {
        return;
    };
    use tauri_specta::Event as _;
    let payload = VolumesEjectingChanged {
        volume_ids: sorted_ids(in_flight),
    };
    if let Err(e) = payload.emit(app) {
        crate::log_error!(target: "eject", "Failed to emit volumes-ejecting-changed: {}", e);
    }
}

#[cfg(test)]
mod tests;
