# Data-safety hunt 2026-10: `misc` verification

### misc-1

- **Verdict:** duplicate of delete-1
- **Severity:** critical (as delete-1)
- **Evidence:**
  - Same mechanism, same sites: `crates/cmdr-local/src/reading.rs:343` (`process_dir_entry`,
    `is_dir = metadata.is_dir() || target_is_dir`, excerpt verbatim) feeds
    `crates/cmdr-operations/src/delete/walker.rs:442` (`if child.is_directory`, no `is_symlink` check). The excerpt's
    recursion block starts at 440 as cited.
  - The extra trigger misc-1 names (a top-level selected symlink with no scan preview, via `parent_hint` reading
    `e.is_directory` off the cached parent listing, `walker.rs:741`–`756`) is real, but delete-1 already reports it (its
    mechanism step 7 cites `walker.rs:741`), and delete-1 also adds the cold-preview variant (`local_posix/scan.rs:118`,
    `std::fs::metadata` follows the link) and the ADB backend. misc-1 adds nothing delete-1 lacks; the delete verifier
    owns the verdict.

### misc-2

- **Verdict:** confirmed
- **Severity:** low (agree; the loss is real but needs an uncommon xattr state)
- **Evidence:**
  - `apps/desktop/src-tauri/src/file_system/tags.rs:26`–`32`: `read_tags` maps `Ok(None)`, every `Err(_)`, and (via
    `parse_tags_plist`, `:50`–`62`) a plist decode failure, a non-array root, and non-string array elements to the same
    empty/partial `Vec`. No way for a caller to tell "untagged" from "couldn't read".
  - `tags.rs:193`: `toggle_color` builds `current` from `read_tags`; with `original` empty, `all_have` is false so `add`
    is true (`:196`–`197`), the color is pushed (`:205`–`210`), `tags != original` holds, and `set_tags` (`:156`–`166`)
    does a full-replace `xattr::set` of a one-element array. Any tags the read missed are gone. No re-read, merge, or
    compare-and-set at write time.
  - The getxattr-fails-but-setxattr-succeeds split is real on macOS: the kernel checks `KAUTH_VNODE_READ_EXTATTRIBUTES`
    and `WRITE_EXTATTRIBUTES` separately, and with plain POSIX modes they follow the read and write bits, so a file the
    owner made `0200` (or an ACL granting `writeextattr` without `readextattr`) hits it. The decode-failure route needs
    a value Cmdr's parser rejects (`plist::Value::from_reader` accepts both XML and binary, so only a malformed buffer
    or non-string elements; the latter are silently dropped and then dropped from disk on the write-back).
  - Extra caller the hunter didn't cite, same flaw: the MCP `tag` tool's `"set"` action
    (`apps/desktop/src-tauri/src/mcp/executor/tags.rs:158`–`170`) reads with `read_tags`, computes
    `desired_tags_for_set`, and writes the whole set with `set_tags`. Its `"toggle"` action goes through `toggle_color`
    (`:146`–`156`). The menu path is `apps/desktop/src-tauri/src/menu/menu_handlers.rs:724`.
  - No test covers an unreadable or malformed xattr on the write path (the `toggle_color` tests at `tags.rs:495`–`559`
    all start from readable tags).

### misc-3

- **Verdict:** confirmed (Linux only)
- **Severity:** low (agree; local multi-user Linux attacker, and Linux isn't the primary platform)
- **Evidence:**
  - Path: `names_file.rs:118`–`123` gives `std::env::temp_dir().join("cmdr-multi-rename")` and prefix `"<pid>-"`;
    `path_for` (`:111`–`114`) appends the session id. Correction to the hunter: the session id is `multi-rename-N`
    (`session.rs:149`, a process-global `NEXT_SESSION` counter), so the file is
    `/tmp/cmdr-multi-rename/<pid>-multi-rename-<N>.txt`. Still fully predictable: the pid is visible in `/proc` to other
    users (unless `hidepid`), and `N` starts at 1.
  - `temp_dir()` on Linux is `$TMPDIR` or `/tmp` (shared, sticky, world-writable); on macOS it's the per-user
    `/var/folders/…/T/`, so macOS is unaffected. `multi_rename` is compiled on every platform (`lib.rs:113`, no `cfg`).
  - `write` (`names_file.rs:93`–`97`): `create_dir_all` succeeds silently on a directory another user already created,
    with no owner or mode check; `std::fs::write` is `File::create` (`O_WRONLY|O_CREAT|O_TRUNC`, no
    `O_EXCL`/`O_NOFOLLOW`), so it follows a planted symlink and truncates the victim-owned target.
  - Linux's `fs.protected_symlinks=1` does NOT save it: that only applies to symlinks in sticky world-writable dirs, and
    the attacker makes `cmdr-multi-rename/` an ordinary `0755` dir it owns.
  - Wider than reported: because the attacker owns the directory, they can also unlink the victim's file and put their
    own in its place, and `read_names` (`session.rs` `read_names`, `std::fs::read_to_string(&path)`) reads it back, so
    the attacker chooses new names for the victim's selected rows (still bounded by the session's own rows and every
    status check, so no rename outside the selection). They can also read the victim's filenames. `Drop`'s `remove_file`
    and the data wipe's `FilesWithPrefix` (`data_wipe/plan.rs:91`) only unlink entries, so they don't follow links.
  - Suggested fix stands: per-user dir (app cache dir, or `tempfile` 0700 dir) plus `create_new(true)`.
