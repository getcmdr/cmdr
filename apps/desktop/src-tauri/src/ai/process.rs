//! llama-server process lifecycle management.

use super::extract::LLAMA_SERVER_BINARY;
use std::fs;
use std::path::Path;

use cmdr_fs::log_private::LogPrivate;

/// Log filename for llama-server output (useful for debugging startup issues).
pub const SERVER_LOG_FILENAME: &str = "llama-server.log";

/// Spawns the llama-server process and returns its PID.
///
/// The caller is responsible for health checking and state management.
pub fn spawn_llama_server(ai_dir: &Path, model_filename: &str, port: u16, ctx_size: u32) -> Result<u32, String> {
    let binary_path = ai_dir.join(LLAMA_SERVER_BINARY);
    let model_path = ai_dir.join(model_filename);

    log::debug!("AI server: checking files at {:?}", LogPrivate::dir_path(ai_dir));
    log::debug!(
        "AI server: binary exists={}, model exists={}",
        binary_path.exists(),
        model_path.exists(),
    );

    if !binary_path.exists() || !model_path.exists() {
        return Err(String::from("AI files not found"));
    }

    // Create log file for llama-server output (helps debug startup issues)
    let log_path = ai_dir.join(SERVER_LOG_FILENAME);
    let log_file = fs::File::create(&log_path).map_err(|e| format!("Failed to create llama-server log file: {e}"))?;
    let log_file_stderr = log_file
        .try_clone()
        .map_err(|e| format!("Failed to clone log file handle: {e}"))?;

    log::debug!("AI server: logging output to {:?}", LogPrivate::file_path(&log_path));

    // Spawn llama-server with DYLD_LIBRARY_PATH set to find the shared libraries
    // The @rpath in the binary points to the directory where the dylibs are located
    let ai_dir_str = ai_dir.to_string_lossy();
    log::debug!(
        "AI server: setting DYLD_LIBRARY_PATH to {}",
        LogPrivate::dir_path(ai_dir)
    );

    let child = std::process::Command::new(&binary_path)
        .env("DYLD_LIBRARY_PATH", &*ai_dir_str)
        .current_dir(ai_dir)
        .arg("-m")
        .arg(&model_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("127.0.0.1")
        .arg("-c")
        .arg(ctx_size.to_string())
        .arg("--temp")
        .arg("0.6")
        .arg("--top-p")
        .arg("0.95")
        .arg("-n")
        .arg("4096")
        .arg("--jinja")
        .arg("-ngl")
        .arg("99")
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file_stderr))
        .spawn()
        .map_err(|e| format!("Failed to start llama-server: {e}"))?;

    let pid = child.id();
    log::debug!("AI server: spawned llama-server with PID {pid}");

    Ok(pid)
}

/// Reads the last N lines from the llama-server log file for diagnostics.
pub fn read_log_tail(ai_dir: &Path, lines: usize) -> String {
    let log_path = ai_dir.join(SERVER_LOG_FILENAME);
    let log_content = fs::read_to_string(&log_path).unwrap_or_default();
    log_content
        .lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns some diagnostic info about the log file (line count, last line).
pub fn log_diagnostics(ai_dir: &Path) -> Option<(usize, String)> {
    let log_path = ai_dir.join(SERVER_LOG_FILENAME);
    let log_content = fs::read_to_string(&log_path).ok()?;
    let line_count = log_content.lines().count();
    let last_line = log_content.lines().last()?.to_string();
    Some((line_count, last_line))
}

/// Finds an available TCP port on localhost.
pub fn find_available_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|listener| listener.local_addr().ok())
        .map(|addr| addr.port())
}

/// Sends SIGKILL to a process. Returns immediately (~0.5ms).
///
/// Use for fire-and-forget scenarios (app quit, orphan cleanup from previous sessions).
/// llama-server is stateless: SIGKILL is safe. macOS reclaims all GPU/Metal/mmap resources
/// on process death regardless of signal. The llama.cpp test suite itself uses SIGKILL.
pub fn kill_process(pid: u32) {
    #[cfg(unix)]
    // SAFETY: `kill` takes two plain integers (no pointers): the pid of a llama-server we spawned
    // (or a stale one we found by matching our own binary path) and `SIGKILL`. No memory is shared
    // with the call; the worst a reused pid could do is signal another process, which is why every
    // caller passes a pid we sourced ourselves. The return value is intentionally ignored (the
    // process may already be gone — fire-and-forget).
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

/// Sends SIGKILL and reaps the zombie in a background thread.
///
/// Use during normal operation (settings switch, explicit stop) so zombies don't
/// accumulate over long-running sessions.
pub fn kill_and_reap_in_background(pid: u32) {
    kill_process(pid);
    #[cfg(unix)]
    // SAFETY: `waitpid` reaps the zombie of the llama-server child we just SIGKILLed. The pid is
    // one we spawned; the args are a plain pid integer, a null status pointer (we don't read the
    // exit status), and a 0 options flag — no memory is shared with the call. The return value is
    // ignored: the only outcome we care about is that the zombie is collected.
    std::thread::spawn(move || unsafe {
        libc::waitpid(pid as i32, std::ptr::null_mut(), 0);
    });
}

/// Stops every running llama-server whose executable is the one in `ai_dir`, whoever started it.
///
/// Belt-and-suspenders defense against orphans from race conditions or crashes, and the launch's
/// only way to find the previous run's server when `ai-state.json` was set aside with its pid
/// (`crate::state_recovery`). `pgrep` names the candidates by process name; each is stopped only
/// once its executable path proves it ours, so a process that merely mentions the path (a
/// `tail -f` of the log beside it) or another app's llama-server is never signalled.
pub fn kill_stale_llama_servers(ai_dir: &Path) {
    #[cfg(unix)]
    {
        // The OS reports an executable with its links resolved.
        let Ok(ours) = ai_dir.canonicalize().map(|dir| dir.join(LLAMA_SERVER_BINARY)) else {
            return;
        };
        let Ok(output) = std::process::Command::new("pgrep")
            .arg("-x")
            .arg(LLAMA_SERVER_BINARY)
            .output()
        else {
            return;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        for pid in stdout.lines().filter_map(|line| line.trim().parse::<u32>().ok()) {
            if executable_of(pid).as_deref() == Some(ours.as_path()) {
                log::info!("AI: stopping stale llama-server (PID {pid})");
                kill_process(pid);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ai_dir;
    }
}

/// The executable behind `pid`, `None` once it's gone or isn't ours to see.
#[cfg(target_os = "macos")]
fn executable_of(pid: u32) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let pid = i32::try_from(pid).ok()?;
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is a live, initialized buffer of exactly `size` bytes, which
    // `proc_pidpath` fills with at most that many bytes of path. No other memory is shared.
    let written = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast::<libc::c_void>(), size) };
    if written <= 0 {
        return None;
    }
    buffer.truncate(usize::try_from(written).ok()?);
    Some(std::ffi::OsString::from_vec(buffer).into())
}

/// The executable behind `pid`, `None` once it's gone or isn't ours to see.
#[cfg(all(unix, not(target_os = "macos")))]
fn executable_of(pid: u32) -> Option<std::path::PathBuf> {
    fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// Returns true if the process with the given PID is still running.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) checks if a process exists without sending a signal
        // SAFETY: signal-0 liveness probe. `kill` takes two plain integers (the pid and signal 0),
        // no pointers, so no memory is shared with the call. Signal 0 delivers nothing; it only runs
        // the kernel's permission/existence check. We read the return: `0` means the process exists.
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::{TestDir, wait_until};
    use std::process::{Child, Command};
    use std::time::Duration;

    /// Stopped by the end of the test, whatever it asserted.
    struct Running(Child);

    impl Running {
        fn has_exited(&mut self) -> bool {
            self.0.try_wait().expect("ask the child").is_some()
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A stand-in llama-server: `sleep`, copied to where ours lives, so its executable IS our
    /// binary's path.
    fn spawn_fake_server(ai_dir: &Path) -> Running {
        let binary = ai_dir.join(LLAMA_SERVER_BINARY);
        fs::copy("/bin/sleep", &binary).expect("copy sleep in as llama-server");
        Running(Command::new(&binary).arg("60").spawn().expect("start the fake server"))
    }

    /// The previous run's `ai-state.json` was unreadable at launch, so it was set aside along with
    /// the running server's pid. The launch sweep finds that server by its executable anyway.
    #[test]
    fn a_server_from_a_run_whose_state_file_was_set_aside_is_stopped_at_launch() {
        let _notices = crate::state_recovery::testing::capture_recoveries();
        let dir = TestDir::new("ai_stray_server");
        let mut server = spawn_fake_server(&dir);
        fs::write(dir.join("ai-state.json"), b"{ not json").expect("a state file that won't parse");

        let state = super::super::state::load_state(&dir);
        assert_eq!(state.pid, None, "precondition: the pid went with the set-aside copy");
        kill_stale_llama_servers(&dir);

        wait_until(Duration::from_secs(5), "the stray server to stop", || {
            server.has_exited()
        });
    }

    /// A process whose command line only MENTIONS our binary (a `tail -f` of the log beside it)
    /// isn't ours to stop. The real one beside it is, which is also how the test knows the sweep
    /// has run its course.
    #[test]
    fn a_process_that_only_names_our_binary_in_its_arguments_keeps_running() {
        let dir = TestDir::new("ai_not_our_server");
        let mut server = spawn_fake_server(&dir);
        let log = dir.join(SERVER_LOG_FILENAME);
        fs::write(&log, b"").expect("the log");
        let mut bystander = Running(
            Command::new("/usr/bin/tail")
                .arg("-f")
                .arg(&log)
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("start the bystander"),
        );

        kill_stale_llama_servers(&dir);

        wait_until(Duration::from_secs(5), "our own server to stop", || server.has_exited());
        assert!(!bystander.has_exited(), "❗ only our own llama-server may be stopped");
    }
}
