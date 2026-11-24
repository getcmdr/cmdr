//! Windows: files on the clipboard as the Shell puts them there, so Explorer and Cmdr paste each
//! other's copies and cuts. Plain text through `arboard` (`arboard_text.rs`).
//!
//! - **Write**: `CF_HDROP` (the paths), the registered `Preferred DropEffect` (`DROPEFFECT_MOVE`
//!   on a cut, `DROPEFFECT_COPY` on a copy), and `CF_UNICODETEXT` (the paths, one per line),
//!   through the plain clipboard API. Why not `OleSetClipboard`: `DETAILS.md` § "Windows".
//! - **Read**: `CF_HDROP` through `DragQueryFileW` (it reads the ANSI form too), and the cut mark
//!   from `Preferred DropEffect`.
//! - **After a paste-as-move of a marked cut** ([`after_cut_paste`]): `Paste Succeeded` with
//!   `DROPEFFECT_MOVE` on the source's data object (`OleGetClipboard`), then the clipboard
//!   emptied if the source left the cut on it, so a cut pastes once, as Explorer's does.
//!
//! ❗ The clipboard is one system-wide lock another app can hold: every open retries a bounded
//! number of times, on a blocking thread, never the main thread.

use std::ffi::OsString;
use std::marker::PhantomData;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::time::Duration;

use tauri::AppHandle;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT, OleGetClipboard, OleInitialize, OleUninitialize};
use windows::Win32::UI::Shell::{
    CFSTR_PASTESUCCEEDED, CFSTR_PERFORMEDDROPEFFECT, CFSTR_PREFERREDDROPEFFECT, DragQueryFileW, HDROP,
};
use windows::core::PCWSTR;

use super::super::{ClipboardFiles, ClipboardFilesError, ClipboardIntent};
use super::shell_formats::{self, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE};

pub(crate) use super::arboard_text::read_text;
pub(crate) use super::unsupported::paste_as_file;

/// How many times an open is tried before the clipboard counts as busy.
const OPEN_ATTEMPTS: u32 = 10;
/// The first wait between tries; each next one waits one more step (about 1.1 s in all).
const OPEN_BACKOFF: Duration = Duration::from_millis(20);

/// The OS's words as the typed error's log detail.
fn unexpected(what: &str, error: &windows::core::Error) -> ClipboardFilesError {
    ClipboardFilesError::Unexpected {
        detail: format!("{what}: {error}"),
    }
}

/// The clipboard, open on this thread until dropped. `!Send`: `CloseClipboard` must run on the
/// thread that opened it.
struct OpenClipboardGuard {
    _thread_bound: PhantomData<*const ()>,
}

impl OpenClipboardGuard {
    /// Opens the clipboard, retrying while another app holds it.
    fn acquire() -> Result<Self, ClipboardFilesError> {
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: `OpenClipboard(None)` takes no pointers; a success is balanced by `Drop`'s
            // `CloseClipboard` on this same thread (the guard is `!Send`). No owner window:
            // everything is rendered up front, so nothing asks an owner for a delayed format.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Self {
                    _thread_bound: PhantomData,
                });
            }
            std::thread::sleep(OPEN_BACKOFF * (attempt + 1));
        }
        Err(ClipboardFilesError::Busy)
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: balances the successful `OpenClipboard` in `acquire`, on the same thread.
        // a close that fails leaves nothing for a caller to do.
        let _ = unsafe { CloseClipboard() };
    }
}

/// A registered format's id (`Preferred DropEffect`, `Paste Succeeded`), the same for every app.
fn registered_format(name: PCWSTR) -> Result<u32, ClipboardFilesError> {
    // SAFETY: `name` is one of the `windows` crate's static NUL-terminated wide literals.
    let id = unsafe { RegisterClipboardFormatW(name) };
    if id == 0 {
        return Err(unexpected(
            "RegisterClipboardFormatW",
            &windows::core::Error::from_thread(),
        ));
    }
    Ok(id)
}

/// A movable global block holding `bytes`, as the clipboard and `STGMEDIUM` take them. The
/// caller owns it until a taker accepts it.
fn global_with(bytes: &[u8]) -> Result<HGLOBAL, ClipboardFilesError> {
    // SAFETY: a plain allocation of `bytes.len()` bytes (at least one: every caller's block ends
    // in a NUL or holds a DWORD).
    let block = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.map_err(|e| unexpected("GlobalAlloc", &e))?;
    // SAFETY: `block` is the live movable block just allocated; locking it gives its address.
    let at = unsafe { GlobalLock(block) };
    if at.is_null() {
        let error = windows::core::Error::from_thread();
        // SAFETY: `block` is ours and unlocked; freeing it is the only cleanup it needs.
        // the lock's failure is what the caller hears.
        let _ = unsafe { GlobalFree(Some(block)) };
        return Err(unexpected("GlobalLock", &error));
    }
    // SAFETY: `at` addresses `bytes.len()` writable bytes (the block's size), not overlapping
    // `bytes`, which is a Rust slice.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), at.cast::<u8>(), bytes.len()) };
    // SAFETY: balances the `GlobalLock` above. It answers "failed" with no error once the lock
    // count reaches zero, which is the success here.
    // see above; the block stays valid either way.
    let _ = unsafe { GlobalUnlock(block) };
    Ok(block)
}

/// Sets `format` to `bytes` on the open clipboard, which then owns the block.
fn set_format(_open: &OpenClipboardGuard, format: u32, bytes: &[u8]) -> Result<(), ClipboardFilesError> {
    let block = global_with(bytes)?;
    // SAFETY: the clipboard is open on this thread (`_open`) and emptied by us; `block` is a
    // movable global block holding the format's bytes, which the system takes on success.
    if let Err(error) = unsafe { SetClipboardData(format, Some(HANDLE(block.0))) } {
        // SAFETY: the system refused `block`, so it's still ours to free.
        // the refusal is what the caller hears.
        let _ = unsafe { GlobalFree(Some(block)) };
        return Err(unexpected("SetClipboardData", &error));
    }
    Ok(())
}

/// `format`'s bytes on the open clipboard, or `None` when it holds none.
fn read_format(_open: &OpenClipboardGuard, format: u32) -> Option<Vec<u8>> {
    // SAFETY: the clipboard is open on this thread (`_open`); the call takes no pointers.
    unsafe { IsClipboardFormatAvailable(format) }.ok()?;
    // SAFETY: as above; the handle stays the clipboard's, valid while it's open.
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    let block = HGLOBAL(handle.0);
    // SAFETY: a clipboard handle for a memory format is a global block.
    let size = unsafe { GlobalSize(block) };
    // SAFETY: as above; locking it gives its address for `size` bytes.
    let at = unsafe { GlobalLock(block) };
    if at.is_null() {
        return None;
    }
    // SAFETY: `at` addresses the block's `size` readable bytes while it's locked.
    let bytes = unsafe { std::slice::from_raw_parts(at.cast::<u8>(), size) }.to_vec();
    // SAFETY: balances the `GlobalLock` above.
    // "failed" with no error is the zero lock count, the success.
    let _ = unsafe { GlobalUnlock(block) };
    Some(bytes)
}

/// The files on the open clipboard, as `DragQueryFileW` lists them (wide or ANSI `CF_HDROP`).
fn read_files(open: &OpenClipboardGuard) -> Result<Vec<PathBuf>, ClipboardFilesError> {
    let format = u32::from(CF_HDROP.0);
    // SAFETY: the clipboard is open on this thread (`open`); the call takes no pointers.
    if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
        return Ok(Vec::new());
    }
    // SAFETY: as above; the handle stays the clipboard's while it's open.
    let handle = unsafe { GetClipboardData(format) }.map_err(|e| unexpected("GetClipboardData(CF_HDROP)", &e))?;
    // A block too small for its own header, or whose list starts past its end, is another app's
    // bug: `DragQueryFileW` would read past it.
    let Some(header) = read_format(open, format) else {
        return Ok(Vec::new());
    };
    if !shell_formats::dropfiles_header_fits(&header) {
        log::warn!(target: "clipboard", "Ignored a malformed CF_HDROP ({} bytes)", header.len());
        return Ok(Vec::new());
    }
    let drop = HDROP(handle.0);
    // SAFETY: `drop` is the clipboard's live `CF_HDROP`; index `u32::MAX` with no buffer asks
    // for the count.
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: as above; no buffer asks for the length in units, NUL excluded.
        let len = unsafe { DragQueryFileW(drop, i, None) } as usize;
        if len == 0 {
            continue;
        }
        let mut buffer = vec![0u16; len + 1];
        // SAFETY: as above; `buffer` holds the path and its NUL, and its length is the `cch`.
        let copied = unsafe { DragQueryFileW(drop, i, Some(&mut buffer)) } as usize;
        buffer.truncate(copied);
        if !buffer.is_empty() {
            paths.push(PathBuf::from(OsString::from_wide(&buffer)));
        }
    }
    Ok(paths)
}

/// A path's UTF-16 units, as the Shell's formats carry them.
fn units(path: &std::path::Path) -> Vec<u16> {
    path.as_os_str().encode_wide().collect()
}

/// Puts `paths` on the clipboard as files for `intent`, with their text. The caller crossed them
/// to their plain form (`to_native_plain`), the one other apps read.
pub(crate) fn write_file_urls(
    _app: AppHandle,
    paths: Vec<PathBuf>,
    intent: ClipboardIntent,
) -> Result<(), ClipboardFilesError> {
    write_files(&paths, intent)
}

fn write_files(paths: &[PathBuf], intent: ClipboardIntent) -> Result<(), ClipboardFilesError> {
    let units: Vec<Vec<u16>> = paths.iter().map(|p| units(p)).collect();
    let effect = match intent {
        ClipboardIntent::Copy => DROPEFFECT_COPY,
        ClipboardIntent::Cut => DROPEFFECT_MOVE,
    };
    let effect_format = registered_format(CFSTR_PREFERREDDROPEFFECT)?;
    let open = OpenClipboardGuard::acquire()?;
    // SAFETY: the clipboard is open on this thread (`open`); emptying it makes us its writer.
    unsafe { EmptyClipboard() }.map_err(|e| unexpected("EmptyClipboard", &e))?;
    set_format(&open, u32::from(CF_HDROP.0), &shell_formats::dropfiles_block(&units))?;
    set_format(&open, effect_format, &shell_formats::effect_bytes(effect))?;
    set_format(&open, u32::from(CF_UNICODETEXT.0), &shell_formats::paths_text(&units))?;
    log::info!(target: "clipboard", "Wrote {} file(s) to the clipboard ({intent:?})", paths.len());
    Ok(())
}

/// The files on the clipboard, and whether `Preferred DropEffect` marks them cut.
pub(crate) fn read_file_urls(_app: AppHandle) -> Result<ClipboardFiles, ClipboardFilesError> {
    let effect_format = registered_format(CFSTR_PREFERREDDROPEFFECT)?;
    let open = OpenClipboardGuard::acquire()?;
    read_marked_files(&open, effect_format)
}

fn read_marked_files(open: &OpenClipboardGuard, effect_format: u32) -> Result<ClipboardFiles, ClipboardFilesError> {
    let paths = read_files(open)?;
    let marked_cut =
        !paths.is_empty() && read_format(open, effect_format).is_some_and(|bytes| shell_formats::effect_is_cut(&bytes));
    Ok(ClipboardFiles { paths, marked_cut })
}

/// The app path for a path another app put in `CF_HDROP`, or `None` when it names no file for
/// certain: the crossing refuses a device, a drive-relative `C:foo`, a relative path, and a name
/// needing an escape; `shell_formats::names_its_file` refuses a plain path Win32 reads as another.
pub(crate) fn app_text(native: &std::path::Path) -> Option<String> {
    let app = cmdr_fs::app_path::AppPathBuf::from_native_unicode(native).ok()?;
    shell_formats::names_its_file(&units(native), &app).then(|| app.into_string())
}

/// Empties the clipboard when it still holds exactly Cmdr's earlier cut of `paths`.
pub(crate) fn drop_cut_mark(paths: Vec<PathBuf>) -> Result<(), ClipboardFilesError> {
    let effect_format = registered_format(CFSTR_PREFERREDDROPEFFECT)?;
    let open = OpenClipboardGuard::acquire()?;
    if holds_cut(&open, effect_format, &paths)? {
        // SAFETY: the clipboard is open on this thread (`open`).
        unsafe { EmptyClipboard() }.map_err(|e| unexpected("EmptyClipboard", &e))?;
    }
    Ok(())
}

/// Tells the source of a cut that the paste moved everything, on a thread of its own.
pub(crate) fn after_cut_paste(paths: Vec<PathBuf>) {
    let spawned = std::thread::Builder::new()
        .name("clipboard-cut-settle".into())
        .spawn(move || {
            if let Err(error) = settle_cut(&paths) {
                log::warn!(target: "clipboard", "Couldn't tell the cut's source the paste moved it: {error:?}");
            }
        });
    if let Err(error) = spawned {
        log::warn!(target: "clipboard", "Couldn't start the cut-settle thread: {error}");
    }
}

/// The delete-on-paste handshake's target half
/// (learn.microsoft.com/windows/win32/shell/datascenarios § "Handling Delete-on-Paste Operations"):
/// Cmdr's move already took the files (an "optimized move"), so the source gets `Performed
/// DropEffect` = `DROPEFFECT_NONE`, then `Paste Succeeded` = `DROPEFFECT_MOVE`. ❌ Never `Performed
/// DropEffect` = move: with `Paste Succeeded` that asks the source to delete the originals itself.
/// Then, if the cut is still on the clipboard, it goes: a cut pastes once.
fn settle_cut(paths: &[PathBuf]) -> Result<(), ClipboardFilesError> {
    let effect_format = registered_format(CFSTR_PREFERREDDROPEFFECT)?;
    let formats = SettleFormats {
        performed: registered_format(CFSTR_PERFORMEDDROPEFFECT)?,
        succeeded: registered_format(CFSTR_PASTESUCCEEDED)?,
    };
    // The clipboard must still hold this very cut: a person who copied something since gets
    // their new clipboard left alone.
    let sequence = {
        let open = OpenClipboardGuard::acquire()?;
        if !holds_cut(&open, effect_format, paths)? {
            return Ok(());
        }
        // SAFETY: takes no arguments.
        unsafe { GetClipboardSequenceNumber() }
    };
    if let Err(error) = tell_paste_succeeded(formats, sequence) {
        // A source that wrote with the plain API (Cmdr itself, many apps) has no data object
        // to tell; the emptying below still ends the cut.
        log::debug!(target: "clipboard", "The cut's source took no `Paste Succeeded`: {error:?}");
    }
    let open = OpenClipboardGuard::acquire()?;
    // SAFETY: takes no arguments.
    let unchanged = unsafe { GetClipboardSequenceNumber() } == sequence;
    if unchanged && holds_cut(&open, effect_format, paths)? {
        // SAFETY: the clipboard is open on this thread (`open`).
        unsafe { EmptyClipboard() }.map_err(|e| unexpected("EmptyClipboard", &e))?;
    }
    Ok(())
}

/// Whether the open clipboard holds exactly `paths` (any order), marked cut.
fn holds_cut(open: &OpenClipboardGuard, effect_format: u32, paths: &[PathBuf]) -> Result<bool, ClipboardFilesError> {
    let on = read_marked_files(open, effect_format)?;
    let mut held = on.paths;
    let mut wanted = paths.to_vec();
    held.sort();
    wanted.sort();
    Ok(on.marked_cut && held == wanted)
}

/// The two registered formats the handshake sets on the source's data object.
#[derive(Clone, Copy)]
struct SettleFormats {
    performed: u32,
    succeeded: u32,
}

/// `Performed DropEffect` = `DROPEFFECT_NONE`, then `Paste Succeeded` = `DROPEFFECT_MOVE`, on the
/// clipboard's data object, in an OLE apartment this thread enters and leaves around the call.
/// Only when the clipboard is still the one `sequence` names: another cut that took its place
/// between the check and `OleGetClipboard` never hears a paste it didn't get.
fn tell_paste_succeeded(formats: SettleFormats, sequence: u32) -> Result<(), ClipboardFilesError> {
    // SAFETY: enters an STA on this dedicated thread; a success is balanced below on it.
    unsafe { OleInitialize(None) }.map_err(|e| unexpected("OleInitialize", &e))?;
    let told = (|| {
        // SAFETY: OLE is initialized on this thread (above).
        let data = unsafe { OleGetClipboard() }.map_err(|e| unexpected("OleGetClipboard", &e))?;
        // SAFETY: takes no arguments.
        if unsafe { GetClipboardSequenceNumber() } != sequence {
            return Err(ClipboardFilesError::Unexpected {
                detail: "the clipboard changed before the source could be told".into(),
            });
        }
        set_effect(&data, formats.performed, DROPEFFECT_NONE)?;
        set_effect(&data, formats.succeeded, DROPEFFECT_MOVE)
    })();
    // SAFETY: balances the successful `OleInitialize` above, on this same thread, after the
    // data object (dropped inside the closure) is released.
    unsafe { OleUninitialize() };
    told
}

/// Sets the registered `format` to the `DROPEFFECT` value `effect` on `data`.
fn set_effect(data: &IDataObject, format: u32, effect: u32) -> Result<(), ClipboardFilesError> {
    let block = global_with(&shell_formats::effect_bytes(effect))?;
    let format_etc = FORMATETC {
        cfFormat: u16::try_from(format).map_err(|_| ClipboardFilesError::Unexpected {
            detail: format!("registered format {format} past u16"),
        })?,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let medium = STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 { hGlobal: block },
        pUnkForRelease: std::mem::ManuallyDrop::new(None),
    };
    // SAFETY: `format_etc` and `medium` live across the call; `fRelease = true` hands the data
    // object `block` on success.
    if let Err(error) = unsafe { data.SetData(&format_etc, &medium, true) } {
        // SAFETY: refused, so `block` is still ours to free. The refusal is what the caller hears.
        let _ = unsafe { GlobalFree(Some(block)) };
        return Err(unexpected("IDataObject::SetData", &error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clipboard path reads as its app path only when it names its file for certain.
    #[test]
    fn an_incoming_path_reads_as_its_app_path_only_when_it_names_its_file() {
        let read = |p: PathBuf| app_text(&p);
        assert_eq!(
            read(PathBuf::from(r"C:\Users\test_user_1\a.txt")).as_deref(),
            Some("C:/Users/test_user_1/a.txt")
        );
        assert_eq!(
            read(PathBuf::from(r"\\?\C:\x\a.")).as_deref(),
            Some("C:/x/a."),
            "verbatim: exactly its file"
        );
        assert_eq!(read(PathBuf::from(r"C:\x\a.")), None, "Win32 opens `a`");
        assert_eq!(read(PathBuf::from(r"C:\x\CON")), None, "the console device");
        assert_eq!(read(PathBuf::from(r"C:foo")), None, "drive-relative");
        assert_eq!(read(PathBuf::from(r"foo\bar")), None, "relative");
        assert_eq!(read(PathBuf::from(r"\\.\PhysicalDrive0")), None, "a device");
    }

    fn read_back() -> ClipboardFiles {
        let effect_format = registered_format(CFSTR_PREFERREDDROPEFFECT).expect("registered");
        read_marked_files(&OpenClipboardGuard::acquire().expect("open"), effect_format).expect("read")
    }

    /// The real clipboard, end to end, in ONE test (nextest runs tests as parallel processes,
    /// and they'd share the one clipboard): a cut reads back as a cut with the same paths in
    /// order, a copy as no cut, the text as one path per line; a settled cut leaves the
    /// clipboard (Cmdr's own, written with the plain API, has no data object to tell), and a
    /// settle finding a copy instead leaves it alone. Clobbers the clipboard, so it runs on CI
    /// only (`CI` set), never over a developer's own clipboard.
    #[test]
    fn cut_copy_and_settle_round_trip_through_the_real_clipboard() {
        if std::env::var_os("CI").is_none() {
            return;
        }
        let paths = vec![
            PathBuf::from(r"C:\Users\test_user_1\a.txt"),
            PathBuf::from(r"\\testnas.local\share\folder with spaces\b 🦀.txt"),
        ];

        write_files(&paths, ClipboardIntent::Cut).expect("cut");
        let read = read_back();
        assert_eq!(read.paths, paths);
        assert!(read.marked_cut);

        write_files(&paths, ClipboardIntent::Copy).expect("copy");
        let read = read_back();
        assert_eq!(read.paths, paths);
        assert!(!read.marked_cut);
        let text = read_format(
            &OpenClipboardGuard::acquire().expect("open"),
            u32::from(CF_UNICODETEXT.0),
        );
        let expected = shell_formats::paths_text(&paths.iter().map(|p| units(p)).collect::<Vec<_>>());
        assert_eq!(text.expect("text"), expected);

        settle_cut(&paths).expect("settled");
        assert_eq!(read_back().paths, paths, "a copy stays");

        write_files(&paths, ClipboardIntent::Cut).expect("cut");
        settle_cut(&paths).expect("settled");
        assert!(read_back().paths.is_empty(), "the cut went");
    }
}
