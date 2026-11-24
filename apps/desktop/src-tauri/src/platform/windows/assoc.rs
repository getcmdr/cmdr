//! The apps Windows offers for a file type, and handing files to one of them: the Shell's own
//! "Open with" machinery (`SHAssocEnumHandlers`, `IAssocHandler`), which "Open with" in the file
//! menu and F4's editor list both read.
//!
//! - [`apps_for`] lists the handlers File Explorer's "Open with" submenu lists (the recommended
//!   ones), each with its name and icon, the type's default first.
//! - [`invoke`] hands items to one handler by its id (`IAssocHandler::GetName`), through a data
//!   object built from the items' ID lists: the app gets the exact items, and ❌ no command line
//!   is ever built from a path.
//! - [`choose_another_app`] shows Windows' own "How do you want to open this file?" dialog
//!   (`SHOpenWithDialog`), which opens the file in what the user picks.
//!
//! Every call here wants a COM apartment (`apartment.rs`) on an 8 MB thread
//! (`shell::SHELL_THREAD_STACK`): a handler's icon and its launch run third-party code.

use image::RgbaImage;
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
use windows::Win32::UI::Shell::{
    ASSOC_FILTER, ASSOC_FILTER_NONE, ASSOC_FILTER_RECOMMENDED, ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR,
    ASSOCSTR_EXECUTABLE, ASSOCSTR_FRIENDLYAPPNAME, AssocQueryStringW, BHID_DataObject, IAssocHandler, IShellItemArray,
    OAIF_ALLOW_REGISTRATION, OAIF_EXEC, OAIF_REGISTER_EXT, OPENASINFO, SHAssocEnumHandlers,
    SHCreateShellItemArrayFromIDLists, SHOpenWithDialog,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::shell::{ItemIdList, ShellRefusal};
use crate::platform::windows_assoc_rules::{assoc_extension, default_index};

/// One app Windows offers for a type.
#[derive(Debug, Clone)]
pub(crate) struct AssocApp {
    /// `IAssocHandler::GetName`: the program's path for a classic app, the package's app id for
    /// a packaged one. Stable across calls, so it's what a menu row and a stored setting name.
    pub(crate) id: String,
    /// `IAssocHandler::GetUIName`, the name File Explorer shows.
    pub(crate) name: String,
    /// Its icon, when one was asked for and something draws from its icon location.
    pub(crate) icon: Option<RgbaImage>,
    /// Whether this is the app the type opens in by default.
    pub(crate) is_default: bool,
}

/// One live handler with the two strings every caller keys it by.
struct Handler {
    handler: IAssocHandler,
    id: String,
    name: String,
}

/// A string the Shell allocated with the COM allocator, taken and freed.
fn take(text: PWSTR) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: a non-null string the Shell just returned, NUL-terminated, owned by this call.
    let owned = unsafe { text.to_string() }.ok();
    // SAFETY: allocated by the Shell with the COM allocator, freed once, here.
    unsafe { CoTaskMemFree(Some(text.0.cast_const().cast())) };
    owned
}

/// Every handler for `.ext` the filter keeps, in the Shell's order, each id once. An extension
/// with no handlers (or none at all) answers an empty list.
fn handlers(extension: &str, filter: ASSOC_FILTER) -> Vec<Handler> {
    let Some(key) = assoc_extension(extension) else {
        return Vec::new();
    };
    // SAFETY: the extension is a live NUL-terminated `HSTRING` for the call.
    let Ok(enumerator) = (unsafe { SHAssocEnumHandlers(&HSTRING::from(key), filter) }) else {
        return Vec::new();
    };
    let mut found: Vec<Handler> = Vec::new();
    loop {
        let mut slot = [None];
        let mut fetched = 0u32;
        // SAFETY: one slot, and `fetched` receives how many the call filled.
        let next = unsafe { enumerator.Next(&mut slot, Some(&raw mut fetched)) };
        if next.is_err() || fetched == 0 {
            break;
        }
        let [Some(handler)] = slot else { break };
        // SAFETY: `handler` is a live interface the enumerator just handed over.
        let id = take(unsafe { handler.GetName() }.unwrap_or(PWSTR::null()));
        // SAFETY: as above.
        let name = take(unsafe { handler.GetUIName() }.unwrap_or(PWSTR::null()));
        let (Some(id), Some(name)) = (id, name) else { continue };
        if found.iter().any(|known| known.id.eq_ignore_ascii_case(&id)) {
            continue;
        }
        found.push(Handler { handler, id, name });
    }
    found
}

/// The apps File Explorer's "Open with" offers for files ending in `.extension` (no dot), the
/// default first and marked. With `icon_size`, each carries its icon at that size.
pub(crate) fn apps_for(extension: &str, icon_size: Option<u32>) -> Vec<AssocApp> {
    let found = handlers(extension, ASSOC_FILTER_RECOMMENDED);
    let default = assoc_extension(extension).and_then(|key| {
        let executable = query(&key, ASSOCSTR_EXECUTABLE);
        let friendly = query(&key, ASSOCSTR_FRIENDLYAPPNAME);
        let pairs: Vec<(&str, &str)> = found.iter().map(|h| (h.id.as_str(), h.name.as_str())).collect();
        default_index(&pairs, executable.as_deref(), friendly.as_deref())
    });
    let mut apps: Vec<AssocApp> = found
        .iter()
        .enumerate()
        .map(|(index, handler)| AssocApp {
            id: handler.id.clone(),
            name: handler.name.clone(),
            icon: icon_size.and_then(|size| icon_of(&handler.handler, size)),
            is_default: Some(index) == default,
        })
        .collect();
    if let Some(index) = default {
        let first = apps.remove(index);
        apps.insert(0, first);
    }
    apps
}

/// A handler's own icon at `size`.
fn icon_of(handler: &IAssocHandler, size: u32) -> Option<RgbaImage> {
    let mut location = PWSTR::null();
    let mut index = 0i32;
    // SAFETY: both out-pointers are live locals; the path comes back COM-allocated (`take`).
    unsafe { handler.GetIconLocation(&raw mut location, &raw mut index) }.ok()?;
    let location = take(location)?;
    crate::icons::platform::app_icon_at(&location, index, size)
}

/// `AssocQueryStringW` for the type's default verb, or `None` when the Shell has no answer.
fn query(extension_key: &str, what: ASSOCSTR) -> Option<String> {
    let mut buffer = vec![0u16; 1024];
    let mut len = u32::try_from(buffer.len()).ok()?;
    // SAFETY: the type is a live `HSTRING`; no verb (the default); `buffer` holds `len` units,
    // which the call updates to what it wrote, NUL included.
    let answered = unsafe {
        AssocQueryStringW(
            ASSOCF_INIT_IGNOREUNKNOWN,
            what,
            &HSTRING::from(extension_key),
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &raw mut len,
        )
    };
    if answered.is_err() {
        return None;
    }
    let end = buffer.iter().position(|&unit| unit == 0)?;
    String::from_utf16(buffer.get(..end)?)
        .ok()
        .filter(|text| !text.is_empty())
}

/// Hands `items` (NUL-terminated plain paths, at least one) to the handler for `.extension`
/// whose id is `handler_id`, as File Explorer's "Open with" does: one data object over the items'
/// pinned ID lists.
///
/// # Errors
///
/// [`ShellRefusal::NoHandler`] when no handler by that id offers the type any more; otherwise
/// the Shell's refusal (an item gone, a launch refused).
pub(crate) fn invoke(extension: &str, handler_id: &str, items: &[Vec<u16>]) -> Result<(), ShellRefusal> {
    // Every handler, not only the recommended ones: a stored editor may have dropped off the
    // recommended list and still open the file.
    let handler = handlers(extension, ASSOC_FILTER_NONE)
        .into_iter()
        .find(|handler| handler.id.eq_ignore_ascii_case(handler_id))
        .ok_or(ShellRefusal::NoHandler)?;
    let data = data_object(items)?;
    // SAFETY: `handler` and `data` are live interfaces for the call.
    unsafe { handler.handler.Invoke(&data) }.map_err(|e| ShellRefusal::of_hresult(e.code().0))
}

/// A data object over `items`, each pinned as an ID list first (`ItemIdList`).
fn data_object(items: &[Vec<u16>]) -> Result<IDataObject, ShellRefusal> {
    let lists = items
        .iter()
        .map(|item| ItemIdList::parse(item))
        .collect::<Result<Vec<_>, _>>()?;
    let pointers: Vec<*const windows::Win32::UI::Shell::Common::ITEMIDLIST> =
        lists.iter().map(|list| list.as_ptr().cast()).collect();
    // SAFETY: every pointer is a live, fully qualified ID list owned by `lists`, which outlives
    // the call; the array copies what it keeps.
    let array: IShellItemArray =
        unsafe { SHCreateShellItemArrayFromIDLists(&pointers) }.map_err(|e| ShellRefusal::of_hresult(e.code().0))?;
    // SAFETY: `array` is live; no bind context; `BHID_DataObject` asks for its data object.
    unsafe { array.BindToHandler(None, &BHID_DataObject) }.map_err(|e| ShellRefusal::of_hresult(e.code().0))
}

/// Windows' "How do you want to open this file?" dialog for `item` (NUL-terminated plain path),
/// owned by the window `parent` (its raw `HWND`, or 0 for none). The dialog opens the file in
/// the app picked and, with "Always use this app" ticked, makes it the type's default, as File
/// Explorer's "Choose another app" does. Returns once the dialog closes.
///
/// # Errors
///
/// [`ShellRefusal`]: `ERROR_CANCELLED` when the user closed the dialog, else the Shell's answer.
pub(crate) fn choose_another_app(parent: isize, item: &[u16]) -> Result<(), ShellRefusal> {
    use windows::Win32::Foundation::HWND;

    let info = OPENASINFO {
        pcszFile: PCWSTR(item.as_ptr()),
        pcszClass: PCWSTR::null(),
        oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_REGISTER_EXT | OAIF_EXEC,
    };
    let owner = (parent != 0).then_some(HWND(parent as *mut core::ffi::c_void));
    // SAFETY: `item` is NUL-terminated and outlives the modal call, as does `info`; `owner` is
    // a window handle or none.
    unsafe { SHOpenWithDialog(owner, &raw const info) }.map_err(|e| ShellRefusal::of_hresult(e.code().0))
}
