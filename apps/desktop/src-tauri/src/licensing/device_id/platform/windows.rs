//! Windows: `MachineGuid` under `HKLM\SOFTWARE\Microsoft\Cryptography`, which Setup writes once
//! per install. The counterpart of Linux's `/etc/machine-id`, with the same trade: a reinstall
//! counts as a new device, and a cloned image that skipped `sysprep` shares its source's id.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW};

/// A GUID in braces-free text is 36 characters; this leaves room for any spelling.
const MAX_CHARS: usize = 128;

pub(crate) fn read_platform_uuid() -> Option<String> {
    let id = read_machine_guid();
    if id.is_none() {
        log::warn!("No usable MachineGuid found; license checks will carry no device id");
    }
    id
}

/// The raw `MachineGuid` value, trimmed; `None` when it's missing, empty, or unreadable.
///
/// Read from the 64-bit registry view (`RRF_SUBKEY_WOW6464KEY`): a 32-bit process would
/// otherwise land in `WOW6432Node`, where the value doesn't exist.
fn read_machine_guid() -> Option<String> {
    let subkey = wide("SOFTWARE\\Microsoft\\Cryptography");
    let value = wide("MachineGuid");
    let mut buf = [0u16; MAX_CHARS];
    let mut size = u32::try_from(size_of_val(&buf)).ok()?;
    // SAFETY: `subkey` and `value` are NUL-terminated UTF-16 strings alive for the call; `buf`
    // is valid for `size` bytes, and `RegGetValueW` writes at most that many, updating `size`
    // to the bytes written (terminator included). `RRF_RT_REG_SZ` refuses any other value type,
    // and the null type pointer is documented as allowed.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    if status != ERROR_SUCCESS {
        log::warn!("Reading MachineGuid answered Win32 error {status}");
        return None;
    }
    let units = usize::try_from(size).ok()? / 2;
    let text = String::from_utf16_lossy(buf.get(..units)?);
    let id = text.trim_end_matches('\0').trim();
    (!id.is_empty()).then(|| id.to_owned())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_machines_guid() {
        let guid = read_machine_guid().expect("Windows Setup writes MachineGuid on every install");
        assert_eq!(guid.len(), 36, "a GUID without braces, got {guid:?}");
        assert!(
            guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "hex digits and dashes only, got {guid:?}"
        );
    }

    #[test]
    fn the_device_id_is_the_guid_hashed_like_every_other_platform() {
        let guid = read_machine_guid().expect("MachineGuid");
        assert_eq!(
            super::super::super::get_device_id(),
            Some(super::super::super::hash_platform_uuid(&guid))
        );
    }
}
