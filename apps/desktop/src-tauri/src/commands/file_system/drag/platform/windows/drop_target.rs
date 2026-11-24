//! Cmdr's OLE drop target, registered on each webview's windows in place of wry's.
//!
//! Why ours: wry's (0.57.0, `webview2/drag_drop.rs`) shows Copy for every drag, ignores the keys
//! held, never sets the effect a drop answers (so the source reads back every effect it allowed,
//! Move included), skips any drag without `CF_HDROP` (so a drag of Cmdr's own from a phone fired
//! nothing), and frees the dropped files with `DragFinish` where `ReleaseStgMedium` is due. This
//! one reports the same enter/over/drop/leave, plus the modifier keys, shows the operation the
//! frontend resolved, and answers a drop with Copy at most (`effects::drop_effect`).
//!
//! Every method runs on the UI thread, inside the source's drag loop: each only reads the data
//! object (memory, no file I/O) and hands an event to its sink.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::core::{Ref, implement};

use super::data_object::{dropped_files, is_cmdr_drag};
use super::effects::{clear_intent, drop_effect, hover_effect, intent, modifiers_of};
use crate::system_events::DragModifiers;

/// What the drop target saw, for its sink to send on.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum TargetEvent {
    /// A drag came over the window, carrying `paths` (native; none for a drag of Cmdr's own
    /// that offers no files).
    Enter { paths: Vec<PathBuf>, position: (i32, i32) },
    /// The drag moved, in client pixels.
    Over { position: (i32, i32) },
    /// It was dropped here.
    Drop { paths: Vec<PathBuf>, position: (i32, i32) },
    /// It left, or was cancelled over the window.
    Leave,
    /// The modifier keys changed (and on every enter).
    Modifiers(DragModifiers),
}

/// Where a drop target's events go.
pub(super) type Sink = Rc<dyn Fn(TargetEvent)>;

/// One window's drop target.
#[implement(IDropTarget)]
pub(super) struct DropTarget {
    /// The window registered, whose client area positions are in.
    hwnd: HWND,
    sink: Sink,
    /// A drag this target took (files, or Cmdr's own) is over the window.
    active: Cell<bool>,
    last_position: Cell<Option<(i32, i32)>>,
    last_modifiers: Cell<Option<DragModifiers>>,
}

impl DropTarget {
    pub(super) fn new(hwnd: HWND, sink: Sink) -> Self {
        Self {
            hwnd,
            sink,
            active: Cell::new(false),
            last_position: Cell::new(None),
            last_modifiers: Cell::new(None),
        }
    }

    /// `pt` (screen pixels) in the window's client area. Left in screen pixels if Windows can't
    /// map it (a window already gone).
    fn client_position(&self, pt: &POINTL) -> (i32, i32) {
        let mut point = POINT { x: pt.x, y: pt.y };
        // SAFETY: `point` is a live local; a stale `hwnd` makes the call fail, not misbehave.
        let _mapped = unsafe { ScreenToClient(self.hwnd, &mut point) };
        (point.x, point.y)
    }

    fn report_modifiers(&self, keys: MODIFIERKEYS_FLAGS, force: bool) {
        let now = modifiers_of(keys);
        if force || self.last_modifiers.get() != Some(now) {
            self.last_modifiers.set(Some(now));
            (self.sink)(TargetEvent::Modifiers(now));
        }
    }
}

/// Writes `effect` through OLE's out pointer.
///
/// # Safety
///
/// `out` is the `pdwEffect` OLE passed this call, valid for reads and writes, or null.
unsafe fn answer(out: *mut DROPEFFECT, effect: DROPEFFECT) {
    // SAFETY: the caller's contract.
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = effect;
    }
}

/// What the source allowed, read from OLE's in/out pointer.
///
/// # Safety
///
/// As [`answer`].
unsafe fn allowed(out: *mut DROPEFFECT) -> DROPEFFECT {
    // SAFETY: the caller's contract.
    unsafe { out.as_ref() }.copied().unwrap_or(DROPEFFECT_NONE)
}

#[allow(non_snake_case, reason = "COM method names")]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let data = pdataobj.as_ref();
        let files = data.and_then(dropped_files);
        let ours = data.is_some_and(is_cmdr_drag);
        // SAFETY: `pdweffect` is OLE's pointer for this call.
        let offered = unsafe { allowed(pdweffect) };
        if files.is_none() && !ours {
            // Text, a link, a browser image: nothing Cmdr reads yet (wry took none of these either).
            self.active.set(false);
            // SAFETY: as above.
            unsafe { answer(pdweffect, DROPEFFECT_NONE) };
            return Ok(());
        }
        if !ours {
            // A drag from another app starts with nothing resolved; Cmdr's own was seeded at start.
            clear_intent();
        }
        self.active.set(true);
        let position = self.client_position(pt);
        self.last_position.set(Some(position));
        self.report_modifiers(grfkeystate, true);
        (self.sink)(TargetEvent::Enter {
            paths: files.unwrap_or_default(),
            position,
        });
        // SAFETY: as above.
        unsafe { answer(pdweffect, hover_effect(intent(), offered)) };
        Ok(())
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: `pdweffect` is OLE's pointer for this call.
        let offered = unsafe { allowed(pdweffect) };
        if !self.active.get() {
            // SAFETY: as above.
            unsafe { answer(pdweffect, DROPEFFECT_NONE) };
            return Ok(());
        }
        self.report_modifiers(grfkeystate, false);
        // OLE calls this on a timer too; a still cursor sends nothing new.
        let position = self.client_position(pt);
        if self.last_position.replace(Some(position)) != Some(position) {
            (self.sink)(TargetEvent::Over { position });
        }
        // SAFETY: as above.
        unsafe { answer(pdweffect, hover_effect(intent(), offered)) };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if self.active.replace(false) {
            self.last_position.set(None);
            (self.sink)(TargetEvent::Leave);
        }
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // SAFETY: `pdweffect` is OLE's pointer for this call.
        let offered = unsafe { allowed(pdweffect) };
        if !self.active.replace(false) {
            // SAFETY: as above.
            unsafe { answer(pdweffect, DROPEFFECT_NONE) };
            return Ok(());
        }
        self.last_position.set(None);
        self.report_modifiers(grfkeystate, false);
        let paths = pdataobj.as_ref().and_then(dropped_files).unwrap_or_default();
        (self.sink)(TargetEvent::Drop {
            paths,
            position: self.client_position(pt),
        });
        clear_intent();
        // SAFETY: as above.
        unsafe { answer(pdweffect, drop_effect(offered)) };
        Ok(())
    }
}

/// The new target for `hwnd`, as the interface OLE takes.
pub(super) fn drop_target_for(hwnd: HWND, sink: Sink) -> IDropTarget {
    DropTarget::new(hwnd, sink).into()
}
