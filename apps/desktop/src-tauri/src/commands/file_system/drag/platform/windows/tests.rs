//! The Windows drag arm, headless: effect negotiation, the data objects a drag carries (built
//! from real temp files, read back as a drop target reads them), and the drop target driven
//! through its COM interface with no window and no mouse. Runs on the Windows CI lane.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropTarget, MK_ALT,
};
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_LBUTTON, MK_SHIFT, MODIFIERKEYS_FLAGS};

use super::data_object::{
    OwnedPidl, dropped_files, empty_data_object, files_data_object, is_cmdr_drag, mark_as_cmdr_drag,
};
use super::drop_target::{TargetEvent, drop_target_for};
use super::effects::{
    DropIntent, OFFERED_EFFECTS, clear_intent, drop_effect, hover_effect, intent, modifiers_of, set_intent,
};
use super::{crossed, in_apartment, native_drag_drop};
use crate::system_events::{DragModifiers, NativeDragDrop};
use crate::test_support::TestDir;

const COPY_OR_MOVE: DROPEFFECT = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0);

// --- Effects ---

#[test]
fn a_drag_out_offers_copy_move_and_link_and_lets_the_target_choose() {
    assert_eq!(
        OFFERED_EFFECTS.0,
        DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0 | DROPEFFECT_LINK.0
    );
}

#[test]
fn the_cursor_shows_what_the_frontend_resolved() {
    assert_eq!(hover_effect(Some(DropIntent::Move), OFFERED_EFFECTS), DROPEFFECT_MOVE);
    assert_eq!(hover_effect(Some(DropIntent::Copy), OFFERED_EFFECTS), DROPEFFECT_COPY);
    assert_eq!(hover_effect(Some(DropIntent::Refuse), OFFERED_EFFECTS), DROPEFFECT_NONE);
}

#[test]
fn nothing_resolved_yet_reads_as_copy() {
    assert_eq!(hover_effect(None, OFFERED_EFFECTS), DROPEFFECT_COPY);
}

#[test]
fn an_effect_the_source_forbids_falls_back_to_one_it_allows() {
    assert_eq!(hover_effect(Some(DropIntent::Move), DROPEFFECT_COPY), DROPEFFECT_COPY);
    assert_eq!(hover_effect(Some(DropIntent::Copy), DROPEFFECT_MOVE), DROPEFFECT_MOVE);
    assert_eq!(hover_effect(Some(DropIntent::Copy), DROPEFFECT_LINK), DROPEFFECT_NONE);
}

/// A source told Move may delete its originals before the person confirms Cmdr's dialog.
#[test]
fn a_drop_answers_copy_at_most_and_never_move() {
    assert_eq!(drop_effect(OFFERED_EFFECTS), DROPEFFECT_COPY);
    assert_eq!(drop_effect(COPY_OR_MOVE), DROPEFFECT_COPY);
    assert_eq!(drop_effect(DROPEFFECT_MOVE), DROPEFFECT_NONE);
}

#[test]
fn the_frontend_names_copy_move_or_none() {
    assert_eq!(DropIntent::from_frontend("copy"), Some(DropIntent::Copy));
    assert_eq!(DropIntent::from_frontend("move"), Some(DropIntent::Move));
    assert_eq!(DropIntent::from_frontend("none"), Some(DropIntent::Refuse));
    assert_eq!(DropIntent::from_frontend("link"), None);
}

#[test]
fn ole_key_state_maps_to_the_modifiers_without_a_command_key() {
    let keys = MODIFIERKEYS_FLAGS(MK_CONTROL.0 | MK_SHIFT.0 | MK_LBUTTON.0);
    assert_eq!(
        modifiers_of(keys),
        DragModifiers {
            alt_held: false,
            cmd_held: false,
            ctrl_held: true,
            shift_held: true,
        }
    );
    assert!(modifiers_of(MODIFIERKEYS_FLAGS(MK_ALT)).alt_held);
    assert_eq!(modifiers_of(MK_LBUTTON), DragModifiers::default());
}

// --- Data objects ---

/// Two real files and a folder under a temp dir, as the paths Windows names them.
fn sample_items(dir: &TestDir) -> Vec<PathBuf> {
    std::fs::write(dir.join("a.txt"), b"a").expect("writes a.txt");
    std::fs::write(dir.join("b c.txt"), b"b").expect("writes b c.txt");
    std::fs::create_dir(dir.join("folder")).expect("makes folder");
    ["a.txt", "b c.txt", "folder"]
        .iter()
        .map(|name| dir.join(name))
        .collect()
}

fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).expect("canonical a") == std::fs::canonicalize(b).expect("canonical b")
}

fn files_object(items: &[PathBuf]) -> IDataObject {
    let lists: Vec<OwnedPidl> = items
        .iter()
        .map(|item| OwnedPidl::for_path(item).expect("the Shell names a real file"))
        .collect();
    files_data_object(&lists).expect("the Shell builds a data object")
}

#[test]
fn a_drag_of_files_carries_them_as_cf_hdrop_in_order() {
    in_apartment(|| {
        let dir = TestDir::new("drag_out_hdrop");
        let items = sample_items(&dir);
        let data = files_object(&items);
        let read = dropped_files(&data).expect("CF_HDROP is there");
        assert_eq!(read.len(), items.len());
        for (sent, got) in items.iter().zip(&read) {
            assert!(
                same_file(sent, got),
                "{} came back as {}",
                sent.display(),
                got.display()
            );
        }
    })
    .expect("enters an apartment");
}

#[test]
fn a_missing_file_has_no_item_list() {
    in_apartment(|| {
        let dir = TestDir::new("drag_out_missing");
        assert!(OwnedPidl::for_path(&dir.join("never-made.txt")).is_none());
    })
    .expect("enters an apartment");
}

#[test]
fn every_drag_of_cmdrs_is_marked_and_another_apps_is_not() {
    in_apartment(|| {
        let dir = TestDir::new("drag_out_mark");
        let data = files_object(&sample_items(&dir));
        assert!(!is_cmdr_drag(&data));
        mark_as_cmdr_drag(&data).expect("the Shell's object takes a private format");
        assert!(is_cmdr_drag(&data));
        // Marking leaves the files readable.
        assert_eq!(dropped_files(&data).map(|files| files.len()), Some(3));
    })
    .expect("enters an apartment");
}

#[test]
fn a_virtual_drag_offers_no_files_but_is_still_known_as_cmdrs() {
    in_apartment(|| {
        let data = empty_data_object().expect("an empty Shell object");
        mark_as_cmdr_drag(&data).expect("marks it");
        assert_eq!(dropped_files(&data), None);
        assert!(is_cmdr_drag(&data));
    })
    .expect("enters an apartment");
}

// --- The drop target ---

/// A drop target with no window (positions stay in screen pixels) recording what it sends.
fn recording_target() -> (IDropTarget, Rc<RefCell<Vec<TargetEvent>>>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink_seen = Rc::clone(&seen);
    let target = drop_target_for(
        HWND::default(),
        Rc::new(move |event| sink_seen.borrow_mut().push(event)),
    );
    (target, seen)
}

fn at(x: i32, y: i32) -> POINTL {
    POINTL { x, y }
}

/// Drives the target's COM methods as OLE would, answering the effect each one wrote.
struct Driver(IDropTarget);

impl Driver {
    fn enter(&self, data: &IDataObject, keys: MODIFIERKEYS_FLAGS, pt: POINTL, allowed: DROPEFFECT) -> DROPEFFECT {
        let mut effect = allowed;
        // SAFETY: a live data object and a local effect slot, as OLE passes them.
        unsafe { self.0.DragEnter(data, keys, pt, &mut effect) }.expect("DragEnter");
        effect
    }
    fn over(&self, keys: MODIFIERKEYS_FLAGS, pt: POINTL, allowed: DROPEFFECT) -> DROPEFFECT {
        let mut effect = allowed;
        // SAFETY: as above.
        unsafe { self.0.DragOver(keys, pt, &mut effect) }.expect("DragOver");
        effect
    }
    fn drop_on(&self, data: &IDataObject, keys: MODIFIERKEYS_FLAGS, pt: POINTL, allowed: DROPEFFECT) -> DROPEFFECT {
        let mut effect = allowed;
        // SAFETY: as above.
        unsafe { self.0.Drop(data, keys, pt, &mut effect) }.expect("Drop");
        effect
    }
    fn leave(&self) {
        // SAFETY: a plain COM call.
        unsafe { self.0.DragLeave() }.expect("DragLeave");
    }
}

#[test]
fn a_drop_from_explorer_reports_its_files_and_answers_copy_even_for_a_move() {
    in_apartment(|| {
        let dir = TestDir::new("drop_in_explorer");
        let items = sample_items(&dir);
        let data = files_object(&items);
        let (target, seen) = recording_target();
        let ole = Driver(target);

        // Explorer's own drag allows everything; Shift asks for a move.
        let entered = ole.enter(&data, MK_SHIFT, at(10, 20), OFFERED_EFFECTS);
        assert_eq!(entered, DROPEFFECT_COPY, "nothing resolved yet reads as copy");
        set_intent(DropIntent::Move);
        assert_eq!(ole.over(MK_SHIFT, at(11, 20), OFFERED_EFFECTS), DROPEFFECT_MOVE);
        let dropped = ole.drop_on(&data, MK_SHIFT, at(11, 20), OFFERED_EFFECTS);
        assert_eq!(dropped, DROPEFFECT_COPY, "Cmdr moves the files itself after its dialog");
        assert_eq!(intent(), None, "a drop forgets the resolved operation");

        let seen = seen.borrow();
        let shift = DragModifiers {
            shift_held: true,
            ..DragModifiers::default()
        };
        assert_eq!(seen[0], TargetEvent::Modifiers(shift));
        let TargetEvent::Enter { paths, position } = &seen[1] else {
            panic!("expected enter, got {:?}", seen[1]);
        };
        assert_eq!(*position, (10, 20));
        assert_eq!(paths.len(), 3);
        assert_eq!(seen[2], TargetEvent::Over { position: (11, 20) });
        let TargetEvent::Drop { paths, .. } = &seen[3] else {
            panic!("expected drop, got {:?}", seen[3]);
        };
        assert!(same_file(&paths[0], &items[0]));
        assert_eq!(seen.len(), 4);
    })
    .expect("enters an apartment");
}

#[test]
fn a_still_cursor_sends_nothing_new_but_a_key_change_does() {
    in_apartment(|| {
        clear_intent();
        let dir = TestDir::new("drop_in_still");
        let data = files_object(&sample_items(&dir));
        let (target, seen) = recording_target();
        let ole = Driver(target);
        ole.enter(&data, MK_LBUTTON, at(5, 5), OFFERED_EFFECTS);
        ole.over(MK_LBUTTON, at(5, 5), OFFERED_EFFECTS);
        ole.over(
            MODIFIERKEYS_FLAGS(MK_LBUTTON.0 | MK_CONTROL.0),
            at(5, 5),
            OFFERED_EFFECTS,
        );
        let seen = seen.borrow();
        assert_eq!(seen.len(), 3, "modifiers, enter, then only the Ctrl change: {seen:?}");
        assert!(matches!(
            seen[2],
            TargetEvent::Modifiers(DragModifiers { ctrl_held: true, .. })
        ));
    })
    .expect("enters an apartment");
}

#[test]
fn cmdrs_own_drag_from_a_phone_is_taken_with_no_files() {
    in_apartment(|| {
        let data = empty_data_object().expect("an empty Shell object");
        mark_as_cmdr_drag(&data).expect("marks it");
        // Seeded at drag start: an own drag keeps it on enter.
        set_intent(DropIntent::Move);
        let (target, seen) = recording_target();
        let ole = Driver(target);
        assert_eq!(ole.enter(&data, MK_LBUTTON, at(1, 1), OFFERED_EFFECTS), DROPEFFECT_MOVE);
        ole.drop_on(&data, MK_LBUTTON, at(1, 1), OFFERED_EFFECTS);
        let seen = seen.borrow();
        assert_eq!(
            seen[1],
            TargetEvent::Enter {
                paths: Vec::new(),
                position: (1, 1)
            }
        );
        assert_eq!(
            seen[2],
            TargetEvent::Drop {
                paths: Vec::new(),
                position: (1, 1)
            }
        );
    })
    .expect("enters an apartment");
}

#[test]
fn a_drag_with_no_files_from_another_app_is_refused_silently() {
    in_apartment(|| {
        let data = empty_data_object().expect("an empty Shell object");
        let (target, seen) = recording_target();
        let ole = Driver(target);
        assert_eq!(ole.enter(&data, MK_LBUTTON, at(1, 1), OFFERED_EFFECTS), DROPEFFECT_NONE);
        assert_eq!(ole.over(MK_LBUTTON, at(2, 2), OFFERED_EFFECTS), DROPEFFECT_NONE);
        ole.leave();
        assert_eq!(
            ole.drop_on(&data, MK_LBUTTON, at(2, 2), OFFERED_EFFECTS),
            DROPEFFECT_NONE
        );
        assert!(seen.borrow().is_empty());
    })
    .expect("enters an apartment");
}

#[test]
fn the_resolved_refusal_shows_the_no_drop_cursor_and_leave_is_reported_once() {
    in_apartment(|| {
        let dir = TestDir::new("drop_in_refuse");
        let data = files_object(&sample_items(&dir));
        let (target, seen) = recording_target();
        let ole = Driver(target);
        ole.enter(&data, MK_LBUTTON, at(1, 1), COPY_OR_MOVE);
        set_intent(DropIntent::Refuse);
        assert_eq!(ole.over(MK_LBUTTON, at(3, 3), COPY_OR_MOVE), DROPEFFECT_NONE);
        ole.leave();
        ole.leave();
        let leaves = seen.borrow().iter().filter(|e| **e == TargetEvent::Leave).count();
        assert_eq!(leaves, 1);
        clear_intent();
    })
    .expect("enters an apartment");
}

// --- What the frontend hears ---

#[test]
fn dropped_windows_paths_reach_the_frontend_as_app_paths() {
    let natives = [
        PathBuf::from(r"C:\Users\test_user_1\a.txt"),
        PathBuf::from(r"\\?\Volume{0}\x"),
    ];
    let (paths, refused) = crossed(&natives);
    assert_eq!(paths, vec!["C:/Users/test_user_1/a.txt".to_string()]);
    assert_eq!(refused, 1);

    let event = native_drag_drop(TargetEvent::Drop {
        paths: natives.to_vec(),
        position: (30, 40),
    })
    .expect("a drop is a drag-drop event");
    let NativeDragDrop::Drop {
        paths,
        refused,
        position,
    } = event
    else {
        panic!("expected a drop");
    };
    assert_eq!(paths, vec!["C:/Users/test_user_1/a.txt".to_string()]);
    assert_eq!(refused, 1);
    assert!((position.x - 30.0).abs() < f64::EPSILON && (position.y - 40.0).abs() < f64::EPSILON);
}

#[test]
fn a_key_change_reaches_the_frontend_as_drag_modifiers() {
    let ctrl = DragModifiers {
        ctrl_held: true,
        ..DragModifiers::default()
    };
    assert_eq!(native_drag_drop(TargetEvent::Modifiers(ctrl)), Err(ctrl));
}

#[test]
fn the_drop_event_is_tagged_by_type_with_camel_case_fields() {
    let json = serde_json::to_value(NativeDragDrop::Leave).expect("serializes");
    assert_eq!(json, serde_json::json!({ "type": "leave" }));
}
