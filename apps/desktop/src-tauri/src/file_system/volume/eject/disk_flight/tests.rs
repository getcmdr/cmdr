use super::test_support::FakeIndex;
use super::*;
use crate::file_system::volume::drive_release::IndexDoor;
use std::time::Instant;

/// Well past `RESUME_SETTLE`, so a resume batch's quiet period is over.
const SETTLED: std::time::Duration = std::time::Duration::from_secs(30);

fn mounted(path: &str, whole_unit: u32) -> MountedVolume {
    MountedVolume {
        bsd_name: format!("disk{whole_unit}s1"),
        whole_unit,
        volume_uuid: None,
        path: crate::volumes::os_mount_path(path).to_path_buf(),
    }
}

#[test]
fn a_disks_siblings_are_the_registered_volumes_at_its_mounts_with_the_asked_one_first() {
    let registry: HashMap<PathBuf, String> = [
        (PathBuf::from("/Volumes/A"), "vol-a".to_string()),
        (PathBuf::from("/Volumes/B"), "vol-b".to_string()),
    ]
    .into();
    let disk = [
        mounted("/Volumes/B", 7),
        mounted("/Volumes/A", 7),
        // A volume of the same disk that Cmdr doesn't have registered.
        mounted("/Volumes/Unknown", 7),
    ];

    let siblings = capture("vol-a", Path::new("/Volumes/A"), &disk, |path| {
        registry.get(path).cloned()
    });

    assert_eq!(
        siblings,
        [
            Sibling {
                volume_id: "vol-a".to_string(),
                path: PathBuf::from("/Volumes/A"),
            },
            Sibling {
                volume_id: "vol-b".to_string(),
                path: PathBuf::from("/Volumes/B"),
            },
        ],
        "the asked volume leads, its registered sibling follows, and an unregistered mount names nobody"
    );
}

#[test]
fn a_volume_reachable_through_a_spare_mount_isnt_captured_twice() {
    // `find_by_root` matches ANY known root, so the active-root check is what keeps
    // one volume from being stopped and resumed as two.
    let disk = [mounted("/Volumes/A", 7), mounted("/Volumes/A-1", 7)];
    let siblings = capture("vol-a", Path::new("/Volumes/A"), &disk, |_| Some("vol-a".to_string()));
    assert_eq!(siblings.len(), 1, "got {siblings:?}");
}

// ── What the teardown aims at, and what counts as done ───────────

fn captured() -> Vec<PathBuf> {
    [PathBuf::from("/Volumes/A"), PathBuf::from("/Volumes/B")].to_vec()
}

#[test]
fn a_disk_whose_captured_mounts_are_all_gone_is_done_only_if_nothing_else_is_mounted_on_it() {
    // `diskutil eject` of a two-partition disk unmounts what it can and exits
    // nonzero on the held one. Asking only about the volume the person clicked
    // would call that done and leave the disk powered on.
    assert!(
        disk_is_still_mounted(&captured(), |path| path == Path::new("/Volumes/B"), || false),
        "a sibling that stayed mounted means the disk is still up"
    );
    assert!(
        disk_is_still_mounted(&captured(), |_| false, || true),
        "and so does a volume the capture never named"
    );
    assert!(
        !disk_is_still_mounted(&captured(), |_| false, || false),
        "every captured mount gone and nothing else on the disk is the only 'done'"
    );
}

#[test]
fn a_disk_nobody_could_read_counts_as_still_mounted_and_an_empty_one_doesnt() {
    // ❗ Three answers, not two. DiskArbitration going silent is NOT an empty disk:
    // collapsing them would answer `Ok` for an eject that left the drive powered on
    // with a volume Cmdr never registered still mounted on it.
    assert!(
        reads_as_mounted(DiskMounts::Unreadable),
        "nobody could say, so the disk is still up"
    );
    assert!(reads_as_mounted(DiskMounts::Read(vec![mounted("/Volumes/A", 7)])));
    assert!(
        !reads_as_mounted(DiskMounts::Read(Vec::new())),
        "a disk genuinely read as empty is the only one that's gone"
    );
}

#[test]
fn a_retry_aims_at_a_mount_thats_still_there_not_at_the_one_that_already_went() {
    // Aimed at `/Volumes/A` after a partial unmount, `diskutil` answers "no such
    // disk" and the retry proves nothing.
    assert_eq!(
        aim_at(&captured(), "/Volumes/A", |path| path == Path::new("/Volumes/B")),
        "/Volumes/B"
    );
    assert_eq!(
        aim_at(&captured(), "/Volumes/A", |_| true),
        "/Volumes/A",
        "with everything still mounted it aims at the disk's own volume, which is captured first"
    );
    assert_eq!(
        aim_at(&captured(), "/Volumes/A", |_| false),
        "/Volumes/A",
        "with nothing left it falls back, and `settle` reads the tool's refusal as already gone"
    );
}

#[test]
fn a_write_op_on_any_volume_of_the_disk_refuses_the_eject() {
    // The teardown takes the whole disk down, so a transfer on a sibling would be
    // truncated just as surely as one on the volume clicked.
    let ids = ["vol-a".to_string(), "vol-b".to_string()];
    assert_eq!(busy_sibling(&ids, &["vol-b".to_string()]), Some("vol-b"));
    assert_eq!(busy_sibling(&ids, &["vol-elsewhere".to_string()]), None);
}

#[test]
fn the_mounts_a_teardown_has_to_see_gone_cover_the_captured_roots_and_a_fresh_read() {
    let siblings = [Sibling {
        volume_id: "vol-a".to_string(),
        path: PathBuf::from("/Volumes/A"),
    }];
    let paths = captured_paths(&[mounted("/Volumes/A", 7), mounted("/Volumes/Unknown", 7)], &siblings);
    assert_eq!(
        paths,
        [PathBuf::from("/Volumes/A"), PathBuf::from("/Volumes/Unknown")],
        "a volume Cmdr never registered still keeps the disk up"
    );
}

// ── What a flight owes back ───────────────────────────────────────

#[test]
fn only_a_volume_that_was_indexing_is_owed_its_index_back() {
    let index = Arc::new(FakeIndex::default());
    index.indexes("vol-a");
    let gate = DriveRelease::with_fake_clock(Arc::clone(&index) as Arc<dyn IndexDoor>);

    let release = gate.release(
        &["vol-a".to_string(), "vol-idle".to_string()],
        Instant::now(),
        index.stop(),
        |_| {},
    );

    assert_eq!(
        owed_ids(&release),
        ["vol-a"],
        "a volume with no index had nothing taken from it"
    );
}

#[test]
fn a_refusal_after_the_flights_own_asks_moved_the_epochs_still_hands_the_index_back() {
    // `diskutil eject` triggers an unmount approval per volume, and each ask's own
    // release moves the epoch. Reading it at the pre-stop would fail every check.
    let index = Arc::new(FakeIndex::default());
    index.indexes("vol-a");
    let gate = DriveRelease::with_fake_clock(Arc::clone(&index) as Arc<dyn IndexDoor>);
    let owner = Arc::new(FlightResume {
        flight_id: u64::MAX,
        paths: [("vol-a".to_string(), PathBuf::from("/"))].into(),
    });

    let release = gate.release(&["vol-a".to_string()], Instant::now(), index.stop(), |_| {});
    let stale = epoch_of(&release, "vol-a");
    // The approver's ask during the teardown, which the eject then refused.
    gate.release(&["vol-a".to_string()], Instant::now(), index.stop(), |_| {});
    assert_ne!(gate.epoch("vol-a"), stale, "the ask moved the epoch");

    let batch = gate.resume(
        vec![ResumeCandidate {
            volume_id: "vol-a".to_string(),
            epoch: gate.epoch("vol-a"),
        }],
        Arc::clone(&owner) as Arc<dyn ResumeOwner>,
    );
    gate.advance(SETTLED);
    batch.wait();

    assert_eq!(index.started(), ["vol-a"], "the index came back");

    // And the same hand-back against the epoch the pre-stop saw resumes nothing.
    index.indexes("vol-a");
    gate.release(&["vol-a".to_string()], Instant::now(), index.stop(), |_| {});
    let batch = gate.resume(
        vec![ResumeCandidate {
            volume_id: "vol-a".to_string(),
            epoch: stale,
        }],
        owner as Arc<dyn ResumeOwner>,
    );
    gate.advance(SETTLED);
    batch.wait();
    assert_eq!(index.started(), ["vol-a"], "the stale epoch resumed nothing");
}
