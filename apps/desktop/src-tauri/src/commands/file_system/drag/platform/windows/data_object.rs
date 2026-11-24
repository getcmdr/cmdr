//! The OLE data objects a drag carries, and reading the files out of one.
//!
//! A drag of real files carries the Shell's own data object for them (`BHID_DataObject` on a
//! Shell item array), the one Explorer drags: `CF_HDROP`, the Shell ID list, and everything else
//! Explorer, browsers, and mail composers read, plus `SetData` for the drag image and drop
//! descriptions the Shell's helpers attach. So no `DROPFILES` is written here. Every drag of
//! Cmdr's also carries [`CMDR_DRAG_FORMAT`], so Cmdr's drop target knows its own drag, even one
//! that offers other apps nothing (a phone, search results, inside an archive).

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::OnceLock;

use windows::Win32::Foundation::{E_FAIL, GlobalFree, HGLOBAL, S_OK};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc};
use windows::Win32::System::Ole::{CF_HDROP, ReleaseStgMedium};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, DragQueryFileW, HDROP, ILFree, IShellItemArray, SHCreateDataObject,
    SHCreateShellItemArrayFromIDLists, SHParseDisplayName,
};
use windows::core::{HSTRING, PCWSTR};

/// The name of the private clipboard format that marks a drag as Cmdr's own.
const CMDR_DRAG_FORMAT: &str = "Cmdr in-app drag";

/// A Shell item ID list for one file, freed on drop. Plain `CoTaskMem` memory, so it may move
/// between threads: built on a worker (parsing a path can wait on a network share), used on the
/// UI thread.
pub(super) struct OwnedPidl(NonNull<ITEMIDLIST>);

// SAFETY: an ID list is a flat byte block from the COM task allocator with no thread affinity;
// `OwnedPidl` owns it alone, and `ILFree` may run on any thread.
unsafe impl Send for OwnedPidl {}

impl OwnedPidl {
    /// The ID list for the file or folder at `native`, or `None` when the Shell can't name it
    /// (gone, no access, or a path the Shell won't parse). Tries each of [`shell_spellings`] in
    /// turn. Asks the file system, so ❌ never on the UI thread; the calling thread must be in a
    /// COM apartment.
    pub(super) fn for_path(native: &Path) -> Option<Self> {
        shell_spellings(native)
            .iter()
            .find_map(|spelling| Self::parse(spelling))
    }

    fn parse(text: &OsStr) -> Option<Self> {
        let wide: Vec<u16> = text.encode_wide().chain(std::iter::once(0)).collect();
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the call, and `pidl` a
        // live local the call fills; on success the list is ours to free.
        let parsed = unsafe { SHParseDisplayName(PCWSTR::from_raw(wide.as_ptr()), None, &mut pidl, 0, None) };
        parsed.ok().and(NonNull::new(pidl)).map(Self)
    }

    fn as_ptr(&self) -> *const ITEMIDLIST {
        self.0.as_ptr().cast_const()
    }
}

impl Drop for OwnedPidl {
    fn drop(&mut self) {
        // SAFETY: the list came from `SHParseDisplayName` and is freed exactly once, here.
        unsafe { ILFree(Some(self.as_ptr())) };
    }
}

/// How the Shell may spell `native`, most likely first. A `\\?\` path, which the Shell doesn't
/// always parse, also gets its plain form first (`C:\…`, `\\server\share\…`). Pure text.
pub(super) fn shell_spellings(native: &Path) -> Vec<OsString> {
    let text = native.as_os_str();
    let plain = text.to_str().and_then(|s| s.strip_prefix(r"\\?\")).and_then(|rest| {
        if let Some(share) = rest.strip_prefix(r"UNC\") {
            Some(format!(r"\\{share}"))
        } else if rest.as_bytes().get(1) == Some(&b':') {
            Some(rest.to_owned())
        } else {
            // A volume GUID or device path: no plain form.
            None
        }
    });
    match plain {
        Some(plain) => vec![OsString::from(plain), text.to_owned()],
        None => vec![text.to_owned()],
    }
}

/// The Shell's data object for `items`, the one Explorer drags. The Shell copies the lists, so
/// `items` may go once this returns. UI thread (the object is apartment-threaded).
///
/// Items from one folder go through a Shell item array, Explorer's own path. Items from several
/// folders (where that object doesn't carry them all) go as absolute lists under the desktop,
/// which names any file. Either way the object must carry every item as `CF_HDROP`, else it's
/// refused (`E_FAIL`): a drag that quietly offers fewer files than it shows is worse than none.
pub(super) fn files_data_object(items: &[OwnedPidl]) -> windows::core::Result<IDataObject> {
    let lists: Vec<*const ITEMIDLIST> = items.iter().map(OwnedPidl::as_ptr).collect();
    let carries_all = |data: &IDataObject| dropped_files(data).map(|files| files.len()) == Some(items.len());
    // SAFETY: every pointer is a live ID list owned by `items`, which outlives both calls.
    let from_array: windows::core::Result<IDataObject> = unsafe {
        SHCreateShellItemArrayFromIDLists(&lists)
            .and_then(|array: IShellItemArray| array.BindToHandler(None, &BHID_DataObject))
    };
    if let Ok(data) = from_array
        && carries_all(&data)
    {
        return Ok(data);
    }
    // SAFETY: no parent folder (the desktop), so each absolute list is its own child; they live
    // through the call, and the Shell copies them.
    let from_desktop: IDataObject = unsafe { SHCreateDataObject(None, Some(&lists), None) }?;
    if carries_all(&from_desktop) {
        Ok(from_desktop)
    } else {
        Err(windows::core::Error::from(E_FAIL))
    }
}

/// Whether `CF_HDROP` can carry `native`: its plain spelling fits in `MAX_PATH` (260 UTF-16 units
/// with the terminator). Past it the Shell still names the file, but its data object offers no
/// `CF_HDROP` for it (verified on the Windows CI runner, `tests.rs`, 2026-11-25).
pub(super) fn fits_cf_hdrop(native: &Path) -> bool {
    const MAX_PATH: usize = 260;
    shell_spellings(native)
        .first()
        .is_some_and(|plain| plain.encode_wide().count() < MAX_PATH)
}

/// An empty Shell data object: a drag that offers other apps nothing, which Cmdr's own drop
/// target still knows by [`mark_as_cmdr_drag`].
pub(super) fn empty_data_object() -> windows::core::Result<IDataObject> {
    // SAFETY: no folder, no items, no inner object: the documented way to get an empty object.
    unsafe { SHCreateDataObject(None, None, None) }
}

/// The registered id of [`CMDR_DRAG_FORMAT`], asked once.
fn cmdr_drag_format() -> u16 {
    static FORMAT: OnceLock<u16> = OnceLock::new();
    *FORMAT.get_or_init(|| {
        // SAFETY: a NUL-terminated name that outlives the call; registering is idempotent.
        let id = unsafe { RegisterClipboardFormatW(&HSTRING::from(CMDR_DRAG_FORMAT)) };
        // A registered format is 0xC000..=0xFFFF; 0 means it failed, and stays 0 (never matches).
        u16::try_from(id).unwrap_or(0)
    })
}

/// A request for `format` in global memory, the only medium these formats use.
fn hglobal_format(format: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0.cast_unsigned(),
    }
}

/// Marks `data` as a drag of Cmdr's own.
pub(super) fn mark_as_cmdr_drag(data: &IDataObject) -> windows::core::Result<()> {
    let format = hglobal_format(cmdr_drag_format());
    // SAFETY: a fresh 1-byte block; on success the object takes it (`fRelease` true), on failure
    // it's freed below and never touched again.
    unsafe {
        let block: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, 1)?;
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0.cast_unsigned(),
            u: STGMEDIUM_0 { hGlobal: block },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        };
        let stored = data.SetData(&format, &medium, true);
        if stored.is_err() {
            let _freed = GlobalFree(Some(block));
        }
        stored
    }
}

/// Whether `data` is a drag of Cmdr's own.
pub(super) fn is_cmdr_drag(data: &IDataObject) -> bool {
    let format = hglobal_format(cmdr_drag_format());
    // SAFETY: a plain query with a well-formed `FORMATETC` that outlives the call.
    unsafe { data.QueryGetData(&format) == S_OK }
}

/// The files `data` carries as `CF_HDROP`, in order, or `None` when it carries none.
pub(super) fn dropped_files(data: &IDataObject) -> Option<Vec<PathBuf>> {
    let format = hglobal_format(CF_HDROP.0);
    // SAFETY: `GetData` hands us a medium we own; we read it as an `HDROP` (what `CF_HDROP`
    // in global memory is) and release it with `ReleaseStgMedium`, which honors
    // `pUnkForRelease` (wry's `DragFinish` on it doesn't).
    unsafe {
        let mut medium = data.GetData(&format).ok()?;
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        let mut files = Vec::with_capacity(count as usize);
        for index in 0..count {
            let length = DragQueryFileW(hdrop, index, None) as usize;
            let mut buffer = vec![0u16; length + 1];
            let copied = DragQueryFileW(hdrop, index, Some(&mut buffer)) as usize;
            buffer.truncate(copied);
            files.push(PathBuf::from(OsString::from_wide(&buffer)));
        }
        ReleaseStgMedium(&mut medium);
        Some(files)
    }
}
