//! Windows: `adb.exe`, with Android Studio's SDK under `%LOCALAPPDATA%`, started with
//! `CREATE_NO_WINDOW`.

use std::path::Path;

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

pub(crate) const HOST_SEARCH: super::AdbSearch = super::AdbSearch::WINDOWS;

/// Windows runs a file by its extension, and every candidate ends in `.exe`, so a regular file
/// is enough.
pub(crate) fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// `CREATE_NO_WINDOW`: `adb.exe` is a console program, and a GUI process (Cmdr has no console)
/// that starts one without it makes Windows open a fresh console window for it, a black flash
/// every time Cmdr starts the ADB server. With it, the child gets a console with no window, and
/// the server `adb start-server` forks detaches from it as it always does.
pub(crate) fn start_quietly(command: &mut tokio::process::Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Console::GetConsoleWindow;

    use super::*;

    /// Set in the child [`console_probe`] runs as.
    const PROBE: &str = "CMDR_ADB_CONSOLE_PROBE";
    /// The probe's exit codes.
    const NO_WINDOW: i32 = 0;
    const HAS_WINDOW: i32 = 3;

    /// The child half of the test below: exits with whether its console has a window. Ignored,
    /// so it runs only when the test below starts this test binary for it.
    #[test]
    #[ignore = "a child process of a_quiet_start_gives_the_child_no_console_window"]
    fn console_probe() {
        if std::env::var_os(PROBE).is_none() {
            return;
        }
        // SAFETY: no arguments; it only reads this process's console state.
        let window = unsafe { GetConsoleWindow() };
        std::process::exit(if window.is_null() { NO_WINDOW } else { HAS_WINDOW });
    }

    /// The real thing on a real Windows: a console program (this test binary) started through
    /// [`start_quietly`] gets no console window. Run from a console (nextest over SSH), a child
    /// started without the flag would inherit that console's window and fail this.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_quiet_start_gives_the_child_no_console_window() {
        let me = std::env::current_exe().expect("the test binary's path");
        let mut command = tokio::process::Command::new(me);
        command
            .args(["--exact", "server::platform::imp::tests::console_probe", "--ignored"])
            .env(PROBE, "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        start_quietly(&mut command);
        let status = command.status().await.expect("the probe starts");
        assert_eq!(status.code(), Some(NO_WINDOW), "{status:?}");
    }
}
