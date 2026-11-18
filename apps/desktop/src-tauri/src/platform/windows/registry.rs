//! Registry reads for Windows arms (the OS version, managed policy), always through the 64-bit
//! view, plus the scratch keys their tests write. Errors are raw Win32 codes: each caller decides
//! what a missing or unreadable key means for its own answer.

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, KEY_READ, KEY_WOW64_64KEY, REG_DWORD, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegEnumKeyExW, RegEnumValueW,
    RegOpenKeyExW,
};

pub(crate) use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

/// One value as the registry holds it: its type (`REG_SZ`, `REG_DWORD`, …) and its raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawValue {
    pub(crate) kind: u32,
    pub(crate) data: Vec<u8>,
}

/// An open key, closed on drop.
pub(crate) struct Key(HKEY);

impl Key {
    /// Opens `subkey` under `root` for reading, or `Ok(None)` when it doesn't exist.
    pub(crate) fn open(root: HKEY, subkey: &str) -> Result<Option<Self>, u32> {
        Self::open_with(root, subkey, KEY_READ)
    }

    /// Opens `subkey` with `access` (plus the 64-bit view), or `Ok(None)` when it doesn't exist.
    pub(crate) fn open_with(root: HKEY, subkey: &str, access: u32) -> Result<Option<Self>, u32> {
        let subkey = wide(subkey);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `subkey` is a NUL-terminated UTF-16 string alive for the call, and `key` is a
        // stack local the call writes the opened handle into; we own that handle from here and
        // close it in `Drop`.
        let status = unsafe { RegOpenKeyExW(root, subkey.as_ptr(), 0, access | KEY_WOW64_64KEY, &raw mut key) };
        match status {
            ERROR_SUCCESS => Ok(Some(Self(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            code => Err(code),
        }
    }

    /// The raw handle, for calls this module doesn't wrap (change notifications).
    pub(crate) fn handle(&self) -> HKEY {
        self.0
    }

    /// Every value under this key, by name, in the registry's own order.
    pub(crate) fn values(&self) -> Result<Vec<(String, RawValue)>, u32> {
        let mut out = Vec::new();
        // Generous first guesses; `ERROR_MORE_DATA` grows them and retries the same index.
        let mut name = vec![0u16; 256];
        let mut data = vec![0u8; 1024];
        let mut index = 0;
        loop {
            let mut name_len = u32::try_from(name.len()).unwrap_or(u32::MAX);
            let mut data_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
            let mut kind: REG_VALUE_TYPE = 0;
            // SAFETY: `name` holds `name_len` UTF-16 units and `data` holds `data_len` bytes; the
            // call writes at most those, updating both lengths to what it wrote (the name's
            // without its terminator). The reserved pointer is documented as null.
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &raw mut name_len,
                    std::ptr::null(),
                    &raw mut kind,
                    data.as_mut_ptr(),
                    &raw mut data_len,
                )
            };
            match status {
                ERROR_SUCCESS => {
                    let name_units = name.get(..name_len as usize).unwrap_or_default();
                    let bytes = data.get(..data_len as usize).unwrap_or_default();
                    out.push((
                        String::from_utf16_lossy(name_units),
                        RawValue {
                            kind,
                            data: bytes.to_vec(),
                        },
                    ));
                    index += 1;
                }
                ERROR_MORE_DATA => {
                    name.resize(name.len() * 2, 0);
                    data.resize((data.len() * 2).max(data_len as usize), 0);
                    // A value name can't pass 16,383 characters, nor data 1 MB in practice; stop
                    // a runaway rather than allocate without bound.
                    if name.len() > 65_536 || data.len() > 64 * 1024 * 1024 {
                        return Err(ERROR_MORE_DATA);
                    }
                }
                ERROR_NO_MORE_ITEMS => return Ok(out),
                code => return Err(code),
            }
        }
    }

    /// The names of this key's direct subkeys.
    pub(crate) fn subkey_names(&self) -> Result<Vec<String>, u32> {
        let mut out = Vec::new();
        // A key name can't pass 255 characters.
        let mut name = [0u16; 256];
        let mut index = 0;
        loop {
            let mut name_len = u32::try_from(name.len()).unwrap_or(u32::MAX);
            // SAFETY: `name` holds `name_len` UTF-16 units; the call writes at most that many and
            // sets `name_len` to the name's length without its terminator. Every other pointer is
            // an optional out-parameter, documented as nullable.
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &raw mut name_len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            match status {
                ERROR_SUCCESS => {
                    out.push(String::from_utf16_lossy(
                        name.get(..name_len as usize).unwrap_or_default(),
                    ));
                    index += 1;
                }
                ERROR_NO_MORE_ITEMS => return Ok(out),
                code => return Err(code),
            }
        }
    }

    /// One value by name, or `None` when it's missing or unreadable.
    pub(crate) fn value(&self, name: &str) -> Option<RawValue> {
        self.values()
            .ok()?
            .into_iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a handle `RegOpenKeyExW` (or `RegCreateKeyExW`) gave us, closed
        // exactly once, here.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

/// A `REG_SZ` value, trimmed of its terminator; `None` when missing, of another type, or empty.
pub(crate) fn read_string(root: HKEY, subkey: &str, name: &str) -> Option<String> {
    let value = Key::open(root, subkey).ok()??.value(name)?;
    if value.kind != REG_SZ || !value.data.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = value
        .data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let text = String::from_utf16(&units).ok()?;
    let text = text.trim_end_matches('\0').trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// A `REG_DWORD` value; `None` when missing or of another type.
pub(crate) fn read_dword(root: HKEY, subkey: &str, name: &str) -> Option<u32> {
    let value = Key::open(root, subkey).ok()??.value(name)?;
    if value.kind != REG_DWORD {
        return None;
    }
    Some(u32::from_le_bytes(value.data.get(..4)?.try_into().ok()?))
}

/// `text` as a NUL-terminated UTF-16 string.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A key a test creates under `HKCU\Software\CmdrTest` and deletes, with everything under it, on
/// drop. ❌ Never anywhere near `SOFTWARE\Policies`: tests must not touch real policy.
#[cfg(test)]
pub(crate) struct ScratchKey {
    path: String,
    key: Key,
}

#[cfg(test)]
impl ScratchKey {
    pub(crate) fn new(tag: &str) -> Self {
        let path = format!("Software\\CmdrTest\\{tag}-{}", std::process::id());
        let key = create(HKEY_CURRENT_USER, &path);
        Self { path, key }
    }

    /// The key's path under `HKEY_CURRENT_USER`.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// Writes `data` as a value of `kind` under `subkey` (empty for this key itself).
    pub(crate) fn set(&self, subkey: &str, name: &str, kind: u32, data: &[u8]) {
        let target = if subkey.is_empty() {
            None
        } else {
            Some(create(self.key.0, subkey))
        };
        set_value(target.as_ref().unwrap_or(&self.key), name, kind, data);
    }

    /// Writes a `REG_SZ`.
    pub(crate) fn set_string(&self, subkey: &str, name: &str, text: &str) {
        let bytes: Vec<u8> = wide(text).iter().flat_map(|unit| unit.to_le_bytes()).collect();
        self.set(subkey, name, REG_SZ, &bytes);
    }

    /// Writes a `REG_DWORD`.
    pub(crate) fn set_dword(&self, subkey: &str, name: &str, value: u32) {
        self.set(subkey, name, REG_DWORD, &value.to_le_bytes());
    }
}

#[cfg(test)]
impl Drop for ScratchKey {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Registry::RegDeleteTreeW;

        let path = wide(&self.path);
        // SAFETY: `path` is NUL-terminated UTF-16 naming our own scratch key under HKCU.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
        if status != ERROR_SUCCESS {
            log::warn!("Couldn't delete the scratch key {}: Win32 error {status}", self.path);
        }
    }
}

/// Writes a `REG_DWORD` at `path` under `HKEY_CURRENT_USER`, creating the key. For a test whose
/// writer runs on another thread than its `ScratchKey` (a key handle isn't `Send`). Test-only.
#[cfg(test)]
pub(crate) fn set_dword_at(path: &str, name: &str, value: u32) {
    set_value(&create(HKEY_CURRENT_USER, path), name, REG_DWORD, &value.to_le_bytes());
}

#[cfg(test)]
fn set_value(key: &Key, name: &str, kind: u32, data: &[u8]) {
    use windows_sys::Win32::System::Registry::RegSetValueExW;

    let name = wide(name);
    // SAFETY: `key` is an open key we hold for the call, `name` is NUL-terminated UTF-16, and
    // `data` is valid for the `data.len()` bytes we pass.
    let status = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            kind,
            data.as_ptr(),
            u32::try_from(data.len()).expect("test values are small"),
        )
    };
    assert_eq!(status, ERROR_SUCCESS, "RegSetValueExW");
}

/// Creates (or opens) `subkey` under `parent` for writing. Test-only.
#[cfg(test)]
fn create(parent: HKEY, subkey: &str) -> Key {
    use windows_sys::Win32::System::Registry::{KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, RegCreateKeyExW};

    let subkey = wide(subkey);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: `subkey` is NUL-terminated UTF-16 alive for the call; `key` is a stack local the
    // call writes the handle into, which `Key` then owns. Class, security attributes, and the
    // disposition are documented as optional.
    let status = unsafe {
        RegCreateKeyExW(
            parent,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            std::ptr::null(),
            &raw mut key,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(status, ERROR_SUCCESS, "RegCreateKeyExW");
    Key(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_key_reads_back_and_goes_away() {
        let path;
        {
            let scratch = ScratchKey::new("registry-roundtrip");
            path = scratch.path().to_string();
            scratch.set_string("", "Name", "test value");
            scratch.set_dword("", "Count", 42);
            scratch.set_string("Child", "1", "first");

            assert_eq!(
                read_string(HKEY_CURRENT_USER, &path, "Name").as_deref(),
                Some("test value")
            );
            assert_eq!(read_dword(HKEY_CURRENT_USER, &path, "Count"), Some(42));
            assert_eq!(read_dword(HKEY_CURRENT_USER, &path, "Name"), None, "wrong type");
            let key = Key::open(HKEY_CURRENT_USER, &path).expect("open").expect("exists");
            assert_eq!(key.subkey_names().expect("subkeys"), ["Child"]);
            assert_eq!(key.values().expect("values").len(), 2);
        }
        assert!(Key::open(HKEY_CURRENT_USER, &path).expect("open").is_none());
    }

    #[test]
    fn a_missing_key_opens_as_none() {
        assert!(
            Key::open(HKEY_CURRENT_USER, "Software\\CmdrTest\\never-created")
                .expect("open")
                .is_none()
        );
    }
}
