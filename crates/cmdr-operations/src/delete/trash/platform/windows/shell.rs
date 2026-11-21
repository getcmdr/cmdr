//! One `IFileOperation` delete with the recycle flags, and the progress sink that
//! keeps it from ever being a permanent delete.
//!
//! (verified on Windows 10 22H2, NTFS, `windows_tests.rs` on the builder, 2026-11-21:
//! a file and a folder on `C:` land as `$R…` items under `C:\$Recycle.Bin\<SID>\`; a
//! file on `\\localhost\C$` has no Recycle Bin, and the sink's veto leaves it in place.)

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use windows::Win32::Foundation::{E_ACCESSDENIED, ERROR_ACCESS_DENIED, ERROR_CANCELLED, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::UI::Shell::{
    COPYENGINE_E_ACCESS_DENIED_SRC, FOF_ALLOWUNDO, FOF_NO_UI, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
    IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH, TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::{HRESULT, HSTRING, PCWSTR, Ref};
use windows_core::implement;

use super::super::super::super::super::types::TrashRefusalKind;

/// Where a recycled item went.
pub(super) enum Recycled {
    /// The Shell's `$R…` item for it, a native path.
    InBin(PathBuf),
    /// Recycled, but the Shell didn't name the item.
    Unnamed,
}

/// Why an item stayed where it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The Shell would have deleted it for good: its drive has no Recycle Bin, or the
    /// bin can't take it. Our sink stopped it.
    NoRecycleBin,
    /// The Shell's own answer for this item, an `HRESULT`.
    Refused(HRESULT),
    /// A COM call before the delete failed (no Shell, a path it can't parse).
    Setup(HRESULT),
}

impl Refusal {
    /// The typed reason the UI words, from the Shell's code, ❌ never its message.
    pub(super) fn kind(self) -> TrashRefusalKind {
        match self {
            Self::NoRecycleBin => TrashRefusalKind::NoTrashForVolume,
            Self::Refused(hr) => refusal_kind_of(hr),
            Self::Setup(_) => TrashRefusalKind::Other,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRecycleBin => f.write_str("no Recycle Bin can take it, and Windows would have deleted it for good"),
            Self::Refused(hr) => write!(f, "the Shell refused it [HRESULT {:#010x}]", hr.0 as u32),
            Self::Setup(hr) => write!(f, "the Shell couldn't start the move [HRESULT {:#010x}]", hr.0 as u32),
        }
    }
}

/// The reason for an `HRESULT` the Shell reported for one item: an access refusal is
/// `NotPermitted`, the rest `Other`. Both the plain Win32 codes and the copy engine's
/// own (`COPYENGINE_E_*_SRC`) count.
pub(super) fn refusal_kind_of(hr: HRESULT) -> TrashRefusalKind {
    let win32 = |code: windows::Win32::Foundation::WIN32_ERROR| HRESULT::from_win32(code.0);
    if hr == E_ACCESSDENIED || hr == win32(ERROR_ACCESS_DENIED) || hr == COPYENGINE_E_ACCESS_DENIED_SRC {
        TrashRefusalKind::NotPermitted
    } else {
        TrashRefusalKind::Other
    }
}

/// Recycles the item at the plain path `plain` (a whole tree for a folder), or
/// leaves it exactly where it was.
pub(super) fn recycle(plain: &Path) -> Result<Recycled, Refusal> {
    let _apartment = Apartment::enter().map_err(Refusal::Setup)?;
    let verdict = Rc::new(RefCell::new(Verdict::default()));
    let sink: IFileOperationProgressSink = DeleteSink {
        verdict: Rc::clone(&verdict),
    }
    .into();

    // SAFETY: plain COM calls on an apartment this thread entered above; every
    // interface is a ref-counted smart pointer released on drop, before `_apartment`.
    let performed = unsafe {
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(setup)?;
        // No UI at all (no progress, no confirmation, no error dialog), the first
        // error stops it, and a delete recycles.
        operation
            .SetOperationFlags(FOF_NO_UI | FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE)
            .map_err(setup)?;
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(plain.as_os_str()), None).map_err(setup)?;
        operation.DeleteItem(&item, &sink).map_err(setup)?;
        operation.PerformOperations()
    };

    let verdict = verdict.borrow();
    if verdict.vetoed {
        return Err(Refusal::NoRecycleBin);
    }
    if let Some(hr) = verdict.result.filter(|hr| hr.is_err()) {
        return Err(Refusal::Refused(hr));
    }
    if let Err(e) = performed {
        return Err(Refusal::Refused(e.code()));
    }
    match (&verdict.result, &verdict.landed) {
        (Some(_), Some(landed)) => Ok(Recycled::InBin(landed.clone())),
        (Some(_), None) => Ok(Recycled::Unnamed),
        // The Shell reported nothing for the item: nothing is known to have moved.
        (None, _) => Err(Refusal::Refused(HRESULT::from_win32(ERROR_CANCELLED.0))),
    }
}

fn setup(e: windows::core::Error) -> Refusal {
    Refusal::Setup(e.code())
}

/// A single-threaded COM apartment for the call, left on drop. A thread already in a
/// multithreaded one (`RPC_E_CHANGED_MODE`) stays in it: `IFileOperation` works there
/// too, and that apartment isn't ours to leave.
struct Apartment {
    entered: bool,
}

impl Apartment {
    fn enter() -> Result<Self, HRESULT> {
        // SAFETY: initializing COM on the calling thread; balanced by `Drop`.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if hr == RPC_E_CHANGED_MODE {
            Ok(Self { entered: false })
        } else if hr.is_ok() {
            Ok(Self { entered: true })
        } else {
            Err(hr)
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.entered {
            // SAFETY: balances the successful `CoInitializeEx` in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}

/// What the sink saw of the one delete.
#[derive(Default)]
struct Verdict {
    /// The Shell announced a delete it wouldn't recycle, and the sink stopped it.
    vetoed: bool,
    /// The delete's own result, from `PostDeleteItem`.
    result: Option<HRESULT>,
    /// The item it became in the Recycle Bin.
    landed: Option<PathBuf>,
}

#[implement(IFileOperationProgressSink)]
struct DeleteSink {
    verdict: Rc<RefCell<Verdict>>,
}

impl IFileOperationProgressSink_Impl for DeleteSink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn FinishOperations(&self, _hrresult: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostRenameItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrrename: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreMoveItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostMoveItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrmove: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreCopyItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostCopyItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _hrcopy: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    /// The veto: a delete the Shell won't recycle is refused before it starts, which
    /// cancels it and everything after it (the `PreDeleteItem` contract).
    fn PreDeleteItem(&self, dwflags: u32, _psiitem: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        if dwflags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            self.verdict.borrow_mut().vetoed = true;
            return Err(HRESULT::from_win32(ERROR_CANCELLED.0).into());
        }
        Ok(())
    }

    fn PostDeleteItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        hrdelete: HRESULT,
        psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        let mut verdict = self.verdict.borrow_mut();
        verdict.result = Some(hrdelete);
        verdict.landed = psinewlycreated.as_ref().and_then(file_system_path);
        Ok(())
    }

    fn PreNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn PostNewItem(
        &self,
        _dwflags: u32,
        _psidestinationfolder: Ref<'_, IShellItem>,
        _psznewname: &PCWSTR,
        _psztemplatename: &PCWSTR,
        _dwfileattributes: u32,
        _hrnew: HRESULT,
        _psinewitem: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn UpdateProgress(&self, _iworktotal: u32, _iworksofar: u32) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

/// A Shell item's file-system path, when it has one.
fn file_system_path(item: &IShellItem) -> Option<PathBuf> {
    // SAFETY: `GetDisplayName` hands back a `CoTaskMemAlloc`ed string we own, freed
    // right after it's copied.
    unsafe {
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0 as *const _));
        path
    }
}
