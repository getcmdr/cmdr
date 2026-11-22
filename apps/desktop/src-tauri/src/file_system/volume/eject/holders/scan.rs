//! The kernel walk behind the holder scan: which processes hold a volume, and what to
//! call each one.
//!
//! macOS answers through `libproc`'s `proc_listpidspath`, declared here as two
//! `extern "C"` lines. ❌ No `libproc` crate for two functions. It sees SAME-UID
//! processes only (`CHECK_SAME_USER`, `xnu/bsd/kern/proc_info.c:2196-2211`), catches
//! open files, cwd, and per-thread cwd, and misses a holder that only memory-mapped the
//! file. Root-owned holders stay invisible, which is why a refusal can still name
//! nobody (`volume/DETAILS.md` § "Eject").
//!
//! Linux and Windows answer `Unreadable`, which reads as "couldn't tell", ❌ never as
//! "nobody is holding it". (Windows' walk would be the Restart Manager, with the drives task.)

/// Linux and Windows have no walk to run, so every refusal there is "couldn't tell". It still goes
/// through `scan_path_with`, so the answer has ONE shape whatever the platform, and the
/// `Unreadable` comes from a walk that couldn't run rather than from a second code path.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) fn scan_path(path: &std::path::Path) -> super::PathScan {
    super::scan_path_with(|| super::root_device(path), || None, |_| None)
}

#[cfg(all(test, target_os = "macos"))]
pub(super) use macos::{FILE_FLAGS, pids_holding};
#[cfg(target_os = "macos")]
pub(super) use macos::{executable_path, scan_path};

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CString, OsString, c_char, c_int, c_void};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use super::super::{HolderKind, PathScan, VolumeHolder, root_device, scan_path_with};

    /// `PROC_ALL_PIDS` (`libproc.h:52`): every process, rather than one user's or one
    /// process group's.
    const PROC_ALL_PIDS: u32 = 1;

    /// `PROC_LISTPIDSPATH_PATH_IS_VOLUME` (`sys/proc_info.h:51`): match anything open on
    /// the path's whole VOLUME, not just that one path.
    const PATH_IS_VOLUME: u32 = 1;

    /// `PROC_LISTPIDSPATH_EXCLUDE_EVTONLY` (`sys/proc_info.h:51`): skip descriptors
    /// opened only to watch for events. They don't keep a volume busy, so counting them
    /// would name processes an unmount never minded.
    const EXCLUDE_EVTONLY: u32 = 2;

    /// What a scan of a whole drive asks for.
    const VOLUME_FLAGS: u32 = PATH_IS_VOLUME | EXCLUDE_EVTONLY;

    /// What a scan of one FILE asks for: the same walk without the volume widening, so a
    /// test can hold a temp file open and see itself named, with no volume in sight.
    #[cfg(test)]
    pub(in crate::file_system::volume::eject::holders) const FILE_FLAGS: u32 = EXCLUDE_EVTONLY;

    /// `PROC_PIDPATHINFO_MAXSIZE` (`sys/proc_info.h`): four times `MAXPATHLEN`.
    const PID_PATH_MAX: usize = 4 * 1024;

    /// How many pids the first walk makes room for. macOS's `kern.maxproc` defaults well
    /// under this, and a result that fills the buffer exactly is retried with twice the
    /// room, so a busy machine can't silently truncate the holder list.
    const FIRST_GUESS: usize = 4096;

    /// How many times the walk may double its buffer before it gives up.
    const MAX_DOUBLINGS: usize = 4;

    unsafe extern "C" {
        /// `libproc.h:85`. Public, same-uid only, and it `stat`s `path` before it walks
        /// the process list, which is the call that can hang on a wedged mount.
        fn proc_listpidspath(
            r#type: u32,
            typeinfo: u32,
            path: *const c_char,
            pathflags: u32,
            buffer: *mut c_void,
            buffersize: c_int,
        ) -> c_int;

        /// `libproc.h`. The process's executable path; 0 once the process is gone
        /// (ESRCH) or isn't ours to see.
        fn proc_pidpath(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
    }

    /// Who holds `path`, its own mount root read before and after so a mount that changed
    /// under the scan discards the answer.
    pub(in crate::file_system::volume::eject::holders) fn scan_path(path: &Path) -> PathScan {
        scan_path_with(|| root_device(path), || pids_holding(path, VOLUME_FLAGS), name_of)
    }

    /// Every same-uid process holding `path`, `None` when the walk itself couldn't run.
    ///
    /// `flags` is a parameter so a test can ask about one FILE instead of a whole drive,
    /// which it can hold open without a real volume anywhere.
    pub(in crate::file_system::volume::eject::holders) fn pids_holding(path: &Path, flags: u32) -> Option<Vec<u32>> {
        let path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut room = FIRST_GUESS;
        for _ in 0..MAX_DOUBLINGS {
            let mut pids: Vec<c_int> = vec![0; room];
            let size = c_int::try_from(size_of_val(pids.as_slice())).ok()?;
            // SAFETY: `path` is a live NUL-terminated C string that outlives the call, and
            // `pids` is a live, initialized buffer of exactly `size` bytes, which the call
            // only writes `pid_t`s into. Nothing unwinds across the boundary.
            let written = unsafe {
                proc_listpidspath(
                    PROC_ALL_PIDS,
                    0,
                    path.as_ptr(),
                    flags,
                    pids.as_mut_ptr().cast::<c_void>(),
                    size,
                )
            };
            if written < 0 {
                return None;
            }
            let found = usize::try_from(written).ok()? / size_of::<c_int>();
            // A result that filled the buffer exactly may have been cut short, and a holder
            // list missing the holder is worse than no list at all.
            if found >= room {
                room *= 2;
                continue;
            }
            pids.truncate(found);
            return Some(pids.into_iter().filter_map(|pid| u32::try_from(pid).ok()).collect());
        }
        log::warn!(target: "eject", "More processes hold the drive than the holder scan made room for");
        None
    }

    /// What to call the process behind `pid`, `None` once it's gone.
    ///
    /// The executable's own name, which the facts stage replaces with an app's or an
    /// image's once it knows what kind of holder this is (`holders::facts`). ❗ The name
    /// is read HERE, during the walk, so a holder the facts budget never reaches is still
    /// named.
    fn name_of(pid: u32) -> Option<VolumeHolder> {
        let executable = executable_path(pid)?;
        let name = executable.file_name()?.to_string_lossy().into_owned();
        Some(VolumeHolder {
            pid,
            name,
            bundle_id: None,
            kind: HolderKind::Unclassified,
        })
    }

    /// The executable behind `pid`, `None` when the process has gone or isn't visible.
    pub(in crate::file_system::volume::eject::holders) fn executable_path(pid: u32) -> Option<PathBuf> {
        let pid = c_int::try_from(pid).ok()?;
        let mut buffer = vec![0u8; PID_PATH_MAX];
        let size = u32::try_from(buffer.len()).ok()?;
        // SAFETY: `buffer` is a live, initialized buffer of exactly `size` bytes, which the
        // call fills with at most that many bytes of path. Nothing unwinds across the
        // boundary.
        let written = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast::<c_void>(), size) };
        if written <= 0 {
            return None;
        }
        buffer.truncate(usize::try_from(written).ok()?);
        Some(PathBuf::from(OsString::from_vec(buffer)))
    }
}
