//! Which scratch a listing leaves out of the pane, under the two visibility
//! settings: Cmdr's own by ownership, another app's safe-save by name.

use std::path::Path;
use std::sync::Arc;

use super::*;

/// Stands in for a running operation: the temps it mints stay hidden until
/// the returned `Arc` is dropped, which is what settling does for real.
fn running_operation() -> Arc<()> {
    Arc::new(())
}

/// Whether `name` stays out of the pane under `(show_staging_temps,
/// show_safe_save_files)`.
fn hidden(name: &str, (show_staging, show_safe_save): (bool, bool)) -> bool {
    hidden_with_settings(name, show_staging, show_safe_save)
}

fn name_of(temp: &StagingTemp) -> String {
    temp.path()
        .file_name()
        .expect("a minted temp always has a filename")
        .to_string_lossy()
        .into_owned()
}

/// The whole point of the guard: while it lives the file is hidden, and the
/// moment it drops it isn't.
#[test]
fn a_live_guard_hides_its_temp_and_dropping_it_stops() {
    let settings = (false, true);
    let op = running_operation();
    let temp = StagingTemp::mint(Path::new("/dir/photo.jpg"), Some(Arc::downgrade(&op)));
    let name = name_of(&temp);

    assert!(hidden(&name, settings));
    drop(temp);
    assert!(!hidden(&name, settings));
}

/// David's first edge case: an operation that wedges and leaves its temps on
/// disk must not leave them hidden too. Its guard may never drop — the driver
/// abandons a task that won't wind down, and the task keeps holding it — so
/// the operation's liveness is what has to answer.
#[test]
fn a_leftover_from_a_dead_operation_is_visible_even_if_its_guard_leaked() {
    let settings = (false, true);
    let op = running_operation();
    let temp = StagingTemp::mint(Path::new("/dir/photo.jpg"), Some(Arc::downgrade(&op)));
    let name = name_of(&temp);
    assert!(hidden(&name, settings), "hidden while the operation runs");

    drop(op);

    // The guard deliberately stays alive, standing in for the abandoned task.
    assert!(
        !hidden(&name, settings),
        "a leftover nobody is running for must be visible"
    );
    drop(temp);
}

/// A cross-filesystem move's staging folder is Cmdr's scratch too, and it
/// sits in the destination the user is watching for the whole move. It hides
/// by the same ownership rule, so one left behind by an interrupted move
/// shows up rather than lurking.
#[test]
fn a_moves_staging_directory_hides_while_the_move_owns_it() {
    let settings = (false, true);
    let op = running_operation();
    let name = format!(".cmdr-staging-{}", uuid::Uuid::new_v4());
    let guard = StagingTemp::adopt(Path::new("/dest").join(&name), Some(Arc::downgrade(&op)));

    assert!(hidden(&name, settings), "hidden while the move runs");
    drop(guard);
    assert!(
        !hidden(&name, settings),
        "a staging folder nobody owns is a leftover the user must see"
    );
}

/// Ordinary files are never touched.
#[test]
fn an_ordinary_file_is_never_hidden() {
    let settings = (false, true);
    let _temp = StagingTemp::mint(Path::new("/dir/photo.jpg"), None);
    assert!(!hidden("photo.jpg", settings));
    assert!(!hidden(".gitignore", settings));
}

/// Safe-save scratch shows by default: it's another app's business, and
/// hiding someone else's files by name is a bigger claim than hiding our own.
#[test]
fn safe_save_scratch_is_shown_by_default() {
    let settings = (false, true);
    assert!(!hidden("notes.txt.sb-1e64c894-vFWIzN", settings));
}

/// Turning the setting off hides it purely by name: no operation owns another
/// app's scratch, so there's nothing else to go on.
#[test]
fn safe_save_scratch_hides_by_name_alone() {
    let settings = (false, false);
    assert!(hidden("notes.txt.sb-1e64c894-vFWIzN", settings));
    assert!(
        hidden("notes.txt.sb-1e64c894", settings),
        "the temp DIRECTORY hides too, not just the file inside it"
    );
    assert!(!hidden("notes.txt", settings));
}

/// The two categories are independent switches, so neither setting can
/// silently change what the other one does.
#[test]
fn the_two_scratch_settings_are_independent() {
    let temp = StagingTemp::mint(Path::new("/dir/photo.jpg"), None);
    let cmdr_name = name_of(&temp);
    let safe_save_name = "notes.txt.sb-1e64c894-vFWIzN";

    {
        let settings = (true, false);
        assert!(!hidden(&cmdr_name, settings), "Cmdr scratch shown");
        assert!(hidden(safe_save_name, settings), "safe-save hidden");
    }
    {
        let settings = (false, true);
        assert!(hidden(&cmdr_name, settings), "Cmdr scratch hidden");
        assert!(!hidden(safe_save_name, settings), "safe-save shown");
    }
}

/// The Settings > Advanced escape hatch overrides the hiding, not the
/// registry: what's in flight is still in flight, it's just shown.
#[test]
fn the_setting_shows_in_flight_temps() {
    let temp = StagingTemp::mint(Path::new("/dir/photo.jpg"), None);
    let name = name_of(&temp);

    let settings = (true, true);
    assert!(!hidden(&name, settings));
    assert!(is_staging_temp_in_flight(&name), "still in flight, just not hidden");
}
