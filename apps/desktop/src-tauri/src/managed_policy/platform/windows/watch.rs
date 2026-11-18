//! The refresh trigger on Windows: a change notification on `SOFTWARE\Policies` in both hives, which
//! a Group Policy refresh (or an admin's `reg add`) writes. Watches the parent, subtree included,
//! because `Policies\Rymd\Cmdr` doesn't exist until the first policy creates it. Only calls
//! [`crate::managed_policy::refresh`]; the registry stays the source of truth.

use crate::platform::registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, Key};
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_FAILED, WAIT_TIMEOUT};
use windows_sys::Win32::System::Registry::{
    HKEY, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, RegNotifyChangeKeyValue,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForMultipleObjects};

const POLICIES: &str = "SOFTWARE\\Policies";

/// A GPO refresh writes many values in a burst; one re-read after it settles covers them all.
const SETTLE: Duration = Duration::from_millis(250);

pub(super) fn start() {
    let spawned = std::thread::Builder::new()
        .name("managed-policy-watch".to_string())
        .spawn(|| {
            let Some(watch) = Watch::new(&[(HKEY_LOCAL_MACHINE, POLICIES), (HKEY_CURRENT_USER, POLICIES)]) else {
                return;
            };
            loop {
                match watch.wait(None) {
                    Ok(true) => {
                        std::thread::sleep(SETTLE);
                        if crate::managed_policy::refresh() {
                            log::debug!(target: "managed_policy", "Re-read the managed policy after a registry change: changed");
                        }
                    }
                    Ok(false) => {}
                    Err(code) => {
                        log::warn!(target: "managed_policy", "The policy registry watch stopped (Win32 error {code}); policy changes apply at the next egress read or launch");
                        return;
                    }
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!(target: "managed_policy", "Couldn't start the policy registry watch: {e}");
    }
}

/// Change notifications on a set of keys, each with its own auto-reset event. Notifications are
/// one-shot, so every `wait` arms them again first. They're tied to the thread that arms them, so
/// one `Watch` lives on one thread.
struct Watch {
    keys: Vec<Key>,
    events: Vec<HANDLE>,
}

impl Watch {
    /// Opens each key for notification; `None` (logged) when none of them could be.
    fn new(paths: &[(HKEY, &str)]) -> Option<Self> {
        let mut keys = Vec::new();
        for (root, path) in paths {
            match Key::open_with(*root, path, KEY_NOTIFY) {
                Ok(Some(key)) => keys.push(key),
                Ok(None) => {
                    // allowed-log-private: `path` is our own fixed registry key, not a user value
                    log::debug!(target: "managed_policy", "{path} doesn't exist in one hive; not watching it");
                }
                Err(code) => {
                    // allowed-log-private: `path` is our own fixed registry key, not a user value
                    log::warn!(target: "managed_policy", "Couldn't open {path} to watch it: Win32 error {code}");
                }
            }
        }
        if keys.is_empty() {
            return None;
        }
        let mut events = Vec::with_capacity(keys.len());
        for _ in &keys {
            // SAFETY: null security attributes and name are documented as allowed; we own the
            // returned handle and close it in `Drop`.
            let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
            if event.is_null() {
                log::warn!(target: "managed_policy", "CreateEventW failed: {}", std::io::Error::last_os_error());
                for made in events {
                    // SAFETY: each is an event handle we created above and haven't closed.
                    unsafe { CloseHandle(made) };
                }
                return None;
            }
            events.push(event);
        }
        Some(Self { keys, events })
    }

    /// Arms every notification, then waits up to `timeout` (forever when `None`). `Ok(true)` when a
    /// key changed, `Ok(false)` on timeout.
    fn wait(&self, timeout: Option<Duration>) -> Result<bool, u32> {
        self.arm()?;
        self.block(timeout)
    }

    /// Registers each key's one-shot notification on its event.
    fn arm(&self) -> Result<(), u32> {
        for (key, event) in self.keys.iter().zip(&self.events) {
            // SAFETY: `key` is an open handle with `KEY_NOTIFY` and `event` a live event, both
            // owned by `self` and outliving the registration (dropping `self` closes the key,
            // which ends it).
            let status = unsafe {
                RegNotifyChangeKeyValue(
                    key.handle(),
                    1,
                    REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                    *event,
                    1,
                )
            };
            if status != ERROR_SUCCESS {
                return Err(status);
            }
        }
        Ok(())
    }

    /// Waits for an armed notification: `Ok(true)` when one fired, `Ok(false)` on timeout.
    fn block(&self, timeout: Option<Duration>) -> Result<bool, u32> {
        let millis = timeout.map_or(INFINITE, |t| u32::try_from(t.as_millis()).unwrap_or(INFINITE - 1));
        let count = u32::try_from(self.events.len()).unwrap_or(u32::MAX);
        // SAFETY: `events` holds `count` live event handles for the whole wait.
        let result = unsafe { WaitForMultipleObjects(count, self.events.as_ptr(), 0, millis) };
        match result {
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(std::io::Error::last_os_error()
                .raw_os_error()
                .map_or(WAIT_FAILED, |c| c as u32)),
            _ => Ok(true),
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for event in &self.events {
            // SAFETY: each is an event handle `new` created and nothing else closes.
            unsafe { CloseHandle(*event) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::registry::ScratchKey;

    /// A write under the watched key (here a scratch key, never `Policies`) wakes the watch, a
    /// quiet key times out, and a change two levels down counts too (`Policies\Rymd\Cmdr` under
    /// the watched `Policies`).
    #[test]
    fn a_change_under_the_watched_key_wakes_it() {
        let scratch = ScratchKey::new("policy-watch");
        let watch = Watch::new(&[(HKEY_CURRENT_USER, scratch.path())]).expect("watch the scratch key");

        assert_eq!(
            watch.wait(Some(Duration::from_millis(200))),
            Ok(false),
            "nothing changed yet"
        );

        watch.arm().expect("arm");
        crate::platform::registry::set_dword_at(&format!("{}\\Rymd\\Cmdr", scratch.path()), "DisableUpdates", 1);
        assert_eq!(
            watch.block(Some(Duration::from_secs(10))),
            Ok(true),
            "the nested write wakes the watch"
        );
    }
}
