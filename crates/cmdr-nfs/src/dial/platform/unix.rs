//! Both Unixes: `gethostname`, `getuid`, `getgid`, and `getgroups`.

pub(crate) fn machine_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is valid for `buf.len()` bytes for the whole call, and
    // `gethostname` writes at most that many.
    let status = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if status != 0 {
        return String::new();
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

pub(crate) fn process_ids() -> (u32, u32) {
    // SAFETY: `getuid` and `getgid` take nothing, can't fail, and touch no
    // memory of ours.
    unsafe { (libc::getuid(), libc::getgid()) }
}

pub(crate) fn process_groups() -> Vec<u32> {
    // SAFETY: a zero count asks for the size and writes nothing through the null pointer.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    let Ok(capacity) = usize::try_from(count) else {
        return Vec::new();
    };
    let mut groups: Vec<libc::gid_t> = vec![0; capacity];
    // SAFETY: `groups` is valid for `count` entries for the whole call, and `getgroups`
    // writes at most that many.
    let written = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
    let Ok(written) = usize::try_from(written) else {
        return Vec::new();
    };
    groups.truncate(written);
    groups
}
