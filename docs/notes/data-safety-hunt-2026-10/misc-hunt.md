# Data-safety hunt 2026-10: `misc` slice (git, Downloads, listing, cloud, tags, open with, rename, create)

Three findings survived. The headline one sits on the seam between this slice (the listing's `FileEntry` shape) and the
volume delete walker, so the `delete` hunter may also report it; the verifier should merge them if so.

### misc-1. Deleting a folder on an external drive (or any non-`root` volume) deletes the contents of every folder a symlink inside it points to

- **Severity:** critical
- **Confidence:** plausible (every step read in code; not run. The one step not exercised is that a real external-disk
  pane reaches `delete_files` with a non-`root` volume id, which `local_start.rs` and `volumes/` say it does)
- **Where:** `crates/cmdr-operations/src/delete/walker.rs:440`, `scan_volume_recursive` (recursion on
  `child.is_directory`); `crates/cmdr-operations/src/delete/walker.rs:741`, `delete_volume_files_with_progress_inner`
  (top-level `parent_hint` from the cached listing); `crates/cmdr-local/src/reading.rs:360`, `process_dir_entry`
  (`is_dir = metadata.is_dir() || target_is_dir`); `crates/cmdr-local/src/local_posix.rs:571`,
  `LocalPosixVolume::delete`; `crates/cmdr-operations/src/local_start.rs:398` (routing)
- **Excerpt:**

```rust
// crates/cmdr-local/src/reading.rs:343
pub(crate) fn process_dir_entry(entry: &fs::DirEntry, dot_hidden_names: &HashSet<String>) -> Option<FileEntry> {
    let file_type = entry.file_type().ok()?;
    let is_symlink = file_type.is_symlink();

    let target_is_dir = if is_symlink {
        fs::metadata(entry.path()).map(|m| m.is_dir()).unwrap_or(false)
    } else {
        false
    };
    ...
    let is_dir = metadata.is_dir() || target_is_dir;
```

```rust
// crates/cmdr-operations/src/delete/walker.rs:440
        for child in &children {
            let child_path = PathBuf::from(&child.path);
            if child.is_directory {
                Box::pin(scan_volume_recursive(
                    volume,
                    volume_id,
                    &child_path,
                    Some(true),
                    entries,
                    total_bytes,
                    seen_inodes,
                    state,
                    events,
                    operation_id,
                    tracker,
                ))
                .await?;
```

- **Mechanism:**
  1. `delete_files_start` (`local_start.rs:398`) sends every `volume_id != "root"` delete to the volume walker. External
     disks, DMGs, and OS-mounted shares are `LocalPosixVolume`s registered under their own ids (`volumes/lifecycle.rs`,
     `volume_id_for_mount`), so they take this path.
  2. `scan_volume_recursive` lists each directory either from the oracle (`authoritative_listing`, a pane's cached
     listing) or `list_directory_with_cancel` → `LocalPosixVolume::list_directory` → `process_dir_entry`. For a symlink
     whose target is a directory, that entry is `is_directory: true, is_symlink: true` (the pane wants to show it as a
     folder).
  3. The recursion checks `child.is_directory` only, never `is_symlink`, and passes `Some(true)` so no `lstat` probe
     runs. It calls `list_directory("<dir>/link")`; `fs::read_dir` follows the link and returns the TARGET's children,
     each with a path `"<dir>/link/<name>"`.
  4. Every such child goes into `entries` as a file to delete (or recurses further). In the delete phase,
     `LocalPosixVolume::delete("<dir>/link/<name>")` does `symlink_metadata` (which only declines to follow the LAST
     component) and `remove_file`, so it removes the file inside the link's target.
  5. Finally the link itself is pushed as `is_dir: true`; `delete` sees `symlink_metadata(link).is_dir() == false` and
     unlinks it. The target folder is left standing and empty.
  6. Top level too: with no scan preview (skip-confirmation, or a preview miss), `parent_hint` reads `e.is_directory`
     off the parent's cached listing (`walker.rs:741`), so selecting the symlink ITSELF and pressing Shift+F8 also walks
     into its target. (With a preview, `top_level_is_directory` probably comes from an `lstat`-based scan and the
     top-level case is safe, but step 3 still applies to every symlink nested inside a selected folder.)
- **Trigger:** The user deletes a folder on an external SSD, a USB stick, or a mounted share that contains a symlink to
  a directory. Realistic sources: a project copied with its `node_modules` (pnpm and npm workspaces are full of
  directory symlinks), a backup holding `Documents -> /Users/<me>/Documents`-style links, a macOS app bundle's
  `Versions/Current` link, a Time Machine-adjacent folder. An absolute link pointing back to the boot disk wipes that
  folder on the boot disk.
- **Guard ruled out:**
  - The boot disk's `root` walker uses `walkdir` (not this code), so the bug is invisible on the most-tested path.
  - `LocalPosixVolume::is_directory` uses `symlink_metadata` (`local_posix.rs:401`), but the walker skips it whenever it
    has a hint, which is always below the top level.
  - `list_directory_as_source` exists for walkers, but only drops AppleDouble sidecars and is not what this walker calls
    anyway; nothing marks a symlink as a non-directory for walkers.
  - No containment check in the delete phase (`walker.rs` ~800–960 deletes `entries` as listed).
  - `crates/cmdr-operations/src/delete/` has no symlink test for the volume walker (`rg symlink` hits only trash).
- **Impact:** Permanent deletion (not Trash) of everything inside the symlink's target, possibly far outside the
  selection and on another disk. The operation log journals the `link/<name>` paths as deleted leaves; nothing is
  recoverable. Likely also affects any non-local backend whose listing sets `is_directory` from the link target (worth a
  check on SFTP/SMB/ADB, not traced here).
- **Suggested fix:** In `scan_volume_recursive` (and the top-level `parent_hint`), treat `is_symlink` entries as leaf
  files: recurse only on `is_directory && !is_symlink`, and let `delete` unlink the link. A regression cell: a
  `LocalPosixVolume` tempdir with `dir/link -> ../outside/` deleted through `delete_volume_files_with_progress_inner`
  must leave `outside/` intact.

### misc-2. Toggling a color tag on a file whose tags can't be read replaces all its Finder tags with that one color

- **Severity:** low
- **Confidence:** confirmed (traced end to end; the triggering condition is rare)
- **Where:** `apps/desktop/src-tauri/src/file_system/tags.rs:26`, `read_tags`; `tags.rs:185`, `toggle_color`;
  `tags.rs:156`, `set_tags`
- **Excerpt:**

```rust
// apps/desktop/src-tauri/src/file_system/tags.rs:26
pub fn read_tags(path: &Path) -> Vec<TagRef> {
    match xattr::get(path, TAGS_XATTR) {
        Ok(Some(bytes)) => parse_tags_plist(&bytes),
        // No xattr, or a read error: no tags. Purely additive — a file with
        // unreadable tags simply shows none.
        _ => Vec::new(),
    }
}
...
// tags.rs:193
    let current: Vec<(String, Vec<TagRef>)> = paths.iter().map(|p| (p.clone(), read_tags(Path::new(p)))).collect();
...
        if tags != original
            && let Err(e) = set_tags(Path::new(&path), &tags)
```

- **Mechanism:** `toggle_color` reads each file's current tags through `read_tags`, which folds a `getxattr` error AND a
  plist decode failure (or a non-array root, or non-string elements, via `filter_map(as_string)`) into "no tags". The
  add branch then pushes the one system color and `set_tags` writes the whole array back with `xattr::set`, replacing
  the attribute. The display-path comment ("purely additive") is true for reading and false once the result feeds a
  write.
- **Trigger:** A file whose `_kMDItemUserTags` the user can write but not read (write-only mode, odd ACLs), or whose
  value another tool wrote in a shape `parse_tags_plist` rejects; the user presses a tag-color shortcut.
- **Guard ruled out:** `set_tags` is only skipped when `tags == original`; with `original` empty and the color pushed,
  it always writes. No re-read or merge at write time.
- **Impact:** The file's other Finder tags (user-authored names) are lost. Not recoverable from Cmdr.
- **Suggested fix:** Give the write path its own reader returning `Result<Option<Vec<TagRef>>>`, and skip (and report) a
  file whose existing xattr couldn't be read or fully decoded, rather than treating it as untagged.

### misc-3. On Linux, the Multi-Rename "Results" file is a predictable path in shared `/tmp`, written through symlinks

- **Severity:** low
- **Confidence:** confirmed (traced; macOS is unaffected because `temp_dir()` is the per-user `$TMPDIR`)
- **Where:** `apps/desktop/src-tauri/src/multi_rename/names_file.rs:93`, `write`; `names_file.rs:111`, `path_for`;
  `names_file.rs:118`, `this_process_files`
- **Excerpt:**

```rust
// apps/desktop/src-tauri/src/multi_rename/names_file.rs:93
pub fn write(path: &Path, rows: &[PreviewRow]) -> io::Result<Written> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, render(rows))?;
...
pub fn path_for(session_id: &str) -> PathBuf {
    let (dir, prefix) = this_process_files();
    dir.join(format!("{prefix}{session_id}.txt"))
}
...
        std::env::temp_dir().join("cmdr-multi-rename"),
        format!("{}-", std::process::id()),
```

- **Mechanism:** The path is `/tmp/cmdr-multi-rename/<pid>-<session>.txt`, all guessable (session ids count from 1).
  `create_dir_all` accepts a directory another user already made, and `std::fs::write` opens with `O_CREAT|O_TRUNC`,
  following a symlink planted at that name. `Drop` later `remove_file`s the same path.
- **Trigger:** A multi-user Linux machine where another local account pre-creates `/tmp/cmdr-multi-rename/` and a
  symlink `<pid>-1.txt -> /home/<victim>/somefile`; the victim opens Results (⌥⏎).
- **Guard ruled out:** No `O_EXCL`, no `O_NOFOLLOW`, no ownership or mode check on the directory.
- **Impact:** A file the victim owns is truncated and overwritten with the rename list; on the same box the other user
  can also read the victim's filenames.
- **Suggested fix:** Put the file under the app's own data/cache dir (or a `tempfile::Builder` directory created with
  mode 0700), and create it with `create_new(true)`.

## Covered

Read in full:

- `crates/cmdr-git/src/tree.rs`, `crates/cmdr-git/src/path.rs` (classification half), `crates/cmdr-git/CLAUDE.md`
- `apps/desktop/src-tauri/src/file_system/git/wiring.rs`, `overlay.rs`, `walker_exposure_tests.rs`, `CLAUDE.md`
- `apps/desktop/src-tauri/src/file_system/volume/manager/git_routing.rs`
- `crates/cmdr-operations/src/rename.rs`, `rename/validity.rs`, `rename/bulk.rs`, `rename/bulk/plan.rs`,
  `rename/bulk/by_move.rs`
- `crates/cmdr-operations/src/create.rs`, `create_numbered.rs`, `unique_name.rs`
- `apps/desktop/src-tauri/src/multi_rename/run.rs`, `names_file.rs`, `CLAUDE.md`
- `apps/desktop/src-tauri/src/file_system/tags.rs` (non-test half), `cloud_actions.rs`
- `apps/desktop/src-tauri/src/file_system/file_provider_actions/mod.rs` (non-test half)
- `crates/cmdr-listing/src/authoritative.rs`
- Followed out of the slice: `crates/cmdr-local/src/local_posix.rs` (`rename_exclusive_with`, `create_file`,
  `is_directory`, `delete`, `list_directory`), `crates/cmdr-local/src/reading.rs` (`process_dir_entry`,
  `get_single_entry`, the listing loop), `crates/cmdr-operations/src/delete/walker.rs` (volume walker),
  `crates/cmdr-operations/src/local_start.rs` (`delete_files_start`), `crates/cmdr-operations/src/look_alike.rs`
  (`place_new_entry`), `crates/cmdr-operations/src/transfer/volume/move_same.rs` (routing head),
  `apps/desktop/src/lib/file-explorer/rename/rename-operations.ts` (save flow)

## Not covered

- `crates/cmdr-git/src/` beyond `tree.rs`/`path.rs`/`volume.rs` heads: `status.rs`, `watcher.rs`, `repo.rs`, `stash.rs`,
  `log.rs`, `snapshot_dates.rs`, `column_meta.rs`, `read_blob.rs`, `submodules.rs`, `worktrees.rs`, `virtual_listing.rs`
  (read only its tail). The crate is read-only by type (`is_writable()` false, every mutation `NotSupported`), so I
  spent the time elsewhere.
- `apps/desktop/src-tauri/src/downloads/` beyond `CLAUDE.md` and a grep for writes: it emits events only.
- `crates/cmdr-listing/src/` beyond `authoritative.rs`; `apps/desktop/src-tauri/src/file_system/watcher.rs`;
  `listing_host/`.
- `file_provider_actions/objc.rs`, `declarations.rs`; `google_drive/` (documented read-only, link lookup only).
- `file_system/open_with.rs` and `menu/open_with.rs` beyond the function list (launch only, no writes).
- `multi_rename/` `session.rs`, `plan.rs`, `mask.rs`, `transform.rs`, `presets.rs`, `history.rs`;
  `commands/multi_rename.rs`.
- Whether SFTP/SMB/ADB/WebDAV listings mark a symlink-to-dir `is_directory: true` (would widen misc-1 to those volumes;
  `cmdr-sftp/src/volume/mapping.rs:40` uses the raw file type, so SFTP may be fine).

## Near misses

- Single rename onto an existing name from a stale listing: local uses `rename_local_exclusive` (`RENAME_EXCL`), and
  every backend's `rename(force=false)` is pinned by `conformance::assert_rename_refuses_an_existing_destination`.
- Case-only rename: sent with `force=false`, so at worst refused as "taken"; local `from == to` is the only plain
  `rename`. The remote `found_the_source_itself` reads a pane listing, but the rename stays `force=false`.
- Bulk/multi-rename cycles and chains (`a→b`, `b→a`): every hop is an exclusive rename, cycles rotate through a
  `.cmdr-bulk-rename-*` temp with restore-on-failure; a wrong plan can only skip or fail, never overwrite.
- Bulk plan `source_to_index[&destination]` would panic on two rows sharing a destination, but the panic lands before
  any hop (plan is built first) and multi-rename marks duplicates `Duplicate` upstream.
- A rename batch run as a move on S3 (`by_move.rs`) has no `settle_remote_conflicts`; folder-onto-existing-folder would
  merge under `Skip`, but multi-rename's `TargetExists` status blocks it outside a race, and sources move serially.
- New file / New folder clobbering: `Volume::create_file` is `create_new(true)` locally and conformance-pinned on every
  backend; `create_numbered_file` retries on typed `AlreadyExists`.
- `find_unique_name` returning an unreserved candidate on a non-`EEXIST` error: the caller's write hits the same error.
- Git portal shadowing the real `.git/worktrees/` and `.git/branches/`: a delete or write there resolves to the
  read-only portal and is refused; walkers list through the parent volume and never route per child.
- Hostile tree-entry names in a cloned repo (`..`, `a/b`) copied out of `.git/branches/…`: the transfer engine refuses
  non-component names (`transfer/volume/hostile_names_tests.rs`), and `GitPortalVolume::local_path()` is `None`.
- Tag writes clobbering other xattrs: `set_tags` touches only `_kMDItemUserTags`.
- iCloud evict/download: Cmdr only calls `evictUbiquitousItemAtURL`/`startDownloading…` on an iCloud Drive path; any
  unsynced-change protection is Apple's.
- File Provider actions: performed by item identifier captured at menu time, executed by the provider itself.
- Downloads watcher: no writes (only `create_dir_all` of the E2E root under `is_e2e_mode`).
