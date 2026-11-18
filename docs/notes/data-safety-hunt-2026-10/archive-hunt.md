# Data-safety hunt 2026-10: archive editing and extraction

Hunter slug: `archive`. Scope: in-place zip rewrites (`ArchiveMutator`), the archive-edit driver
(`crates/cmdr-operations/src/archive_edit/`), the remote pull-apply-upload-swap path, move into and out of a zip, fresh
compress publication, the archive read core's name sanitizer and index cache, and extraction to the open-with temp.

Seven findings survive, ranked. Severity counts: high 3, medium 2, low 2.

### archive-1. A remote zip edit whose final rename fails leaves the only copy of the archive under a `.cmdr-tmp-*` name, which the next copy into that folder deletes an hour later

- **Severity:** high
- **Confidence:** plausible (the server-side reason a rename fails after a delete succeeded is not traced end to end;
  every Cmdr-side step is)
- **Where:** `crates/cmdr-operations/src/archive_edit/remote.rs:284`, `swap_into_place`; `remote.rs:146`,
  `place_local_file`; `crates/cmdr-operations/src/transfer/volume/cleanup.rs:367`, `reap_stale_transfer_temps`
  (`STALE_TEMP_MIN_AGE` = 1 h at `cleanup.rs:40`); contrast `crates/cmdr-operations/src/transfer/volume/finalize.rs:84`
  (`swap_into_place` + `rescue_out_of_temp_space`)
- **Excerpt:**

```rust
284	async fn swap_into_place(parent: &dyn Volume, remote_temp: &Path, archive_path: &Path) -> Result<(), EditError> {
...
291	    if parent.create_directory_errors_on_existing_dir() && parent.rename(remote_temp, archive_path, true).await.is_ok()
292	    {
293	        return Ok(());
294	    }
295
296	    // Delete-then-rename. Tolerate a MISSING original: `place_local_file` can publish
297	    // a brand-new target on MTP, which allows same-name siblings. Any OTHER delete fault is real. The crash window (between
298	    // the two) leaves the NEW, fully-uploaded data under the temp name: recoverable,
299	    // never lost.
300	    match parent.delete(archive_path).await {
301	        Ok(()) => {}
302	        Err(VolumeError::NotFound(_)) => {}
303	        Err(err) => return Err(EditError::Op(to_write_error(archive_path, &err))),
304	    }
305	    parent
306	        .rename(remote_temp, archive_path, false)
307	        .await
308	        .map_err(vol_op(remote_temp))?;
309	    Ok(())
310	}
```

- **Mechanism:**
  1. Any remote zip edit (delete/rename/mkdir inside, copy or move into, move out) ends in `place_local_file` →
     `upload_archive` to `foo.zip.cmdr-tmp-<uuid>` → `swap_into_place`.
  2. MTP always, and SMB/SFTP/WebDAV/ADB whenever the forced rename returns `Err` for ANY reason, take the
     delete-then-rename branch. The delete succeeds, the plain rename fails: the original is gone and the complete new
     archive exists only as `foo.zip.cmdr-tmp-<uuid>`. The function returns a bare `IoError` naming the temp path; it
     does not rename the bytes to a non-temp name, and no `NewDataKeptAt` tells the user this file is now their archive.
  3. `reap_stale_transfer_temps` runs at the start of every copy (`transfer/volume/copy.rs:674`) and volume move
     (`move_cross.rs:253`) into that directory and deletes any file whose name contains `.cmdr-tmp-` and whose mtime is
     over an hour old. Its own doc comment says: "Nothing may leave committed data under a temp name... a `.cmdr-tmp-*`
     that holds the only copy of something is deleted here an hour later." The transfer engine's own swap
     (`finalize.rs`) renames to ` (recovered)` for exactly this reason; the archive swap copies its shape but not the
     rescue.
  4. Second branch of the same defect: the fallback runs `parent.delete(archive_path)` on any forced-rename error
     without checking the temp is still there. A forced rename that the server applied but answered with an error (a
     reply lost to a dropped connection, then a reconnect for the delete) means `archive_path` already holds the NEW
     bytes: the delete removes them, and the rename then fails `NotFound` on the vanished temp. Old and new are both
     gone at once.
- **Trigger:** Edit a zip on an MTP phone and the device hiccups between the delete and the rename (USB link, device
  busy), or edit a zip on an SMB share while another client has it open with delete sharing (Finder Quick Look, a
  Windows indexer): the replacing rename is refused, the delete only marks it delete-pending, and the plain rename hits
  the still-occupied name. Any crash or force quit between the two calls lands in the same state. An hour or more later,
  the user copies anything into that folder.
- **Guard ruled out:** The archive edit's own `reap_remote_temps` (24 h gate) is not the problem; the transfer engine's
  1 h reaper is, and it knows nothing about this temp (no ledger entry, no rescue).
  `create_directory_errors_on_existing_dir` only chooses the branch. No test in `remote_tests.rs` makes the rename fail
  after the delete (the five tests cover reap, round trip, cancel, and the happy sibling-allowing swap).
- **Impact:** The whole archive (every entry, not just the edited one) is permanently deleted from the remote. Before
  the reap, it's recoverable only if the user recognizes `foo.zip.cmdr-tmp-<uuid>` as their archive.
- **Suggested fix:** Reuse the transfer engine's landing (`what_a_refused_delete_left` before trusting a delete, and
  `rescue_out_of_temp_space` + `NewDataKeptAt` when the rename fails after the delete), and before the fallback delete
  confirm the temp still exists (if it doesn't, the forced rename landed: return `Ok`).

### archive-2. Moving a folder into a zip that holds a FILE of the same name hides the folder's contents inside the zip, then deletes the source folder

- **Severity:** high
- **Confidence:** confirmed (traced end to end)
- **Where:** `crates/cmdr-operations/src/archive_edit/copy_into.rs:422`, `build_copy_into_changeset_inner`;
  `copy_into.rs:728` and `:741` (the move's source sweep); `crates/cmdr-archive/src/mutation/mutator.rs:287`, `apply`
  (retained entries written first); `crates/cmdr-archive/src/read/index.rs:609` and `:663`, `build_tree` /
  `ensure_ancestors`
- **Excerpt:**

```rust
422	        if meta.is_dir() {
423	            for entry in WalkDir::new(src).into_iter().filter_map(Result::ok) {
424	                let rel = entry.path().strip_prefix(src).unwrap_or_else(|_| entry.path());
425	                let inner = if rel.as_os_str().is_empty() {
426	                    base_inner.clone()
427	                } else {
428	                    join_inner_str(&base_inner, &rel.to_string_lossy().replace('\\', "/"))
429	                };
430	                let file_type = entry.file_type();
431	                if file_type.is_dir() {
432	                    if !index.exists(&inner) && planned.insert(inner.clone()) {
433	                        mkdir_sources.insert(inner.clone(), entry.path().to_path_buf());
434	                        mkdirs.push(inner);
435	                    }
436	                } else if file_type.is_file() {
437	                    plan_file_add(
```

and the reader that later drops those children (`index.rs`):

```rust
609	    for seed in seeds {
610	        // Path collisions are resolved first-writer-wins. If an ancestor path is
611	        // already a FILE, this entry can't be placed under it — drop it rather
612	        // than leaving it as an unreachable orphan.
613	        if !ensure_ancestors(&seed.path, &mut nodes, &mut child_paths, &mut linked) {
614	            continue;
615	        }
```

- **Mechanism:**
  1. The zip holds a file entry `notes`. The user moves a local folder `notes/` (with `a.txt`, `b.txt`) into the zip's
     root.
  2. The walk reaches the folder itself: `index.exists("notes")` is true (it's the FILE), so the mkdir is silently
     dropped. No conflict is raised, whatever the policy: directory collisions never consult it (DETAILS § Conflicts: "a
     source DIRECTORY meeting a same-named FILE entry just skips its `mkdir` and adds its children under the name").
  3. Each child `notes/a.txt` doesn't exist in the index, so it's a plain add. `skipped_count` stays 0, so
     `should_delete = is_move && plan.skipped_count == 0` is true.
  4. The mutator writes retained entries first (the file `notes`), then the adds `notes/a.txt`, `notes/b.txt`. The
     rewrite commits.
  5. `delete_originals` sweeps the carried ledger: the source folder and its files are removed from disk.
  6. Every later parse (`build_tree`) meets the file `notes` first; `ensure_ancestors("notes/a.txt")` finds a FILE at
     `notes` and returns false, so both children are dropped from the tree and their read handles pruned
     (`index.rs:568`). Cmdr never lists or extracts them again. `unzip` refuses them too (it can't make a folder where a
     file is).
- **Trigger:** A zip containing an extensionless file (`notes`, `build`, `LICENSE`-like names) and a folder of the same
  name in the pane opposite; F6 into the zip. Deeper levels hit it too (an archive folder holding a FILE `x`, and the
  moved tree holding a folder `x` at that spot).
- **Guard ruled out:** The conflict layer is file-only (`plan_file_add` / `resolve_effective`); the FE's confirm-dialog
  answer for the top-level clash never reaches the dir branch. The stamp-ledger sweep only protects files that changed
  during the move. No test covers this shape (`copy_into_tests.rs` has the file-over-folder direction,
  `a_blanket_overwrite_never_replaces_an_archive_directory_with_a_file`, but not folder-over-file).
- **Impact:** The moved folder's files are gone from disk and invisible in the zip. Recoverable only with expert tools
  (`unzip -p archive.zip notes/a.txt`, a hex editor), which a user won't know to try. On a plain copy the same edit
  silently does nothing visible, while reporting success.
- **Suggested fix:** Treat a source directory meeting an archive FILE as a type-crossing clash (prompt or apply the
  shape-aware policy via `resolution_for_clash`, the same as the file-over-folder direction), and count a refused one as
  skipped so a move keeps its source.

### archive-3. Any edit of a zip with non-UTF-8 entry names rewrites every such name as CP437 mojibake, permanently

- **Severity:** high
- **Confidence:** confirmed (traced through `zip` 9.0.1's `ZipFileData::name`, `raw_copy_file_rename`, and the writer's
  UTF-8 flag)
- **Where:** `crates/cmdr-archive/src/mutation/mutator.rs:227`, `apply` (plan) and `:289` (raw copy);
  `~/.cargo/registry/src/*/zip-9.0.1/src/types.rs:121` (`name()`: UTF-8 if valid, else CP437) and `types.rs:352` (the
  UTF-8 flag set on write); the reader's decoding is rc-zip's (`rc-zip-5.4.1/src/fsm/archive.rs:304`, chardetng with
  Shift-JIS detection)
- **Excerpt:**

```rust
227	    for index in 0..src.len() {
228	        let file = src.by_index_raw(index).map_err(MutationError::Zip)?;
229	        let original_name = file.name().map_err(MutationError::Zip)?.into_owned();
230	        let compressed_size = file.compressed_size();
231	        let encrypted = file.encrypted();
232	        drop(file);
...
289	    for entry in &retained {
290	        checkpoint(hooks)?;
291	        let file = src.by_index_raw(entry.index).map_err(MutationError::Zip)?;
292	        writer
293	            .raw_copy_file_rename(file, entry.new_name.clone())
294	            .map_err(MutationError::Zip)?;
```

- **Mechanism:**
  1. A zip made by a legacy-codepage tool (Japanese Windows / Lhaplus in Shift-JIS, Chinese GBK, Russian CP866) stores
     raw name bytes with the UTF-8 flag off.
  2. `file.name()` decodes them as CP437 (the `zip` crate's only fallback), so `original_name` is mojibake (`日本.txt` →
     `“ú–{.txt`-style text). Every retained entry's `new_name` is that string.
  3. `raw_copy_file_rename` writes the string's UTF-8 bytes as the new raw name, and `initialize_local_block` sets the
     UTF-8 flag because the bytes are valid, non-ASCII UTF-8. The original code-page bytes are gone from every entry the
     edit merely kept.
  4. Cmdr's own reader shows the regression for Shift-JIS: rc-zip detected Shift-JIS before the edit and listed
     `日本.txt`; after it, the names are flagged UTF-8 and list as mojibake. For GBK/CP866 every tool that used to
     decode with the right code page (Windows in that locale, `unzip -O`, The Unarchiver) now shows garbage too.
     Shift-JIS second bytes can be `0x5C`, which the CP437 decode turns into a literal `\`, so after the edit the
     reader's sanitizer splits those names into spurious folders.
  5. Side effect: a delete or rename the user picks by the rc-zip name never matches the CP437 name in `plan_new_name`,
     so it silently does nothing while the rewrite still re-encodes everything else.
- **Trigger:** Add, delete, or rename any one entry (or move one out) in a zip made on a non-UTF-8 system. The user
  doesn't need to touch a non-ASCII entry.
- **Guard ruled out:** No encoding check before the rewrite; `non_utf8_name_is_decoded_best_effort`
  (`read/archive_test.rs:412`) only covers browsing. The Info-ZIP Unicode Path extra field (`0x7075`) rides along when
  present, but archives from these tools usually don't carry one.
- **Impact:** Every non-ASCII filename in the archive is corrupted. The bytes are recoverable in principle (CP437 is a
  byte bijection, so mojibake → CP437 → original code page works), but only with a hand-written script.
- **Suggested fix:** Plan and write by RAW name bytes (`file.name_raw()`, and a raw-bytes rename through
  `raw_copy_file_rename_internal`'s equivalent or `raw_copy_file` for unchanged entries), and match deletes/renames
  against rc-zip's decoded name mapped back to the raw bytes by central-directory ordinal; or refuse to edit an archive
  with non-UTF-8 names until that exists.

### archive-4. Moving or deleting a folder in a zip also deletes entries under it that Cmdr never showed or extracted

- **Severity:** medium
- **Confidence:** confirmed (traced end to end; the archives that trigger it are malformed or unusual)
- **Where:** `crates/cmdr-archive/src/mutation/mutator.rs:658`, `plan_new_name` (matches RAW names by prefix);
  `crates/cmdr-operations/src/archive_edit/move_out.rs:218` (delete batch from the selection);
  `crates/cmdr-archive/src/read/index.rs:497` (quarantine), `:565`–`:568` (shadowed files pruned)
- **Excerpt:**

```rust
658	fn plan_new_name(original_name: &str, deletes: &[String], renames: &[(String, String)]) -> Option<String> {
659	    let is_dir = original_name.ends_with('/');
660	    let logical = original_name.trim_end_matches('/');
661
662	    for delete in deletes {
663	        if path_matches_or_under(logical, delete.trim_matches('/')) {
664	            return None;
665	        }
666	    }
```

- **Mechanism:** The reader hides some raw entries: names with a `..` component or more than 256 components are
  quarantined (`index.rs:497`); a file whose ancestor is a file, or a file shadowed by a same-path directory, is dropped
  from the tree (`index.rs:613`, `:626`). The move-out extract only copies what the tree lists. The phase-2
  `{ delete "dir" }` then runs against raw names, and `path_matches_or_under("dir/../x", "dir")`,
  `("dir/a/a/…(300 deep)", "dir")`, and `("dir/f/g", "dir")` (with `dir/f` a file) are all true. Those entries are
  deleted without ever having landed anywhere. F8 on the folder deletes them the same way, though there the user did ask
  to delete the folder.
- **Trigger:** Move a folder out of a zip produced by a buggy or hostile writer (a file and a folder at one path, a `..`
  name). The quarantine list is never shown to the user.
- **Guard ruled out:** `note_source_landed_clean` counts deep SKIPS reported by the copy engine, but these entries never
  enter the copy at all, so the count is zero and the source qualifies for deletion.
- **Impact:** The hidden entries' bytes are lost from the archive. They were unreachable through Cmdr, but other tools
  (or a later Cmdr) could read them.
- **Suggested fix:** Compute the move-out delete set from the reader's index (the central-directory ordinals of the
  nodes that actually extracted) instead of a raw-name prefix, or keep any raw entry under the prefix that has no
  accepted node.

### archive-5. Extracting an encrypted entry from a zip with a duplicate entry name returns another entry's bytes, and a move-out then deletes the real one

- **Severity:** medium
- **Confidence:** plausible (the misalignment is traced in both crates; the end-to-end move-out run is not)
- **Where:** `crates/cmdr-archive/src/read/zip.rs:34`, `ZipHandle` (the ordinal) and `:253`, `run_decrypt_producer`;
  `~/.cargo/registry/src/*/zip-9.0.1/src/read/zip_archive.rs:75` (`SharedBuilder::build`); `index.rs:550`
- **Excerpt:**

```rust
75	        let mut index_map = IndexMap::with_capacity(self.files.len());
76	        self.files.into_iter().for_each(|(file_name_raw, file)| {
77	            index_map.insert(file_name_raw, file);
78	        });
```

- **Mechanism:** rc-zip keeps every central-directory record, so its ordinals run 0..n. The `zip` crate keys files by
  raw name in an `IndexMap`: a duplicate name overwrites the earlier record IN PLACE, so it holds n−1 files and every
  index after the duplicate is shifted down by one. `run_decrypt_producer` calls `by_index_decrypt(rc_zip_ordinal)`,
  which for any encrypted entry past a duplicate opens the NEXT entry. Its CRC is that entry's own, so it verifies and
  streams cleanly under the wrong name (the last entry fails `FileNotFound`). The docs claim "the ordinals align,
  pinned" (`archive_test::zip_crate_ordinals_align_with_rc_zip`), but the pin has no duplicate. A move-out of the
  affected entries lands the wrong bytes, then deletes the originals (the mutator allows deleting encrypted entries; if
  every encrypted entry is moved, nothing encrypted is retained and the edit proceeds).
- **Trigger:** A password-protected zip with two entries of the same name (Python's `zipfile` writes them with a
  warning; some appending tools do too). Extract or move entries listed after the duplicate.
- **Guard ruled out:** CRC verification passes (it's the other entry's CRC); the `EncryptedEntryRetained` refusal only
  blocks keeping an encrypted entry, not deleting one.
- **Impact:** Silent wrong content at the destination; with a move, the real entry's bytes are deleted from the archive.
- **Suggested fix:** Decrypt by data offset (rc-zip's `header_offset`) or by matching raw name plus offset in the `zip`
  crate's metadata, and refuse when the two crates' entry counts differ.

### archive-6. Every rewrite drops the archive comment, any bytes before the zip, and all but the last of a duplicate-named entry

- **Severity:** low
- **Confidence:** confirmed
- **Where:** `crates/cmdr-archive/src/mutation/mutator.rs:285`, `apply`; `zip_archive.rs:75` (as in archive-5)
- **Excerpt:**

```rust
277	    let staged = StagingTemp::mint(archive_path, None);
278	    let temp_path = staged.path().to_path_buf();
279	    hooks.note_pending(&temp_path);
280	    let temp_file = File::create(&temp_path).map_err(MutationError::Io)?;
281	    let mut guard = TempGuard {
282	        path: temp_path.clone(),
283	        armed: true,
284	    };
285	    let mut writer = ZipWriter::new(temp_file);
```

- **Mechanism:** The temp is a fresh `ZipWriter::new` that never receives `src.comment()` or the zip64 extensible data,
  and starts at byte 0, so a self-extractor stub or any prefix before the first local header is gone. `src.len()` counts
  the `IndexMap`, so the earlier copy of a duplicate name is never retained.
- **Trigger:** Any edit of such an archive (an unrelated delete is enough).
- **Guard ruled out:** None in the mutator; `mutator_test.rs` doesn't cover comments, prefixes, or duplicates.
- **Impact:** Archive-level metadata and shadowed entries silently disappear; the entries a user can see are intact.
- **Suggested fix:** Carry `set_raw_comment(src.comment())`; refuse (or warn on) archives with a non-zero data offset or
  with a duplicate name.

### archive-7. Editing a zip reached through a symlink replaces the link with a regular file and leaves the real archive unedited

- **Severity:** low
- **Confidence:** plausible (routing a symlinked `.zip` as an archive is inferred from `std::fs::metadata` following
  links; not run)
- **Where:** `crates/cmdr-archive/src/mutation/mutator.rs:359`, `apply`
- **Excerpt:**

```rust
358	    hooks.note_pending(archive_path);
359	    std::fs::rename(&temp_path, archive_path).map_err(MutationError::Io)?;
360	    guard.disarm();
```

- **Mechanism:** `File::open(archive_path)` reads through the link; the temp is minted beside the LINK; `rename`
  replaces the link itself. The target keeps its old bytes, and the link becomes an independent file.
- **Trigger:** Edit inside `~/Desktop/report.zip`, which is a symlink to an archive elsewhere.
- **Guard ruled out:** No `symlink_metadata` check in `apply` or the routes.
- **Impact:** The symlink is lost, and the edit (including a move-in whose sources were then deleted) lives in a file
  the user may not look at. No bytes are lost.
- **Suggested fix:** Resolve the link (`canonicalize`) before minting the temp, or refuse to edit through a symlink.

## Covered

Read in full:

- `crates/cmdr-archive/CLAUDE.md`, `crates/cmdr-archive/src/mutation/CLAUDE.md`, `DETAILS.md`,
  `crates/cmdr-archive/src/read/CLAUDE.md`
- `crates/cmdr-archive/src/mutation/mutator.rs`
- `crates/cmdr-archive/src/read/name.rs` (non-test part), `read/cache.rs`
- `crates/cmdr-operations/src/archive_edit/CLAUDE.md`, `DETAILS.md`
- `crates/cmdr-operations/src/archive_edit/engine.rs`, `remote.rs`, `move_out.rs`, `copy_into.rs`, `routing.rs`,
  `driver.rs`, `compress.rs`
- Out of slice, read for mechanisms: `crates/cmdr-operations/src/rename.rs` (archive route), `create.rs` (archive
  route), `routing.rs` (into/out/compress routing), `transfer/volume/finalize.rs` (swap + rescue),
  `transfer/volume/cleanup.rs` (`reap_stale_transfer_temps`), `transfer/recovered_name.rs` (header),
  `crates/cmdr-smb/src/volume/mutation.rs` (rename), `crates/cmdr-sftp/src/volume/mutation.rs` (rename),
  `crates/cmdr-fs/src/staging.rs` + `staging/names.rs` (mint, prefix)
- `zip` 9.0.1 source: `read/zip_archive.rs` (builder), `read/zipfile.rs` (`options`, `encrypted`), `types.rs` (`name`,
  local block flags), `write.rs` (`raw_copy_file_rename`, `start_entry`); rc-zip 5.4.1 `fsm/archive.rs` (encoding
  detection)

Read in part:

- `crates/cmdr-archive/src/read/index.rs` (`build_index`, `build_tree`, `ensure_ancestors`, `upsert_dir`)
- `crates/cmdr-archive/src/read/zip.rs` (handle, decrypt producer)
- `crates/cmdr-archive/src/volume.rs` (index/load/open_stream, `node_to_entry`, function map)
- `crates/cmdr-archive/src/apple_double.rs` (xattr admission)
- `crates/cmdr-operations/src/archive_edit/fresh_compress.rs` (`start`, `run`, `produce_direct`, `produce_via_spool`,
  `publish_stage`)
- `crates/cmdr-operations/src/transfer/volume/copy_serial.rs` and `copy_concurrent.rs` (where `note_source_landed_clean`
  fires)
- `apps/desktop/src-tauri/src/file_viewer/open_with_extract.rs` (header and dirs)

## Not covered

- `crates/cmdr-operations/src/archive_edit/fresh_plan.rs`, `fresh_zip.rs`, `fresh_pipeline.rs`, `fresh_validate.rs`,
  `fresh_rows.rs`, `conflicts.rs` past `resolve_effective`'s head, `edit_error.rs`
- `crates/cmdr-archive/src/read/` parsers beyond the zip handle: `tar.rs`, `sevenz.rs`, `extract.rs`, `reader.rs`,
  `source.rs`, `sidecars.rs`, `zip_times.rs`
- `crates/cmdr-archive/src/watch/`, `boundary.rs`, `volume_streams.rs`, `volume_scan.rs`
- `apps/desktop/src-tauri/src/file_viewer/materialize.rs` (only grepped), the MCP archive tools
- The SMB server behavior assumed in archive-1's trigger (delete-pending after a refused `ReplaceIfExists`) wasn't
  checked against Samba or Windows.

## Near misses

- Mutator cancel/crash mid-rewrite: temp+rename with an RAII `TempGuard`; the original is untouched until `rename`.
- Retaining a ZipCrypto entry would drop its flag: refused by `EncryptedEntryRetained` before the temp exists. AES
  entries keep `encrypt_with` metadata in `options()`, and they're refused anyway.
- Zip Slip on extract: `sanitize_entry_name` quarantines `..`, clamps absolute paths, maps `\` to `/`; ArchiveVolume
  never surfaces a link target, so no symlink-then-write escape.
- Quarantine laundering through a `__MACOSX` sidecar: `keep_archive_quarantine` replaces any sidecar quarantine with the
  archive's own.
- Moving a zip INTO itself: the mutator's rename gives `foo.zip` a new inode and size, so the local stamp (size +
  dev/ino) mismatches and the sweep keeps it.
- Move-out with a destination inside the same or another zip: the extract fails at `write_from_stream`, nothing is
  deleted.
- Move-out skip and deep-skip handling: pinned by `move_out_a_lone_skipped_source_stays_in_the_archive` and the
  deep-skip tests.
- Remote pull truncated without an error: the missing EOCD fails `ZipArchive::new`, so the edit fails safe.
- Stale index after Cmdr's own edit: `run_managed_edit` calls `forget_archive_index` and refreshes listings; conflicts
  plan inside the op against the working copy.
- Compress over an existing zip replaces it whatever the conflict policy: deliberate and documented
  (`routing.rs::start_volume_compress`), published through the tracked `StagedWrite` landing.
- Local `reap_sibling_temps` has no age gate, so on a Finder-mounted share it can delete another Mac's in-flight
  `foo.zip.cmdr-tmp-*`: that only fails the other edit (its original stays intact), so it's not a loss.
- Open-with extraction: copies are `0o444` in a per-instance dir, reaped only at the next launch.
