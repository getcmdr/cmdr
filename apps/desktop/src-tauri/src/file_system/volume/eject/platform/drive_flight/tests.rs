use std::sync::Mutex;

use super::*;
use crate::file_system::volume::drive_release::IndexDoor;
use crate::file_system::volume::eject::fake_index::FakeIndex;
use crate::ignore_poison::IgnorePoison;

/// Well past `RESUME_SETTLE`, so a resume batch's quiet period is over.
const SETTLED: Duration = Duration::from_secs(30);

/// A drive side that records what the flight asked of it, in order, and answers the request
/// with what the test scripted.
struct ScriptedSide {
    steps: Mutex<Vec<&'static str>>,
    answer: Mutex<Option<RequestAnswer>>,
    /// The index the flight stopped, read when the request runs: the stop must come first.
    index: Arc<FakeIndex>,
    volume_id: String,
    stopped_before_request: Mutex<Option<bool>>,
}

impl ScriptedSide {
    fn new(index: &Arc<FakeIndex>, volume_id: &str, answer: RequestAnswer) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(Vec::new()),
            answer: Mutex::new(Some(answer)),
            index: Arc::clone(index),
            volume_id: volume_id.to_string(),
            stopped_before_request: Mutex::new(None),
        })
    }

    fn steps(&self) -> Vec<&'static str> {
        self.steps.lock_ignore_poison().clone()
    }
}

impl DriveSide for ScriptedSide {
    fn let_go(&self) {
        self.steps.lock_ignore_poison().push("let go");
    }

    fn request(&self) -> RequestAnswer {
        self.steps.lock_ignore_poison().push("request");
        *self.stopped_before_request.lock_ignore_poison() = Some(self.index.volume_kind(&self.volume_id).is_none());
        self.answer
            .lock_ignore_poison()
            .take()
            .expect("the flight asks Windows once")
    }

    fn take_back(&self) {
        self.steps.lock_ignore_poison().push("take back");
    }

    fn is_listed(&self) -> bool {
        true
    }
}

fn gate_over(index: &Arc<FakeIndex>) -> &'static DriveRelease {
    Box::leak(Box::new(DriveRelease::with_fake_clock(
        Arc::clone(index) as Arc<dyn IndexDoor>
    )))
}

fn run(gate: &'static DriveRelease, index: &Arc<FakeIndex>, side: &Arc<ScriptedSide>, busy: bool) -> FlightEnd {
    eject_drive(
        gate,
        &side.volume_id,
        Arc::clone(side) as Arc<dyn DriveSide>,
        || busy,
        index.stop(),
        REQUEST_DEADLINE,
    )
}

#[test]
fn an_idle_stick_lets_go_then_ejects_and_keeps_nothing_back() {
    let index = Arc::new(FakeIndex::default());
    index.indexes("vol-stick");
    let gate = gate_over(&index);
    let side = ScriptedSide::new(&index, "vol-stick", RequestAnswer::Ejected);

    let end = run(gate, &index, &side, false);

    assert!(
        matches!(end.answer, Ok(EjectOutcome::DiskEjected)),
        "got {:?}",
        end.answer
    );
    assert_eq!(
        side.steps(),
        ["let go", "request"],
        "an eject that went takes nothing back"
    );
    assert_eq!(
        *side.stopped_before_request.lock_ignore_poison(),
        Some(true),
        "the index let go before Windows was asked"
    );
    assert!(end.handed_back.is_none(), "the drive went, so its index stays stopped");
}

#[test]
fn a_veto_takes_everything_back_and_refuses_honestly() {
    // A file on the stick open in another app: Windows vetoes, and the flight puts back exactly
    // what it let go of, the index included, as a refused "Safely remove" does.
    let index = Arc::new(FakeIndex::default());
    index.indexes("vol-stick");
    let gate = gate_over(&index);
    let side = ScriptedSide::new(
        &index,
        "vol-stick",
        RequestAnswer::Vetoed {
            detail: "veto 5 (PNP_VetoOutstandingOpen) by STORAGE\\Volume\\x".to_string(),
        },
    );

    let end = run(gate, &index, &side, false);

    match &end.answer {
        Err(EjectError::UnmountRefused { holders, detail }) => {
            assert_eq!(
                *holders,
                HolderScan::not_scanned(),
                "nobody named, which is ❌ never 'nobody holds it'"
            );
            assert!(!detail.is_empty(), "the veto rides along for the log");
        }
        other => panic!("expected an honest refusal, got {other:?}"),
    }
    assert_eq!(side.steps(), ["let go", "request", "take back"]);
    let batch = end.handed_back.expect("the index that was running is handed back");
    gate.advance(SETTLED);
    batch.wait();
    assert_eq!(index.started(), ["vol-stick"], "the index came back");
}

#[test]
fn a_refusal_of_a_drive_with_no_index_takes_the_watches_back_and_starts_nothing() {
    let index = Arc::new(FakeIndex::default());
    let gate = gate_over(&index);
    let side = ScriptedSide::new(
        &index,
        "vol-card",
        RequestAnswer::NotPossible {
            detail: "CR_ACCESS_DENIED".to_string(),
        },
    );

    let end = run(gate, &index, &side, false);

    assert!(
        matches!(end.answer, Err(EjectError::Unexpected { .. })),
        "no holder, so ❌ not 'something is in use': got {:?}",
        end.answer
    );
    assert_eq!(side.steps(), ["let go", "request", "take back"]);
    assert!(end.handed_back.is_none(), "nothing was indexing, so nothing is owed");
}

#[test]
fn a_running_operation_refuses_before_anything_lets_go() {
    let index = Arc::new(FakeIndex::default());
    index.indexes("vol-stick");
    let gate = gate_over(&index);
    let side = ScriptedSide::new(&index, "vol-stick", RequestAnswer::Ejected);

    let end = run(gate, &index, &side, true);

    assert!(matches!(end.answer, Err(EjectError::Busy)), "got {:?}", end.answer);
    assert!(side.steps().is_empty(), "no let-go and no request: {:?}", side.steps());
    assert_eq!(
        index.volume_kind("vol-stick"),
        Some(cmdr_index::IndexVolumeKind::LocalExternal),
        "the index never stopped"
    );
}

#[test]
fn an_index_that_wont_let_go_keeps_the_drive_untouched() {
    let index = Arc::new(FakeIndex::default());
    let gate = gate_over(&index);
    let side = ScriptedSide::new(&index, "vol-stick", RequestAnswer::Ejected);

    let end = eject_drive(
        gate,
        "vol-stick",
        Arc::clone(&side) as Arc<dyn DriveSide>,
        || false,
        |_: &str| RemovableStop::StillReleasing,
        REQUEST_DEADLINE,
    );

    assert!(
        matches!(end.answer, Err(EjectError::NotResponding { .. })),
        "got {:?}",
        end.answer
    );
    assert!(
        side.steps().is_empty(),
        "an index that may still hold the drive never meets the request"
    );
}

/// A side whose request answers only when the test lets it.
struct HeldRequest {
    release: Mutex<Option<mpsc::Receiver<RequestAnswer>>>,
    taken_back: mpsc::SyncSender<()>,
}

impl DriveSide for HeldRequest {
    fn let_go(&self) {}

    fn request(&self) -> RequestAnswer {
        let release = self.release.lock_ignore_poison().take().expect("asked once");
        release.recv().expect("the test answers")
    }

    fn take_back(&self) {
        let _ = self.taken_back.send(());
    }

    fn is_listed(&self) -> bool {
        true
    }
}

#[test]
fn a_request_past_its_deadline_answers_at_once_and_takes_back_only_when_it_ends_refused() {
    let index = Arc::new(FakeIndex::default());
    let gate = gate_over(&index);
    let (answer, release) = mpsc::sync_channel(1);
    let (taken_back, took_back) = mpsc::sync_channel(1);
    let side = Arc::new(HeldRequest {
        release: Mutex::new(Some(release)),
        taken_back,
    });

    let end = eject_drive(
        gate,
        "vol-slow",
        Arc::clone(&side) as Arc<dyn DriveSide>,
        || false,
        index.stop(),
        Duration::from_millis(50),
    );

    assert!(
        matches!(end.answer, Err(EjectError::TimedOut)),
        "the eject may still land: got {:?}",
        end.answer
    );
    assert!(
        took_back.try_recv().is_err(),
        "nothing comes back while the request may still land"
    );
    answer
        .send(RequestAnswer::Vetoed {
            detail: "late veto".to_string(),
        })
        .expect("the request is waiting");
    took_back
        .recv_timeout(Duration::from_secs(10))
        .expect("a request that ended refused takes the drive back");
}
