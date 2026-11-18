//! Windows has no uid or gid, so the process asks as `nobody` (65534), the anonymous identity
//! an `AUTH_SYS` server maps an unknown caller to anyway, with no other groups; a person who
//! needs another identity sets one on the place (the add form's "User ID" and "Group ID"). The
//! machine name is the DNS host name, as `gethostname` gives it on a Unix.

use windows_sys::Win32::System::SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW};

pub(crate) fn machine_name() -> String {
    let mut size: u32 = 0;
    // SAFETY: a null buffer with a zero size asks for the size: the call writes nothing through
    // the pointer, sets `size` to the units needed (terminator included), and fails with
    // `ERROR_MORE_DATA`, the expected answer here, so its result is ignored.
    unsafe { GetComputerNameExW(ComputerNameDnsHostname, std::ptr::null_mut(), &raw mut size) };
    let Ok(capacity) = usize::try_from(size) else {
        return String::new();
    };
    if capacity == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; capacity];
    // SAFETY: `buf` is valid for `size` UTF-16 units for the whole call, and the call writes at
    // most that many, setting `size` to the units written without the terminator.
    let ok = unsafe { GetComputerNameExW(ComputerNameDnsHostname, buf.as_mut_ptr(), &raw mut size) };
    if ok == 0 {
        return String::new();
    }
    let written = usize::try_from(size).unwrap_or(0).min(buf.len());
    String::from_utf16_lossy(&buf[..written])
}

pub(crate) fn process_ids() -> (u32, u32) {
    (65_534, 65_534)
}

pub(crate) fn process_groups() -> Vec<u32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_machine_name_is_this_computers_host_name() {
        let name = super::machine_name().to_ascii_uppercase();
        let netbios = std::env::var("COMPUTERNAME")
            .expect("COMPUTERNAME is set on Windows")
            .to_ascii_uppercase();
        // The NetBIOS name is the DNS host name cut to 15 characters, uppercased.
        assert!(!name.is_empty(), "every Windows install has a host name");
        assert!(name.starts_with(&netbios), "{name:?} should start with {netbios:?}");
    }
}
