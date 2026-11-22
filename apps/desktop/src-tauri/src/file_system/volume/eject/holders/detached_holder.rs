//! A test holder that runs a binary FROM the drive it holds, and doesn't descend from
//! the test process.
//!
//! Both halves are load-bearing, and neither is obvious:
//!
//! - **Not a descendant.** `facts`'s first rule answers `Cmdr` for anything this process
//!   started, and it beats every rule below it. A plain `Command::spawn` child can
//!   therefore never exercise rule 5, so this one is started through a shell that exits
//!   and leaves it to launchd (verified on macOS 27.0, 2026-09-16: `PPID` 1).
//! - **Running from the drive.** macOS SIGKILLs a plain copy of an arm64e platform binary
//!   (`/bin/sleep` copied and run: exit 137, macOS 27.0, 2026-09-16), so the copy is
//!   re-signed ad-hoc, which is enough to run it and leaves it un-platform.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// A process on the drive, holding a file on the drive, owned by nobody.
#[derive(Debug)]
pub(super) struct DetachedHolder {
    pid: u32,
    executable: PathBuf,
}

impl DetachedHolder {
    /// Copies `/bin/sleep` into `directory`, re-signs it, and starts it detached with
    /// `held` open as its stdin.
    pub(super) fn running_from(directory: &Path, held: &Path) -> Self {
        let executable = directory.join("cmdr-holder");
        std::fs::copy("/bin/sleep", &executable).expect("copy a binary onto the drive");
        let signed = Command::new("codesign")
            .args(["-f", "-s", "-"])
            .arg(&executable)
            .output()
            .expect("run codesign");
        assert!(
            signed.status.success(),
            "codesign wouldn't sign the copy: {}",
            String::from_utf8_lossy(&signed.stderr)
        );

        // `$!` is the background job's pid, and the shell exits at once, so launchd
        // adopts it. Its stdout goes to `/dev/null`, or `output()` below would wait for
        // the holder itself to end rather than for the shell.
        let started = Command::new("/bin/sh")
            .args(["-c", r#""$1" 600 < "$2" > /dev/null 2>&1 & echo $!"#, "sh"])
            .arg(&executable)
            .arg(held)
            .output()
            .expect("start the detached holder");
        assert!(
            started.status.success(),
            "the shell wouldn't start the holder: {}",
            String::from_utf8_lossy(&started.stderr)
        );
        let pid: u32 = String::from_utf8_lossy(&started.stdout)
            .trim()
            .parse()
            .expect("the shell prints the holder's pid");

        // It's forked by the time the shell printed its pid, but not necessarily
        // `exec`'d, and the rules read the executable.
        //
        // ❗ Compared against the RESOLVED path: `proc_pidpath` answers through the
        // firmlink (`/private/var/folders/…` for a `$TMPDIR` scratch dir), so comparing
        // the path we spawned would wait forever.
        let running = std::fs::canonicalize(&executable).expect("the copy resolves");
        crate::test_support::wait_until(Duration::from_secs(5), "the detached holder to start", || {
            super::scan::executable_path(pid).is_some_and(|path| path == running)
        });
        Self {
            pid,
            executable: running,
        }
    }

    /// The holder's pid.
    pub(super) fn pid(&self) -> u32 {
        self.pid
    }

    /// The binary it runs, which lives on the drive.
    pub(super) fn executable(&self) -> &Path {
        &self.executable
    }
}

impl Drop for DetachedHolder {
    fn drop(&mut self) {
        // Nobody's child, so there's nothing to reap: launchd does that.
        let Ok(pid) = libc::pid_t::try_from(self.pid) else {
            return;
        };
        // SAFETY: `kill` takes two integers by value and touches no memory of ours. The
        // pid is one this holder started; the worst a reused one costs is a stray signal
        // to a process this test also owns.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}
