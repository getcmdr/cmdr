//! The OLE drop effects a Cmdr drag offers and answers, and the modifier keys a drag reports.
//! Pure: no COM, so every rule here is a plain test.

use std::sync::atomic::{AtomicU8, Ordering};

use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, MK_ALT,
};
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_SHIFT, MODIFIERKEYS_FLAGS};

use crate::system_events::DragModifiers;

/// What a drag out of Cmdr lets its target do: everything Explorer's own drags allow. The target
/// picks among them by its own rules and the keys held (Explorer: Move on the same drive, Copy
/// across drives, Ctrl copies, Shift moves, Alt or Ctrl+Shift makes a shortcut), as Finder does
/// with the permissive mask on macOS.
pub(super) const OFFERED_EFFECTS: DROPEFFECT = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0 | DROPEFFECT_LINK.0);

/// What the frontend resolved a drop over Cmdr would do (`set_self_drag_resolved_op`), so the
/// cursor says the same as the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DropIntent {
    Copy,
    Move,
    /// No drop target under the cursor, or one that refuses this drag.
    Refuse,
}

impl DropIntent {
    /// The intent the frontend named: `copy`, `move`, or `none`; anything else is unknown.
    pub(super) fn from_frontend(operation: &str) -> Option<Self> {
        match operation {
            "copy" => Some(Self::Copy),
            "move" => Some(Self::Move),
            "none" => Some(Self::Refuse),
            _ => None,
        }
    }
}

/// The intent of the drag over Cmdr right now: 0 for none said yet, else a [`DropIntent`].
static INTENT: AtomicU8 = AtomicU8::new(0);

/// Remembers what the frontend resolved for the drag under the cursor.
pub(super) fn set_intent(intent: DropIntent) {
    let code = match intent {
        DropIntent::Copy => 1,
        DropIntent::Move => 2,
        DropIntent::Refuse => 3,
    };
    INTENT.store(code, Ordering::Relaxed);
}

/// Forgets it: a new drag starts with nothing said.
pub(super) fn clear_intent() {
    INTENT.store(0, Ordering::Relaxed);
}

/// What the frontend last resolved, if it said anything for this drag.
pub(super) fn intent() -> Option<DropIntent> {
    match INTENT.load(Ordering::Relaxed) {
        1 => Some(DropIntent::Copy),
        2 => Some(DropIntent::Move),
        3 => Some(DropIntent::Refuse),
        _ => None,
    }
}

/// The effect a drag over Cmdr shows (the cursor's "Copy" or "Move" badge), given what the
/// frontend resolved and what the source `allowed`. Nothing said yet reads as Copy, wry's old
/// answer, until the frontend's first answer lands a move later. An effect the source doesn't
/// allow falls back to one it does: the drop is still Cmdr's own transfer, and its dialog says
/// what it does.
pub(super) fn hover_effect(intent: Option<DropIntent>, allowed: DROPEFFECT) -> DROPEFFECT {
    let wanted = match intent {
        Some(DropIntent::Refuse) => return DROPEFFECT_NONE,
        Some(DropIntent::Move) => DROPEFFECT_MOVE,
        Some(DropIntent::Copy) | None => DROPEFFECT_COPY,
    };
    [wanted, DROPEFFECT_COPY, DROPEFFECT_MOVE]
        .into_iter()
        .find(|effect| allowed.0 & effect.0 != 0)
        .unwrap_or(DROPEFFECT_NONE)
}

/// The effect a drop on Cmdr answers its source: Copy when the source allows it, else nothing.
///
/// ❌ Never Move, even for a move: the drop only opens Cmdr's transfer dialog, and Cmdr moves the
/// files itself once the person confirms. A source told Move may delete its originals as soon as
/// `DoDragDrop` returns (the shell's "non-optimized move"), before the person has said yes.
pub(super) fn drop_effect(allowed: DROPEFFECT) -> DROPEFFECT {
    if allowed.0 & DROPEFFECT_COPY.0 != 0 {
        DROPEFFECT_COPY
    } else {
        DROPEFFECT_NONE
    }
}

/// The modifier keys OLE reports with a drag. There is no Command key on Windows.
pub(super) fn modifiers_of(keys: MODIFIERKEYS_FLAGS) -> DragModifiers {
    DragModifiers {
        alt_held: keys.0 & MK_ALT != 0,
        cmd_held: false,
        ctrl_held: keys.0 & MK_CONTROL.0 != 0,
        shift_held: keys.0 & MK_SHIFT.0 != 0,
    }
}
