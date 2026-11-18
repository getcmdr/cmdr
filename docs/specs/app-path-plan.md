# App path plan

App paths (`/`-separated and self-rooted: `/Users/x`, `C:/Users/x`, `//server/share/x`, `sftp://u@h:22/srv/x`,
`mtp://…`) travel through the Rust code as `std::path::Path`/`PathBuf`, so on Windows every `join`, `parent`,
`file_name`, `components`, `starts_with`, and `display` applies Windows rules: remote and virtual paths gain `\`, `a:b`
reads as a drive, and drive paths get reparsed. This plan replaces them with a typed app path that the `Volume` trait
takes, so a native path can't go where an app path belongs.

- Issue: [#1225](https://github.com/getcmdr/cmdr/issues/1225) (and the index's share of it,
  [#1232](https://github.com/getcmdr/cmdr/issues/1232)). Part of #942, the Windows port. The live Linux data-safety bug
  it fixes on the way (an action on a non-UTF-8 name misses it or hits a `\u{FFFD}`-named sibling):
  [#1236](https://github.com/getcmdr/cmdr/issues/1236), tracked on its own in case this effort slips.
- Decided by David (2026-11-18): option (a) in #1225, a typed app path in the `Volume` signatures. It's **D6** in
  `docs/specs/windows-port-plan.md` (first written as D5, renumbered in review round 1 because the menu-model branch,
  `worktree-agent-a29c9bd1e9e324d93`, holds its own D5).
- Decided by the lead on David's delegation (2026-11-18): consumers that mean "this machine's local volume" name a
  neutral `cmdr_local::LocalVolume`; POSIX-specific tests keep `LocalPosixVolume`. The Windows drive index's name key
  stays lowercase with the Unicode form untouched.
- The path model this implements: D1 in `docs/specs/windows-port-plan.md`. The grammar it builds on:
  `crates/cmdr-fs/src/volume/path_grammar/mod.rs` and its case table `path_grammar_cases.json`.
- Shape of this doc: `docs/guides/multi-agent-refactors.md` § "The loop".

## Loud rules

1. **macOS and Linux behave byte-for-byte as before at every milestone.** Same listings, same paths on the wire, same
   rows in every store, same sort orders. The characterization suite (milestone 1) is the proof; ❌ never edit a pinned
   expectation to green a conversion. If a pin is wrong, stop and tell the lead. The three deliberate exceptions, each
   its own commit with its own before/after pin (decided by the lead, 2026-11-18: all three stay inside this effort): a
   name that isn't UTF-8 gets a lossless path AND name (§ "Names that aren't UTF-8", milestone 9; Linux and remote
   servers only), an input with a trailing `/` or a `.`/`..` name is normalized, resolved through its volume, or refused
   at the boundary (§ "Normalization" and § "Typed and pasted `..`", milestones 10c/10d), and the operation log roots
   drive and UNC paths correctly (milestone 8, Windows only).
2. **❌ Never lossy past the OS boundary.** A native name or path that the type can't hold losslessly is a typed refusal
   (`NotAnAppPath`, a counted skip in a walk), never a `to_string_lossy` spelling: a lossy path can name a DIFFERENT
   file (a sibling literally called `caf\u{FFFD}.txt`). That includes `FileEntry.name`: it carries the same escaped
   spelling as the path's last name (`name == path.file_name()`, always), because the listing protocols address rows BY
   NAME (§ "Name-addressed protocols"). Only the SHOWN text is lossy (`shown()`, `shownName`, `displayPath`).
3. **The wire doesn't change.** `FileEntry.path`, every IPC DTO, and `bindings.ts` stay `string`. `bindings.ts` is
   byte-identical after every milestone.
4. **No store migrations.** Persisted formats keep their exact serialized shape (§ "Persisted formats" proves none is
   needed). A milestone that finds it needs one stops and asks.
5. **❌ No `From<PathBuf>`, `From<&Path>`, `AsRef<Path>`, `AsRef<OsStr>`, or `Deref<Target = Path>` on the app path
   types, ever**, not even "temporarily". Construction goes through the named constructors in § "The type"; the one
   transition bridge is named, counted, and deleted at the end.
6. **❌ No `std::path` arithmetic on an app path.** Once a body holds an `&AppPath`, its `join`/`parent`/`file_name`/
   `starts_with` are the app path's own. Converting to `&Path` to do arithmetic is the bug this plan removes.
7. **Native paths cross only at named boundaries**: `AppPath::to_native` going down, `AppPathBuf::from_native` coming
   back, each call site in the `app-path-native-crossing` allowlist (milestone 11).
8. **Case sensitivity belongs to the volume, never the path.** App path equality, hashing, and `starts_with` are exact
   (no case or Unicode folding); a folded comparison takes the volume's `NameRules`.
9. **Each milestone lands on its own** with macOS and Linux CI green, through the lead's `pnpm land`. Windows progress
   is measured on the builder (`ssh mami`) per milestone, with the test lists named below.
10. Strip milestone numbers from code, comments, test names, and `reason` strings before landing (the guide's rule).
    This file keeps its own.

## Survey of the current code

Taken 2026-11-18 on `worktree-agent-a576e2e459ad8ffeb` (integration branch plus Windows leg four). Method: codegraph
plus scoped greps; the scripts and raw output were in the planner's scratchpad, the numbers are below. "Prod" means
non-test files; test files are `*_test.rs`, `*_tests.rs`, `tests/`, `test_support`, `testing/`, benches, and examples.

### The `Volume` trait

- `crates/cmdr-fs/src/volume/mod.rs` lines 40–2410: about 120 methods, **61 carry a path**. 54 take one (`&Path`, a
  `&Path` pair, or `&[PathBuf]`) and 7 return one: `root`, `find_stored_spelling`, `read_link_target`, `listing_path`,
  `local_path`, `stands_in_for_own_disk`, `local_backing_file`.
- Path-carrying families: listing (`list_directory`, `_with_cancel`, `_as_source`, `_for_scan`, `index_walk`,
  `listing_path`, `watch_listing`, `listing_watch_coverage`); stat (`get_metadata`, `exists`, `presence`,
  `is_directory`, `entry_kind`, `node_identity`, `find_stored_spelling`, `supported_dates`); links (`read_link_target`,
  `create_symlink`); mutation (`create_file`, `create_directory`, `create_directory_all`, `delete`,
  `delete_with_cancel`, `delete_directory`, `delete_files`, `rename`, `rename_work`, `tally_subtree`, `clear_blockers`,
  `apply_metadata`, `folder_metadata`, `notify_mutation`); xattrs (`read_xattr_many`, `write_xattr`, `remove_xattr`);
  streams (`open_read_stream` and its five variants, `resume_read_stream`, `read_range`, `open_random_reader`,
  `write_from_stream`, `continue_write_from_stream`, `copy_within`, `copy_on_server`, `open_sequential_extract`,
  `extraction_is_sequential`); scans (`scan_for_copy`, `_batch`, `_batch_with_boundary`, `scan_for_conflicts`); space
  and access (`get_space_info_at`, `write_access_at`); `share_link`; `rerooted`; and the three local-identity answers.
- Types beside the trait that carry a path: `FileEntry.path` and `redirect_to_path` (`String`, the wire), `ScanConflict`
  (`String`), `CopyScanResult`'s `source_path` and `BatchScanResult.per_path` (`PathBuf`), `routed.rs` (`PathBuf`),
  `root_echo.rs` (three `PathBuf`s), `mkdir_all.rs` `shallowest_created`, `RemoteRoot` (three `PathBuf`s),
  `InMemoryVolume` (root, link targets, and four per-path override tables).

### Implementors

183 `impl Volume for` blocks. Production: `LocalPosixVolume`, `LocalWindowsVolume`, `SmbVolume`, `SftpVolume`,
`WebdavVolume`, `S3Volume`, `NfsVolume`, `MtpVolume`, `AdbVolume`, `ArchiveVolume`, `GitPortalVolume`, `ResticVolume`,
`InMemoryVolume`, `WedgedVolume` (testing), and the app's wrappers and stand-ins (`UnverifiedSmbMount`,
`agent/suggested_ops/facts.rs`'s `LocalDisk`), plus `cmdr-index`'s `YankedVolume`. The rest are test doubles. Per crate:
`cmdr-operations` 108, the app 33, `cmdr-index` 12, `cmdr-listing` 8, `cmdr-fs` 5, `cmdr-restic` 5, `cmdr-local` 2, and
one each in `cmdr-adb`, `cmdr-archive`, `cmdr-git`, `cmdr-mtp`, `cmdr-nfs`, `cmdr-s3`, `cmdr-sftp`, `cmdr-smb`,
`cmdr-viewer`, `cmdr-webdav`. Most test doubles override two to six methods and delegate the rest to an inner
`InMemoryVolume`.

### Call sites per crate

Lines naming `Path`/`PathBuf`, lines doing path arithmetic (`.join(`, `.parent(`, `.file_name(`, `.components(`,
`.starts_with(`, `.strip_prefix(`, `.ancestors(`, `.extension(`, `.file_stem(`, `.with_file_name(`, `.display(`; some
hits are on strings), lines calling native fs APIs, and lines calling a path-taking `Volume` method; prod / test.

- `src-tauri` (the app): Path 2,515 / 1,582; arithmetic 1,467 / 1,957; native fs 950 / 1,139; volume calls 177 / 629.
- `cmdr-operations`: 1,910 / 4,557; 670 / 3,251; 309 / 2,533; 367 / 2,418.
- `cmdr-index`: 953 / 602; 359 / 917; 291 / 867; 61 / 134.
- `cmdr-fs`: 723 / 735; 454 / 173; 75 / 119; 122 / 246.
- `cmdr-local`: 254 / 297; 30 / 202; 47 / 154; 5 / 166.
- `cmdr-mtp`: 214 / 116; 68 / 28; 18 / 21; 59 / 72.
- `cmdr-git`: 197 / 32; 133 / 65; 60 / 39; 4 / 28.
- `cmdr-smb`: 157 / 749; 49 / 42; 2 / 1; 48 / 613.
- `cmdr-viewer`: 117 / 57; 45 / 80; 19 / 102; 5 / 23.
- `cmdr-listing`: 112 / 238; 17 / 35; 7 / 0; 15 / 53.
- `cmdr-adb`: 101 / 33; 12 / 19; 3 / 2; 22 / 65.
- `cmdr-listing-wasm`: 98 / 20; 18 / 1; 4 / 2; 7 / 11.
- `cmdr-s3`: 96 / 58; 25 / 113; 5 / 4; 26 / 216.
- `cmdr-restic`: 93 / 152; 69 / 129; 3 / 26; 27 / 119.
- `cmdr-sftp`: 93 / 267; 8 / 6; 1 / 2; 18 / 178.
- `cmdr-archive`: 92 / 142; 59 / 74; 39 / 130; 10 / 85.
- `cmdr-nfs`: 75 / 24; 9 / 39; 0 / 47; 12 / 109.
- `cmdr-webdav`: 70 / 48; 16 / 110; 0 / 0; 20 / 139.
- `cmdr-operation-log`: 52 / 29; 25 / 14; 2 / 39; 11 / 1.
- `cmdr-multi-rename`: 35 / 11; 18 / 24; 12 / 24; 0 / 6.
- Small: `index-query` 23 / 0, `cmdr-update-trust` 22 / 0 (native only), `fsevent-stream` 14 / 6 (native only),
  `cmdr-search` 2 / 1, `cmdr-media-meta` 3 / 4. No paths: `cmdr-collation`, `cmdr-sort`, `cmdr-recents`,
  `cmdr-platform-cfg`, `cmdr-brief-columns`.
- Totals: about 8,000 prod lines and 9,800 test lines name `Path`/`PathBuf`. Test literals: `Path::new("/…")` 3,525,
  `PathBuf::from("/…")` 1,262, and 38 scheme literals built as `Path`.
- Helpers already speaking app paths by string: `RemoteRoot::to_remote_path` 139 calls, `to_app_path` 34,
  `display_path_for` 22, `root_anchored` 28, `app_paths::path_under`/`path_is_under` 52, `normalize_remote_path` 15,
  `path_grammar::AppPath::parse` 15, `from_native` 7, `to_native` 8.

### App path or native path: the rule

A `Path` value is an **app path** when it came from, or goes to, any of: a `Volume` method, `FileEntry.path`, an IPC
argument or event, a pane/tab/favorite/recent, an index or operation-log row, a scheme URL, or `Volume::root`. It's a
**native path** only between `to_native`/`from_native` and an OS API, inside `cmdr-local`'s volume bodies, and for files
the app owns itself: the data, cache, and config dirs, SQLite stores, logs, secrets, the updater, crash reports, temp
and scratch dirs, model files, bundled resources, and test fixture dirs before they're handed to a volume. Everything
else is an app path. When a value is both (the local engine's fast paths), it's held as an app path and crosses to
native at the OS call.

167 prod files call native fs APIs AND carry app-path signals. The real clusters, where an agent must decide per value:

- `cmdr-operations` local engine: `transfer/{macos_copy,linux_copy,chunked_copy,copy_strategy,real_image}.rs`,
  `transfer/move_op/{mod,source_sweep}.rs`, `validation.rs`, `local_disk/`, `delete/{cloud_trash,trash,real_image}.rs`,
  `transfer/volume/extract_quarantine.rs`, `scratch_dir.rs`, `in_flight_records.rs`, `in_flight_sweep_surface.rs`,
  `ledger.rs`, `unique_name.rs`, `source_binding.rs`, `archive_edit/`. Today the finalize hands
  `cmdr_local::rename_local_exclusive` a native `C:\…` path, which refuses it: no local copy or move lands on Windows.
- `cmdr-index` local scanner and watcher: `indexing/scanner/walker/bulk_read.rs`, `reconcile/reconciler/dir_read.rs`,
  `watch/`, `transports/local_external/`, `store/paths.rs` (resolves by `&Path`, walks components), `media_index/` and
  `importance/store/` (path `TEXT` columns). Native-only: `drive_index_relocation.rs`, store dirs.
- `cmdr-viewer` `session.rs` (opens a local file) and `markdown_links.rs` (relative link arithmetic, app side).
- `cmdr-git` (git2 opens the repo natively; portal paths are app paths), `cmdr-archive` (`mutation/mutator.rs`, the
  local backing file), `cmdr-restic` `hub.rs`, `cmdr-multi-rename` `names_file.rs`, `cmdr-mtp` `virtual_device.rs`.
- The app: `commands/rename.rs`, `commands/file_system/{stat,path_checks}.rs`, `go_to_path/`,
  `file_viewer/ {materialize,open_with_copies}.rs`, `native_drag/fulfillment.rs`, `about/platform/`,
  `file_system/{watcher,share}.rs`, `file_system/volume/eject/holders/`, `volumes/platform/`, `recents/`, `favorites/`,
  and the MCP `tools/`.
- Native-only app files that look mixed but aren't: `data_wipe/`, `state_recovery/`, `secrets/`, `updater/`,
  `config.rs`, `instance_lock.rs`, `icons/disk_cache.rs`, `analytics/spool.rs`, `crash_reporter/`, `dev_fixtures.rs`.

### Wire types

`FileEntry.path` is a `String`, filled today by `PathBuf::join` then `to_string_lossy().into_owned()`
(`crates/cmdr-local/src/reading.rs` ~332 and `minimal_row`). IPC DTOs carry `String`; the ~40 serde/specta types with
`PathBuf` fields serialize a `PathBuf` as a string too (serde's `PathBuf` refuses non-UTF-8). So the wire is already a
forward-slash UTF-8 string and stays one: the new types implement `Serialize` and `specta::Type` as a plain `string`,
and DTOs that hold `String` today keep `String`.

### Persisted formats

- **Drive index** (`cmdr-index` `entries`): one `name` per row under `parent_id`; paths resolve component by component
  (`store/paths.rs`). No full path stored, nothing to migrate. Its key `name_folded` is the lowercase-Unicode decision.
  Also "rebuild, don't migrate" applies to caches.
- **Media index** (`media_file.path TEXT UNIQUE`) and **importance** (`visits.path`, `path_folded`, user data): full
  path strings.
- **Operation log**: directories as a component tree (`store/dirs.rs`, `reconstruct_dir_path`, scheme roots included)
  plus names. `intern_dir` splits on `/` and drops empty names, so on POSIX it already stores the normalized form (a
  trailing `/` or `//` never reaches a row) and the typed path writes the same rows. ❗ It knows only two roots: a
  `scheme://authority` head kept as one name, else `/`. A Windows `C:/x/y` interns `C:`, `x`, `y` and reconstructs as
  `/C:/x/y`; `//srv/share/x` reconstructs as `/srv/share/x`, a different path that a rollback would hand to a volume.
  Milestone 8 makes a drive or UNC root one name, like a scheme root; POSIX rows keep their bytes (verified in code,
  review round 1).
- **Non-UTF-8 names in stores**: the drive index, the media index, importance, and the operation log hold the lossy
  spelling today (TEXT columns, names from the lossy row or `to_string_lossy`). With the lossless spelling (§ "Names
  that aren't UTF-8") those rows change once: the drive and media indexes are caches (the reconciler sees one remove and
  one add per such name), importance loses the visit history of those few paths, and old operation-log rows for them
  stay unrollbackable, as they are today. No migration; say so in milestone 9's commit.
- **Favorites, tabs, recents**: strings as the user or the app wrote them, possibly with a trailing `/`
  (`favorites/store.rs::normalize_for_dedup` trims it for dedup only). They keep their bytes; `from_wire` at use time
  normalizes, so a stored `/a/` still opens `/a` (pinned in milestone 1).
- **Recents, favorites, settings**: JSON strings (`favorites/store.rs` `path: String`, deduped by
  `normalize_for_dedup`).
- **In-flight records ledger** (`cmdr-operations` `in_flight_records.rs`, JSON, `PathBuf` fields with derived `Ord`):
  local temps the local engine owns and sweeps.
- Why none needs a migration: (1) on macOS and Linux an app path's spelling is the exact string these stores hold today,
  for every path the current code writes (milestone 1 pins "parse then print is the identity" over each store's write
  path), except the non-UTF-8 names above, whose rows were already wrong; (2) no Windows install has any data yet; (3)
  stores keep their `String` fields, and the ledger keeps native `PathBuf`s (it's the local engine's own file
  bookkeeping, swept with `std::fs`).

### Where native paths legitimately live

`cmdr-local` (inside both volumes; `local_windows/paths.rs` is today's one Windows crossing), `cmdr-operations`
`local_disk/` and the local copy strategies, `cmdr-index`'s local walker and watch ingestion, `fsevent-stream`,
`cmdr-update-trust`, the app's own-files modules listed above, and OS integrations that hand a user's file to the OS
(drag, reveal, open with, Quick Look, the services menu, eject holders).

### Other facts that shape the design

- **Non-UTF-8 names: lossy on the wire, raw inside the native walks** (verified in code, review round 1). Linux allows a
  name that isn't UTF-8 (APFS refuses one with `EILSEQ`). Today:
  - The ROW is lossy: `FileEntry.name` and `.path` are `to_string_lossy` spellings
    (`reading_parity_test::a_non_utf8_name_lists_lossily`); fix `fdfa5b58cb` only made the reader `lstat` through the
    raw `OsStr` so the row has its size and dates. So any op the frontend starts ON that row (delete, rename, open, copy
    of the file itself) addresses `caf\u{FFFD}.txt`: it misses the file, or, if a sibling is literally named
    `caf\u{FFFD}.txt`, it acts on THAT file. Two names differing only in their invalid bytes (`caf\xe9`, `caf\xe8`) get
    the same row path. These are bugs today, not a baseline to keep.
  - The native walks are lossless: `cmdr-operations` `scan_walker.rs` (`entry.path()`), `delete/walker.rs`, the move
    source sweep (a ledger of raw `PathBuf`s), `cmdr-local`'s `local_posix/scan.rs` (`WalkDir`), and `cmdr-index`'s
    `std_read_dir`/`bulk_read_dir` (`RawDirEntry.path: PathBuf`) carry the raw `OsStr`. So copying, moving, or deleting
    a FOLDER that holds such a name works today on Linux, and the index stats it. A typed walk that went through a lossy
    or refusing `from_native` would silently drop that child from a copy, which is the regression this plan must not
    ship.
  - Already lossy inside a walk: `scan_walker.rs::walk_cached_entries` (the listing-oracle short-circuit) rebuilds
    children from `PathBuf::from(&entry.path)`, so with a pane open on the folder the scan records the lossy path (a bug
    today); `LocalPosixVolume::scan_for_conflicts_impl` joins the lossy `item.name`. The Windows reader's `child_path`
    joins lossily too (`fdfa5b58cb`, unpaired surrogates).
  - Other backends: NFS skips a non-UTF-8 entry (`volume/query.rs`, `gathered.skipped`); the Google Drive mirror refuses
    a non-UTF-8 component because "a lossy name would match a row that describes a different file"
    (`google_drive/mirror_db.rs`), the same principle as loud rule 2.
- **Name-addressed protocols** (verified in code, review round 2): the frontend and the listing engine address rows by
  `FileEntry.name`, not only by path, so a lossy name is an address too:
  - `resort_listing` (`crates/cmdr-listing/src/operations.rs`) re-maps the SELECTION by name after a sort
    (`names_to_rows`): with `caf\xe9.txt` and `caf\xe8.txt` (both `caf\u{FFFD}.txt` today) the selection can move to the
    other row, and a Delete then acts on a file the person didn't pick (#1236). `set_listing_include_hidden` maps the
    cursor by name and the selection by path.
  - The cursor: `cursorFilename` on `resort_listing`, `set_listing_include_hidden`, `set_listing_name_filter`,
    `find_file_index` (with a look-alike fallback, `row_of_any_spelling`), and persisted in every tab and closed tab.
  - `check_rename_validity` takes `dir` + `old_name` and rebuilds the old path (`PathBuf::from(&dir).join(&old_name)`,
    `rename/validity.rs`); the frontend feeds it `target.originalName`, while the rename itself sends `target.path`.
  - A transfer's `preKnownConflicts` and `destinationName` are source NAMES; the pre-skip matches by name.
  - What doesn't: rename, delete, copy, and every row action send the row's `path`
    (`DualPaneExplorer.getFileAndPathUnderCursor`, `rename-operations.ts::performRename`), and `cmdr-media://` URLs are
    opaque tokens (`mediaUrl(token)`), so no path is ever URL-encoded on that route.
- **`Path`'s `Eq`/`Hash`/`Ord` are component-wise and lenient**: `Path::new("/a/") == Path::new("/a")` and
  `/a/./b == /a/b` (trailing `/`, `.`, and repeated `/` vanish in `components()`), while the OS call still receives the
  raw bytes, trailing `/` included. So today's `HashSet<PathBuf>` dedup merges `/a` and `/a/`, and a bytewise-`Eq` app
  path fed the same unnormalized text through the bridge would split them.
- **MTP pick segments** (`twin_path.rs`): a row path can carry a component of NUL plus an id (`/DCIM/␀4242/IMG_1.JPG`).
  The app path must accept it, `parent` must keep today's answer (the pick), and `twin_path::folder_of` and `shown` stay
  the pick-aware layer on top.
- **`root_anchored`** (`volume/mod.rs` ~2441) accepts relative paths and the root aliases `""`, `.`, `/`, and joins
  anything off-root onto the root. `RemoteRoot::to_remote_path` accepts relative paths too. So relative paths DO reach
  `Volume` methods today (from backends' own code and tests).
- **Ordering**: `PathBuf`'s `Ord` is component-wise (`/a/b` < `/a-b`, `/a/b` < `/a.b`, `/a/b` < `/a b`), a string's is
  bytewise (`/a b` < `/a-b` < `/a.b` < `/a/b`, since space, `-`, and `.` sort below `/`). Within a component both
  compare bytes, so non-ASCII UTF-8 orders the same either way. Six `BTreeMap`/`BTreeSet`s key on `PathBuf`
  (`cmdr-index` `reconciler.rs`, `cmdr-listing-wasm` `folder_sizes.rs`, tests), `in_flight_records` derives `Ord`, and
  233 `HashMap`/`HashSet`s key on `PathBuf`.
- **`LocalPosixVolume`** is named in 262 files (901 lines, 408 `LocalPosixVolume::…` calls): 189 test files, 73 prod
  files (many only in doc comments). The two siblings share `local_folder`, `on_mount`, and `with_listing_hook`; only
  POSIX has `barriers_issued`, `barriers`, `with_barrier_interval`, and `with_sidecar_probe`.

## The type

```rust
// crates/cmdr-fs/src/app_path/mod.rs (pure; builds for wasm32)
#[repr(transparent)]
pub struct AppPath(str);          // borrowed, unsized, like `Path`
pub struct AppPathBuf(String);    // owned, like `PathBuf`; `Borrow<AppPath>`, `Deref<Target = AppPath>`
```

**Invariant** (once the transition bridge is gone): a root, then names joined by `/`, normalized: no empty, `.`, or `..`
names, no trailing `/` except the root's own. The roots:

- `/` (POSIX shape). A valid VALUE on every OS: it's the in-memory volume's and the test fixtures' root. On a Windows
  host it never crosses to the OS (`to_native` refuses it, § "The native boundary") and never enters from the wire
  (`from_wire` parses with the host flavor, which refuses a bare `/x`), so only `parse(_, PathFlavor::Posix)`, the
  in-memory volume, and `app_path!` make one there. Review question 1, answered below.
- `C:/` (drive, letter uppercased; `C:` alone parses to it, `C:x` is refused) and `//server/share/` (UNC, one root, with
  or without its trailing `/`): parsed only under `PathFlavor::Windows`. Under `Posix`, `//server/share/x` normalizes to
  `/server/share/x` and `C:/x` is no app path, as the grammar does today.
- `scheme://authority/` (any RFC 3986 scheme): the root is `scheme://authority`, so `parent` clamps there. An empty
  authority (`scheme:///x`, root `scheme://`) is a valid value, pinned, and no backend mints one. Where a remote VOLUME
  is rooted (`sftp://u@h:22/srv/data`, `mtp://<device>/<storage>`) is deeper and stays `RemoteRoot`'s and the backend's
  business; an archive inside a remote file (`sftp://…/x/a.zip/inner`) is the same scheme path with more names.

A name may hold any character but `/` (and NUL, except as the first character of an MTP pick segment). `\` and `:` are
ordinary in a name: a Linux SFTP server can list `a\b`, and `a:b` must never read as a drive. Drive and UNC paths refuse
`\` anywhere, as the grammar does, since NTFS can't hold one. A name may hold the escape code points of § "Names that
aren't UTF-8".

### Names that aren't UTF-8

Decided in review round 1 (loud rule 2; the rejected alternative was "stay lossy, as today", see the survey for why
today isn't a safe baseline). Ranges settled and the canonical form added in review round 2 (the lead's decision 3).

- **The spelling**: `from_native` decodes each name and spells every unit its decoder rejects as ONE plane-16
  private-use code point. Two alphabets, disjoint:
  - **Byte escapes**, U+10FE00 + the byte (U+10FE80–U+10FEFF): a byte a UTF-8 decoder rejects (always `0x80`–`0xFF`).
    Made by byte-native namers: a Unix host's local disk, SFTP, NFS.
  - **Unit escapes**, U+10E800 + (unit − 0xD800) (U+10E800–U+10EFFF): an unpaired UTF-16 surrogate. Made by
    UTF-16-native namers: a Windows host's local disk, and SMB if `smb2` hands one up (milestone 7 pins what it does).
  - `to_native` maps each back, so the round trip is the identity on every byte or unit.
- **Why these ranges** (validated against Unicode 16 in review round 2):
  - Both sit in Supplementary Private Use Area-B (U+100000–U+10FFFD), so every escape is a valid Unicode scalar value: a
    Rust `String`, valid UTF-8 for serde_json, SQLite `TEXT`, a JS string (one surrogate PAIR, never a lone one, so
    `encodeURIComponent`, `TextEncoder`, and `JSON.parse` all accept it), XML's `Char`, and the macOS and Windows
    clipboards. Neither range touches the plane's two noncharacters (U+10FFFE, U+10FFFF) or any other of Unicode's 66
    noncharacters.
  - Private use has no case mapping, no decomposition, and no canonical combining class, so `to_lowercase`,
    `toLowerCase`, `normalize('NFC'|'NFD')`, and HFS+'s NFD storage leave an escape untouched; `NameRules` folding and
    the frontend's `foldedForCompare` can't merge or split one.
  - Plane 16 is the quietest private-use space: the BMP's U+E000–U+F8FF holds Font Awesome and Nerd Fonts glyphs (people
    put those in folder names), Apple's SFM and Samba's `catia` map NTFS-forbidden characters to U+F001–U+F0FF, and Nerd
    Fonts 3 moved Material Design Icons to plane 15 (U+F0001–U+F1AF0, per its glyph-set wiki, checked 2026-11-18).
    Nothing we know of assigns plane 16.
  - Rejected, PEP 383's `surrogateescape` (U+DC80–U+DCFF) and Markus Kuhn's UTF-8b, the same scheme: lone surrogates
    aren't scalar values, so neither a Rust `String` nor strict JSON can carry them, and a JS string holding one makes
    `encodeURIComponent` throw. Rejected, WTF-8 (Rust's `OsStr` on Windows): it's not UTF-8, so it can't cross IPC or
    sit in a `TEXT` column, and it only covers surrogates, never bytes. Rejected, MirBSD's OPTU-8 (U+EF80–U+EFFF): the
    right idea in the crowded BMP private-use area.
- **One canonical spelling per native name** (review round 2; round one's text was injective one way only). Two rules
  together make `to_native` injective over canonical text, so equal native names have equal app paths and the reverse:
  - `from_native` escapes a code point of EITHER alphabet that a name holds as valid text, via its own units (four byte
    escapes on a byte-native namer, two unit escapes on a UTF-16 one). So decoding gives back exactly what the disk
    holds.
  - A name whose escapes decode to units the decoder would have ACCEPTED (`[U+10FEC3, U+10FEA9]` decodes to `C3 A9`,
    which is `é`) is not canonical: `parse` canonicalizes each name as `escape(unescape(name))` (so that text becomes
    `é`), and `from_normalized` refuses it. Same for a name mixing the two alphabets (`MixedEscape`, refused: no namer
    makes one). PEP 383 leaves exactly this hole open (a string of surrogates that spells valid UTF-8 encodes to the
    same bytes as the plain text), so two spellings could name one file and `Eq`, the dedup sets, and the operation log
    would split them.
  - The properties, in milestone 3: `from_native(to_native(s)) == canon(s)` and `to_native(canon(s)) == to_native(s)`
    for arbitrary strings; `from_native` then `to_native` is the identity on arbitrary bytes and arbitrary `u16`s; two
    names differing only in invalid bytes get different spellings; a name holding a literal escape code point
    round-trips.
- **Text a person types never holds an escape.** `from_user_input` and `ChildName::from_typed` (rename, new file, new
  folder, the transfer's destination name) escape any escape-range code point as LITERAL text (its own UTF-8 bytes as
  byte escapes), so typed text always means what it says, and the escape syntax can't be typed or pasted into an
  address. Round one's "accepted edge" (typed escape code points read as raw bytes) is reversed. Only backend-spelled
  text (`from_wire`, `from_normalized`) carries live escapes.
- **Cross-namer copies**: a name holding escapes of an alphabet the destination can't store (byte escapes to NTFS, SMB,
  WebDAV, or S3; unit escapes to a Unix disk, SFTP, or NFS; any escape to a UTF-8-only protocol) is a typed per-item
  refusal at the destination (`NativeRefusal::ForeignEscape` locally, `NotRepresentable` from a backend), counted and
  reported on the op's summary, ❌ never written as the escape code point or as U+FFFD. Today the volume engine writes
  the lossy name, and can land it over a real `caf\u{FFFD}.txt`. Landing it under its shown name is a choice for a
  person to make later (a conflict-style prompt), not this plan.
- **What people see**: every escape renders as U+FFFD, exactly what today's row shows. Rust: `shown()` and
  `shown_name()`. Frontend: `shownName(name)` and `displayPath(text)` (which `shownPath`, `t()`'s params, and Copy path
  all pass through). § "The escape on the wire and in the frontend" lists every stop.
- **The wire stays `string`** (`bindings.ts` unchanged); only the bytes of a path or name holding such a name change,
  and those rows never reached their file before.
- **Who makes escapes**: `cmdr-local` (both readers and every walk that hands a path to a typed API), the bridge (so the
  flip can't lose a raw path), and any network backend whose protocol carries raw names (SFTP and NFS bytes, SMB
  UTF-16): each one escapes or skips with a typed count, ❌ never `from_utf8_lossy` (milestones 6 and 7 decide per
  backend; NFS skips today and may keep doing so).
- macOS never produces an escape for its own disks (APFS and HFS+ refuse non-UTF-8, `EILSEQ`); Linux, Windows (unpaired
  surrogates from WSL or old tools), and remote servers do.
- **Windows, ran on NTFS** (review round 3): `a<D800>b` and `a<D801>b` create side by side; `std::fs::read_dir` and
  `FindFirstFileW` both hand back the exact units, and both names' `to_string_lossy` is `a\u{FFFD}b`, so #1236's
  collision is live on Windows today too (the reader's `child_path` is lossy). A plain path reaches them as well as a
  verbatim one; renaming one to `a<DC00>` and deleting both touched only the named file, and `canonicalize` keeps the
  lone unit. A name that is the literal scalar U+10E800 (the first unit-escape code point) creates and lists back as
  that scalar, so the "literal escape" canonical rule above is a real Windows case, not a theoretical one.

### The escape on the wire and in the frontend

Review round 2 followed the escaped string to every stop. Byte-identical for every path and name that's valid UTF-8 and
holds no plane-16 private-use code point: the escape function is the identity there (property-tested in milestone 3), so
99.99% of rows, every persisted value, and every fixture keep their bytes.

**The wire.** `FileEntry.path` and `FileEntry.name` both carry the escaped spelling (loud rule 2); every IPC string that
holds a path or name (`cursorFilename`, `selectedPaths`, `preKnownConflicts`, `oldName`, operation events, the operation
log's rows) carries what the backend spelled. Until now those strings held `\u{FFFD}` in place of each invalid byte
(`to_string_lossy`). `bindings.ts` is unchanged: every field stays `string`.

**Persisted values** made before the change hold the lossy spelling for those few names. Each keeps its bytes (no
migration, loud rule 4) and behaves as it does today, which is to point at a lossy name:

- **Tabs and closed tabs** (`path`, `cursorFilename`): a tab inside a non-UTF-8-named folder reopens at the nearest
  ancestor that exists, as it does today; a lossy `cursorFilename` finds no row, or, through `row_of_any_spelling`, a
  unique look-alike. Accepted: the cursor is a convenience, and nothing acts on it unseen.
- **Favorites and recents** (go-to-path's `RECENT_PATHS`, recents): an old entry naming such a folder opens what it
  opens today (nothing, or the `\u{FFFD}` sibling); re-adding the favorite stores the escaped path. Accepted.
- **The drive index and media index**: caches; one remove and one add per such name, then convergence (pinned in 10b).
- **Importance** (`visits.path`): the visit history of those few paths starts over. Accepted.
- **The operation log**: old rows for those names stay unrollbackable, as they are today; new rows roll back.
- **Ask Cmdr chat transcripts**: hold whatever text the agent saw (shown form, below); nothing re-addresses from them.

**Display.** Every surface renders an escape as U+FFFD; ❌ never the raw private-use glyph (a box, or nothing):

- Through `displayPath` (which every shown path already uses: the breadcrumb, tooltips, dialogs, toasts, the
  operation-log viewer, `useShortenMiddle`): it maps escapes to U+FFFD over ANY text, since a plane-16 code point never
  means anything in prose, before `shownPath`.
- `t()`: maps escapes in EVERY param, not only `pathParam`s (a name param like `{name}` in "Delete {name}?" is as
  likely), for the same reason.
- `shownName(name)` (new, `lib/path`): the listing's name column (Full and Brief), the Brief columns' text-width measure
  (`get_brief_column_text_widths` measures in Rust: it measures `shown_name`), the rename editor's prefill, search
  results, the selection summary, drag images, and the native menu labels the backend builds (`shown_name` in Rust).
- Copy path: `copiedPath` passes through `displayPath`, so the clipboard gets the U+FFFD text a person saw. It can't
  round-trip to the file (no text clipboard can hold a raw byte), as today. Copy FILES and drag-out hand the OS file
  URLs built from `to_native` (percent-encoded raw bytes in `text/uri-list`, `NSURL` from the native path), so other
  apps get the real file: built from the app path, ❌ never from the shown text.
- Logs and crash reports: `Display` is the escaped wire spelling inside `LogPrivate`; fine for a log, and reports redact
  paths anyway.
- **The catch-all**: a Linux E2E (`desktop-e2e-linux`, milestone 9) lists a fixture folder holding `caf\xe9.txt`, opens
  rename, the delete dialog, the operation log, and search on it, and asserts the DOM's text holds no code point in
  U+10E800–U+10EFFF or U+10FE80–U+10FEFF and shows `caf\u{FFFD}.txt`. A sink that forgot `shownName` fails it.

**Frontend arithmetic.** `lib/path` splits on `/` and slices at `/` boundaries, so an escape (one code point, a
surrogate pair in UTF-16) can't be cut there. Checked, each safe: `parsePath`/`normalizePath`/`joinPath` (split and join
on `/`), `foldedForCompare` (private use has no case or decomposition), `pathRelativeTo` (slices at the ancestor's
length, which ends on a code-point boundary), sorting (the backend sorts; `localeCompare` on favorites puts escapes in
one consistent spot), and URL encoding (no path is URL-encoded: `cmdr-media://` carries tokens; a scheme path's `%XX` is
the backend's). Not safe: `shortenMiddle` slices by UTF-16 unit and can cut ANY surrogate pair, an emoji today (a
separate live bug, reported to the lead in review round 2); it runs on shown text, where every escape is already U+FFFD
(BMP), so the escape adds no new case, but the emoji one is live. The shared case table gains escaped rows (milestone
3): join, parent, basename, `shown` (U+FFFD), `normalized` (canonical), and a non-canonical row (`[U+10FEC3, U+10FEA9]`
normalizes to `é`), written as JSON `􏻃`-style surrogate-pair escapes so the file stays ASCII.

**Input.** Typed text never carries an escape (§ "Names that aren't UTF-8"): go-to-path, rename, new file and folder,
the transfer destination, and the CLI go through `from_user_input` / `ChildName::from_typed`. Paste and drop of FILES
arrive as native paths or file URLs and cross at `from_native` (escaping). Deep links arrive as text: `from_user_input`.
Rename's "unchanged" check compares the edited text with `shownName(originalName)`, ❌ never `originalName` (an escaped
original never equals its prefill, so Enter would rename `caf\xe9.txt` to `caf\u{FFFD}.txt`); a real edit lands the
typed text, U+FFFD included, which is the honest outcome of renaming what the person saw (and the usual way to fix such
a name). `validateFilename`'s 255-byte rule measures the escaped name by its NATIVE length (one byte per byte escape),
❌ never `TextEncoder` over the escape (four bytes each): a `nativeNameLength` helper next to `shownName`.

**Agent surfaces** (the MCP server's tool outputs, Ask Cmdr's context). A model can't be trusted to echo a plane-16 code
point back intact, and a mangled echo of `caf<escape>.txt` as `caf\u{FFFD}.txt` would hit the sibling. So:

- Outputs show a path or name holding an escape in its shown form, plus an `unaddressable: true` flag on that entry
  (`docs/` for the MCP tools say what it means: act on it in the app).
- An input path whose name holds U+FFFD is checked against its folder (one listing, only when U+FFFD is present): if a
  sibling's `shown_name` equals it while its real name differs, the tool refuses typed (`AmbiguousName`), even when a
  file with the literal name exists, since the agent saw both rows as one spelling. Milestone 10d.

### Normalization

What `parse` and `from_wire` do, each picked so it can't change which file a path names, or, where it can, changing it
deliberately toward the safe answer with a pin:

- **Repeated `/`**: collapsed. POSIX and every backend read `a//b` as `a/b` (Linux and macOS give a leading `//` no
  special meaning). Under `Windows`, a leading `//` is the UNC root, never collapsed. ❗ S3 keys are the exception:
  `a//b` and `a/b` are two objects, so the S3 key layer never goes through `parse` (milestone 6).
- **`.` names**: dropped (always the same directory).
- **`..` names**: `from_wire` REFUSES one (`NotAnAppPath::DotName`) instead of resolving it lexically. On a local POSIX
  volume the OS resolves `/a/link/../x` physically, to the link target's parent, and a lexical resolve would retarget an
  IPC delete or MCP write to `/a/x`, a different file. Lexical resolution stays only where it is the safety rule
  (`RemoteRoot`, which then refuses off-root). Text a person or an agent types goes through the volume-aware resolver in
  § "Typed and pasted `..`" (review round 2 replaced round one's "keep `from_user_input`'s lexical answer"), so
  `from_wire`'s refusal only ever meets a frontend or backend bug, and logs as one.
- **A trailing `/`**: dropped. This changes which file a POSIX call touches when the path is a symlink to a folder:
  `lstat("/a/link/")` follows the link, `lstat("/a/link")` doesn't, so today a wire path with a trailing `/` stats, and
  a recursive delete walks, the link's TARGET. Dropping it names the link, the safe direction (deleting a link must
  never empty its target). Rows never carry a trailing `/` (they're built by `join`), so only wire input changes;
  milestone 1 pins today's answer on Linux and macOS and milestones 10c/10d flip it deliberately.
- **Unicode form**: never touched. `café` in NFC and NFD are two values; on APFS and HFS+ both open one file, on ext4
  two. Equality is exact, so a typed NFC path and a listed NFD path compare unequal, as `PathBuf`s do today; a folded or
  normalization-insensitive compare is the volume's `NameRules` business.
- **Case**: never folded, except the drive letter, uppercased (Windows reads `c:` and `C:` as one drive and the OS
  reports either). A UNC server and share keep their case as typed or reported (decided by the lead, 2026-11-18,
  question 6): `Eq` and `Hash` stay exact, and every ROOT match (the resolver picking a volume for a path, `starts_with`
  against a volume root, `RemoteRoot`-style prefix checks, `same_root`, and the frontend's `isPathOnVolume` for a
  `//server/share/` root) compares the server and share components with Unicode simple case folding, since both are
  case-insensitive on the wire (DNS and NetBIOS names; SMB share names). Below the root, case is the volume's
  `NameRules`, as everywhere. Ran (review round 3): `\\?\UNC\LOCALHOST\d$\x` opens `\\localhost\D$\x`, and a lowercase
  letter in `\\?\d:\x` opens the drive. ❗ Not run, and not volume-wide: Windows 10 1803+ lets a FOLDER be
  case-sensitive (`fsutil file setCaseSensitiveInfo`, what WSL sets on folders it creates), so one NTFS volume can hold
  `A` and `a` side by side in that folder. The builder refused the switch (no WSL feature installed). A per-volume
  `NameRules` folds the two together there, and the drive index's lowercase key collides; Windows plan task 35 makes it
  a per-directory property of the local Windows volume (question 13).

### Typed and pasted `..`

Review round 2. People type and paste `..` often (`../foo` from the pane, a path copied from a build log like
`/opt/app/lib/../share`), and agents do too. Refusing it is wrong UX; resolving it by text alone is wrong semantics
across a symlink on POSIX, where the OS resolves `/a/link/..` to the link TARGET's parent. The rule is "`..` means what
the volume holding the path means", answered by the volume at the input boundary, once, before the path becomes an
`AppPathBuf`:

- **One resolver**: `resolve_typed(text, base: Option<&AppPath>) -> Result<AppPathBuf, TypedPathRefusal>` in the app's
  input layer (go-to-path's module), called by go-to-path, the CLI's path arguments, deep links, and the MCP tools' path
  inputs. It expands `~`, parses with `from_user_input`'s flavor rules but KEEPS `.`/`..` names as tokens, picks the
  volume for the path (the resolver, `resolve_path_to_volume`), then walks the names left to right holding a normalized
  prefix.
- **A relative input's leading `..`s** (`../foo`, `../../x`) step up from the pane's folder the way the pane's own `..`
  row and Backspace do (`parentInVolume` on the pane path, lexical: "up from where I am", which is also what a shell's
  `cd ..` does). Clamped at the volume root, as the `..` row is.
- **Every other `..`** (in an absolute input, or after a name: `x/../y`) asks the volume:
  `Volume::parent_for_dot_dot(&AppPath) -> Result<AppPathBuf, VolumeError>`, a new trait method with a default of the
  lexical parent. POSIX local disks (`LocalVolume` on Unix) and SFTP override it: if the prefix's last name is a symlink
  (`lstat`, or SFTP `LSTAT`), resolve that ONE link (`read_link_target`, relative to its folder, through `from_native` /
  the server's spelling) and take the target's parent; otherwise the lexical parent. Windows' local volume keeps the
  default on purpose: Win32 resolves `..` lexically before the filesystem sees it (`GetFullPathNameW`), so lexical IS
  what Windows means. Ran (review round 3): through a junction AND a directory symlink, the plain `lk\j\..\marker` and
  `lk/j/../marker` opened `lk\marker`, never the target's sibling, and `del lk\j\..\victim` removed `lk\victim` and left
  `real\victim`; Explorer and `cmd` read it the same way, so lexical is also the safe answer for a delete or a move
  there. It has to happen in the app: the verbatim form `to_native` emits refuses `..` outright (`ERROR_INVALID_NAME` on
  a drive, `ERROR_BAD_PATHNAME` over UNC; ran). SMB keeps it too (the protocol has no `..`; a Windows client resolves
  paths lexically before sending, whatever the server). S3, MTP, ADB, archives, and the virtual volumes have no links,
  so lexical is exact.
- **Faithful by default**: only a `..` that crosses a symlink changes the text; every other part stays as typed. That's
  why go-to-path refused `canonicalize` (`apps/desktop/src-tauri/src/go_to_path/CLAUDE.md`: it needs the whole path to
  exist, and rewrites every link in the shown path); this resolves one link only where `..` makes it matter, and a
  prefix that doesn't exist is no link, so the nearest-ancestor case keeps working.
- **What a person sees**: go-to-path lands where `ls` (or Finder, or Explorer) would put them for that text; the preview
  line shows the resolved path when it differs from the typed text, using the existing `info` tone. A `..` above a
  volume's root clamps there for a person (go-to-path, the CLI) and is refused typed for an agent (`OffRoot`), the MCP
  tools' rule for anything ambiguous. A volume that times out answering (a hung mount) refuses with the two-layer
  timeout's answer, ❌ never a lexical guess.
- **Pins**: milestone 1 (g) pins today's lexical answers; milestone 10c lands this as loud rule 1's second exception,
  its own commit, flipping (g) where a symlink is crossed, with new cells: `/x/link/../y` on Linux and macOS lands on
  the target's sibling; `C:/x/link/../y` on Windows (a junction) lands on `C:/x/y`; `../y` from a pane inside `/x/link`
  lands on `/x/y`; a missing prefix still reaches its nearest ancestor.

### The native boundary

`to_native(&self) -> Result<PathBuf, NativeRefusal>` and
`from_native(&Path, PathFlavor) -> Result<AppPathBuf, NotAnAppPath>`, both typed (loud rule 2; ❌ no `Option` that a
caller turns into "not found"):

- **Scheme path**: `NativeRefusal::NotLocal`.
- **Wrong flavor**: a POSIX-shaped path on a Windows host is `WrongFlavor`, ❗ never `\x`: Windows reads a rooted path
  with no drive as the CURRENT drive's root, so `/Users/x` would open whatever drive the process cwd is on
  (`GetFullPathNameW("/x")` answered `C:\x`; ran, review round 3). A drive or UNC path on a POSIX host is `WrongFlavor`
  too.
- **Characters NTFS and Win32 can't hold** (in the NAMES of drive and UNC paths): `:`, `*`, `?`, `"`, `<`, `>`, `|`,
  control characters, and NUL are `ForbiddenCharacter`. ❗ `:` above all: through `\\?\` AND through a plain path,
  `C:/x/a:b` writes the alternate data stream `b` of file `a`, so a write lands INSIDE another file and no listing shows
  it; if `a` doesn't exist, the write creates an empty `a` to hold the stream; folders take streams too, and a trailing
  `a:` is `ERROR_INVALID_NAME` (ran, review round 3). NUL is refused on POSIX too (an MTP pick never reaches
  `to_native`). ❗ The check covers names only, never the UNC server: `//[::1]/share/x` is a valid root, and both
  `\\[::1]\D$\x` and `\\?\UNC\[::1]\D$\x` open the share (ran, review round 3).
- **A UNC server with a port** (`//host:445/share`, `//host@445/share`) is `NativeRefusal::NoPortOnUnc`: the Windows 10
  SMB client takes no port in a path (both spellings answer `ERROR_BAD_NETPATH`, ran, review round 3), so that text
  belongs to the app's own SMB backend (`smb://host:445/share`), and the resolver routes it there.
- **Trailing dots and spaces, and reserved device names** (`CON`, `NUL`, `COM1`, `con.txt`, `lpt1 .x`, `CONIN$`): NOT
  refused here. Through `\\?\` they reach the exact file, the only way to open or delete one that WSL or a Linux tool
  made. Creating one is refused earlier, by `NameRules` (Windows plan task 14). Pinned both ways: `to_native` of
  `C:/x/CON` and `C:/x/a.` is the verbatim path, and create and rename refuse those names. Ran on NTFS (review round 3):
  each of those six names creates, lists, reads, and deletes through `\\?\`.
- **❗ The plain form names a DIFFERENT file** (ran, review round 3), which is why `to_native` is verbatim-only and why
  a second, refusing form exists (next bullet). With `dup` and `dup.` side by side, a plain `D:\x\dup.` READ `dup` and a
  plain `remove_file` DELETED `dup`, leaving `dup.`; a plain `x\dot.` with no `dot` beside it is `NotFound`; a plain
  write to `x\nul.txt` succeeded and wrote nothing anywhere (the `NUL` device: silent data loss); a plain `con.txt` is
  `\\.\con`; and moving `rb.` to the Recycle Bin by its plain path (.NET's `FileSystem.DeleteFile`, over
  `SHFileOperation`) recycled `rb`.
- **`to_native_plain(&self) -> Result<PathBuf, NativeRefusal>`** (new, Windows only; on POSIX it's `to_native`): the
  drive or UNC path WITHOUT `\\?\`, for the APIs that can't take a verbatim path: the shell (the Recycle Bin through the
  `trash` crate, `ShellExecuteExW` for open and open with, `SHOpenFolderAndSelectItems`, drag-out's `CF_HDROP`), a child
  process's working directory and arguments (a terminal, an editor, "open with"), and any third-party crate that opens
  plain paths. It refuses typed (`NotPlainSafe`) every path whose plain form wouldn't name the same file: a name ending
  in `.` or space, a reserved device name with or without an extension, and a total past 259 units (a child's working
  directory past 260 is `ERROR_DIRECTORY`, ran; shell APIs don't take the long-path opt-in, documented). A refusal is a
  typed per-item answer ("Cmdr can't hand this name to Windows"), ❌ never a fallback to stripping `\\?\` by hand
  (`dunce::simplified` and the like), which is exactly the wrong-file hazard above. UNC working directories are another
  limit: `cmd.exe` started in `\\localhost\D$\x` falls back to `C:\Windows` with a warning (ran), so "open a terminal
  here" on a UNC path needs `pushd` or a terminal that takes UNC. Its call sites join `app-path-native-crossing`'s
  allowlist (milestone 11). This plan ships the type and its reasons; each consumer's Windows plan task writes the words
  a person sees (question 14).
- **Length**: a pure fence at 32,767 UTF-16 units including the `\\?\` prefix is `TooLong`, but NTFS stops earlier: a
  `create_dir` at 32,750 units answered `ERROR_FILENAME_EXCED_RANGE` (206, `InvalidFilename`) where 32,649 worked (ran,
  review round 3, 100-unit steps). So the OS's 206 maps to the typed `NameTooLong` too, ❌ never a hard-coded exact
  limit. On POSIX the OS answers `ENAMETOOLONG` as today. ❗ The builder has `LongPathsEnabled` on (its setup script),
  user machines default it off: a plain path past 260 that a third-party crate opens works on the builder and fails for
  users (an exe without the opt-in got `ERROR_PATH_NOT_FOUND` from `GetFileAttributesW` on a 300-unit plain path, while
  `std::fs` added `\\?\` itself and succeeded; ran). Cmdr's `windows/app.manifest` declares `longPathAware`, which helps
  only with the registry switch on.
- **Escaped names**: decoded back to raw bytes (Unix) or surrogate units (Windows).
- **`from_native` refuses**, typed: a relative path, a drive-relative `C:foo` AND a bare `C:` (both mean that drive's
  CURRENT directory to Win32: `GetFullPathNameW("D:foo")` answered `D:\foo` only because `D:`'s cwd was its root; ran,
  review round 3; `parse` and `from_user_input` keep reading a typed `C:` as `C:/`, which is what a person means),
  `\\.\` device paths, `\\?\Volume{…}` and `\\?\GLOBALROOT\…`, a UNC server of `?` or `.`, and a `.`, `..`, or empty
  name inside a verbatim path. Win32 doesn't treat those as literal names: NTFS refuses them (`\\?\D:\x\.\y` is
  `ERROR_PATH_NOT_FOUND`, `\\?\D:\x\\y` and a verbatim `..` `ERROR_INVALID_NAME`, a verbatim `..` over UNC
  `ERROR_BAD_PATHNAME`, `\\?\D:` without its `\` `ERROR_INVALID_PARAMETER`; ran, review round 3), so the refusal only
  types what the OS would say. Today's grammar answers `None` for most of these; the conversion only types the reason.
- **8.3 short names are aliases** (ran on `C:`, where 8.3 creation is on, as it is by default on a Windows 10 system
  drive; the builder's `D:` has it off; review round 3). `from_native` is pure, so it can't tell `C:/x/LONGFO~1` from
  `C:/x/Long Folder Name`: `read_dir` through the short spelling returns children under it (a second app path for every
  file inside), and `canonicalize` gives back the long one. The listing never shows a short name, so a row never holds
  one; the inbound crossings that didn't come from a listing can (an OS-reported path like `%TEMP%` for an account name
  past eight characters, a drop from a legacy app, a CLI argument, a deep link, a typed or pasted path). Two rules:
  - The Windows local volume's inbound native crossings (`from_native` callers outside a listing walk) expand short
    names with `GetLongPathNameW` on the longest existing prefix first, so an OS-reported path lands on the spelling the
    listing shows.
  - ❗ A destination NAME can alias a different file: through `LONGFI~1.TXT`, a `create_new` answered `AlreadyExists`, a
    truncating write REPLACED `Long File Name.txt`'s bytes, and a rename renamed it (ran). A source literally named
    `LONGFI~1.TXT` (from SFTP, a zip, a Linux disk) copied into that folder passes a name-folded conflict check
    (`NameRules` folding doesn't relate the two) and lands on the person's file. So on Windows the destination conflict
    scan and the landing's collision answer read the STORED name (`find_stored_spelling`, `cFileName` from
    `FindFirstFileW`), and a stored name that differs from the destination name beyond the volume's folding is a
    conflict on the stored name, ❌ never an overwrite through the alias. Milestone 10a, with a Windows-only cell.

**Operations** (all on `&AppPath`, none allocate unless they return an owned path):

- `as_str()`, `root() -> &AppPath`, `kind() -> PathKind`, `is_root()`.
- `parent() -> Option<&AppPath>` (a slice; `None` at the root), `file_name() -> Option<&str>`, `names()` (an iterator of
  `&str`).
- `join(&self, name: ChildName) -> AppPathBuf` is THE join (one plain name, so it can't climb or replace).
  `join_relative(&self, rel: &RelativeAppPath) -> AppPathBuf` for a validated multi-name tail (the remote-root and
  `rebase` cases). ❌ No `join(&str)`: `Path::join("/x")` replacing the base is exactly the surprise to remove.
- `starts_with(&self, ancestor: &AppPath) -> bool` (whole names, exact),
  `strip_prefix(&self, ancestor) -> Option<&RelativeAppPath>`, `relative_to` as the same thing named for readers; folded
  twins take `&NameRules`.
- `rebase(&self, old_root, new_root) -> Option<AppPathBuf>` (replaces `app_paths::rebase`).
- `to_native` and `from_native`: § "The native boundary" (today's `path_grammar` bodies, moved, with typed refusals).
- `Display`: the wire spelling (for logs, wrapped in `LogPrivate` as today). `shown()`: what a person reads, the ONE
  formatter (Windows `\`, picks taken out through `twin_path::shown`), the twin of the frontend's `shownPath`.
- `Eq`, `Hash`, `Ord`: **component-wise, defined as POSIX `Path::components` over the text**: the root first (`/` as
  `RootDir`, which sorts below any name, so `/x` < `sftp://…` as `PathBuf` has it), then the names, skipping empty and
  `.` names and keeping `..`. On a normalized value `Eq` is the same as comparing bytes; on a bridge value made from an
  unnormalized `PathBuf` (`/a/`, `/a/./b`) it still equals what `PathBuf` equals, so a `HashSet` keyed on the typed path
  dedups exactly as today (survey). Within a name, bytes compare as `OsStr` bytes do. The one known divergence: an
  escaped non-UTF-8 name (§ "Names that aren't UTF-8") sorts by its escape code point, above every raw byte, where
  `PathBuf` sorted the raw byte; it's a consistent total order, and no path order is persisted (the `BTree*`s are
  in-memory), so it's accepted.

**Construction**, each named and greppable, ❌ no `From` impls:

- `AppPathBuf::parse(text: &str, flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath>`: normalizes per §
  "Normalization" (a `..` name is refused, not resolved).
- `AppPathBuf::from_wire(text: &str)`: `parse` with `PathFlavor::HOST`, for IPC arguments and stored strings.
- `AppPathBuf::from_user_input(text: &str, flavor)`: typed or pasted text with NO `..` left (separators turned on
  Windows, `.` dropped, escape-range code points escaped as literal text); a `..` is refused here, since resolving it
  needs a volume: the app's `resolve_typed` (§ "Typed and pasted `..`") tokenizes first and calls this on each resolved
  prefix. `ChildName::from_typed` is the same rule for one name.
- `AppPath::from_normalized(text: &str) -> Result<&AppPath, NotAnAppPath>`: zero-copy validation for a string that is
  already canonical (a `FileEntry.path` a backend built); `FileEntry::app_path()` uses it.
- `AppPathBuf::from_native(...)`, `RemoteRoot::to_app_path(...)`, `Volume::root()`, and the derivations above.
- Tests only (`any(test, feature = "testing")`): `app_path!("/a/b")` returns a `&'static AppPath`-like value, panicking
  on a bad literal, so the 4,800 test literals convert by sed.
- **The transition bridge, deleted in milestone 11**: `AppPath::legacy_unchecked(&Path) -> Cow<'_, AppPath>` and
  `AppPath::as_std_legacy(&self) -> Cow<'_, Path>`. Both exist so the trait can flip without a semantic change anywhere;
  each call is counted by the `app-path-std-bridge` check and goes to zero crate by crate.
  - `legacy_unchecked` wraps UTF-8 text as is, borrowed: unvalidated, unnormalized (a trailing `/`, `//`, `.`, `..`, and
    a Windows `\` all survive). Non-UTF-8 input is escaped (owned), ❗ never lossy: the local engine's walks hand raw
    `PathBuf`s from `read_dir` to code that will call `Volume` methods, and a lossy bridge would make those calls miss
    the file, or hit a sibling named with U+FFFD. Rejected: "lossy, as the wire does" (the planner's first draft).
  - `as_std_legacy` borrows when the text holds no escape, and decodes to the exact bytes (owned) when it does.
  - The checkable proof that the bridge never normalizes (milestone 3): a property test over arbitrary `OsString`s
    (Unix: any bytes, invalid UTF-8, `//`, trailing `/`, `.`, `..`, `\`; Windows: any UTF-16 units, unpaired surrogates,
    `\`, `C:foo`, `\\?\`) that `legacy_unchecked(p).as_std_legacy()` has the same `as_os_str()` BYTES (Unix
    `as_bytes()`, Windows `encode_wide()`) as `p`. ❌ Never assert it with `Path ==`: `Path`'s equality ignores a
    trailing `/` and `.`, so a normalizing bridge would pass.

**Placement**: `cmdr_fs::app_path` (new module), with `path_grammar` becoming its parser and keeping the frontend twin's
case table. The current `path_grammar::AppPath` struct (`AppRoot` plus `Vec<String>`) folds into the new type;
`AppRoot`, `PathFlavor`, and `PathKind` stay. `ChildName::under(&AppPath) -> AppPathBuf` replaces today's
`under(&Path)`.

### One file, several app paths (Windows)

Review round 3, ran on the builder. One folder on `D:` opened under all of: `D:/x`, `//localhost/D$/x`,
`//LOCALHOST/d$/x`, `//127.0.0.1/D$/x`, `//[::1]/D$/x`, `//0--1.ipv6-literal.net/D$/x`, a `subst` drive `Q:/`, a
junction and a directory symlink pointing at it, and (on `C:`) an 8.3 short spelling. App path `Eq` keeps them apart on
purpose (loud rule 8: equality is exact; the root fold only picks a volume), and no store needs them equal. What must
never decide "same file" or "inside" by app path:

- **Destination inside source** (`validation.rs::validate_destination_not_inside_source`) compares native `canonicalize`
  results today, which on Windows folds `subst`, junctions, symlinks, and 8.3 into the drive path (ran), but leaves a
  loopback or mapped share as `\\?\UNC\localhost\D$\x` (ran). So copying `D:/x` into `//localhost/D$/x/sub` passes the
  guard and recurses until the disk fills. 10a keeps the guard on canonical NATIVE paths on both sides (`to_native`,
  then `canonicalize`), ❌ never one canonical native path against an app path, and the UNC-alias gap goes to task 35's
  `file_identity` (volume serial plus 128-bit file id, an ancestor walk by identity). Until then `file_identity`'s
  Windows arm answers `None`, and `may_be_same_file` fails closed.
- **Move as a rename** asks `same_device`, which also answers "can't tell" on Windows today (copy then delete, safe).
- **Dedup of a selection or a batch** by app path keeps two aliases as two items; both name one file, so the second op
  meets an already-moved or already-deleted source (`already_gone.rs`'s answer), never a different file.
- DFS (`//domain/root/x` beside its target share) is the same shape; not run (no DFS namespace on the builder).

### Design decisions and what each rejected

- **Owned and borrowed pair over a `str` (`AppPathBuf`/`&AppPath`).** Rejected: the current `AppRoot` + `Vec<String>`
  struct (an allocation per name, no borrowed form, can't borrow a `FileEntry.path`); a single owned type (every
  `Volume` call would clone, and `parent` couldn't be a slice).
- **UTF-8 text, lossless by an escape** (review round 1). Rejected: an `OsStr`-backed type (WTF-8 on Windows would leak
  into arithmetic, and specta/serde want a string); and the first draft's "lossy, as today", because today's native
  walks are lossless (survey) and a lossy type would drop a non-UTF-8 child from a folder copy, and because a lossy path
  can address a different file. § "Names that aren't UTF-8".
- **`from_wire` refuses `..`; it doesn't resolve it.** Rejected: lexical resolution everywhere (the grammar's current
  behavior), which retargets a local IPC path across a symlink. § "Normalization".
- **Typed `..` is the volume's answer** (review round 2). Rejected: refusing it (people type and paste it all the time),
  lexical everywhere (wrong across a POSIX symlink), and `canonicalize` (needs the whole path to exist and rewrites
  every link). § "Typed and pasted `..`".
- **`FileEntry.name` is lossless too** (review round 2). Rejected: round one's "name stays lossy display text"; the
  listing protocols address rows by name (survey), so a lossy name is an address, and a resort can move a selection to
  another file. Rejected also: a separate `displayName` field (changes `bindings.ts` and every row's size for a case
  that's one in a million rows). Display goes through `shownName`, and a Linux E2E catches a sink that doesn't.
- **Escapes in plane 16, with a canonical form** (review round 2). § "Names that aren't UTF-8" holds the ranges, the
  Unicode checks, and the rejected schemes (PEP 383, WTF-8, OPTU-8).
- **Typed native refusals.** Rejected: `Option` (a `None` becomes "not found" upstream, hiding an ADS-shaped name or a
  wrong-flavor path). § "The native boundary".
- **Our own type, not a crate.** Rejected: `camino::Utf8Path` (UTF-8, but std's Windows rules on Windows: the very bug)
  and `typed_path`'s Unix paths (POSIX rules everywhere, but `C:/x`, `//server/share`, and `sftp://h/x` read as
  relative, so `parent` and `starts_with` don't clamp at the root). Neither knows our roots or pick segments.
- **Named constructors only.** Rejected: `From<PathBuf>`/`AsRef<Path>` (makes "unrepresentable" false the day it lands)
  and a lint-only rule over `&Path` (#1225 option b, which David turned down).
- **Path-shaped method names are NOT mimicked.** Rejected: giving `AppPath` a `join(&str)` with `Path`'s semantics so
  bodies compile unchanged. `Path::join` with an absolute argument replaces the base and keeps `..`; an app path that
  silently did otherwise would change macOS behavior in bodies nobody reread. The flip keeps every body on the bridge
  instead, and each crate's milestone converts arithmetic deliberately, under its pins.
- **Component-wise `Eq`, `Hash`, and `Ord`.** Rejected: bytewise (cheaper, but reorders the six `BTree*`s, the ledger's
  derived `Ord`, and every `sort()` of paths, and splits `/a` from `/a/` in every dedup set while bridge values are
  unnormalized). Once milestone 11 deletes the bridge every value is normalized, so `Eq` and `Hash` may switch to
  bytewise then with identical answers (the milestone-3 property test proves the equivalence).
- **Case is the volume's.** Rejected: a case-folding flag on the path (two equal-looking paths would hash apart by where
  they were made). `starts_with_folded`/`eq_folded` take the volume's `NameRules`.
- **Link targets stay raw.** `read_link_target` and `create_symlink`'s `target` keep `PathBuf`: a link's text is
  whatever the link holds (relative, or native on Windows), never an app path to do arithmetic on.
- **Relative paths stop reaching `Volume` methods.** A typed `&AppPath` is absolute. Callers that pass `""`, `.`, `/`,
  or a relative tail today pass `volume.root()` or a `join_relative` from it. `root_anchored` survives only for wire
  input, then goes. Each crate's milestone finds its relative callers (they surface as `legacy_unchecked` inputs).

### Cost

The listing hot path builds one path per entry. Today that's two allocations (`PathBuf::join`, then
`to_string_lossy().into_owned()`); with `AppPath::join` it's one `String` with exact capacity. `parent`, `file_name`,
`starts_with`, and `strip_prefix` are slices with no allocation. Measured, not assumed:

- Milestone 1 pins **allocations per listed entry** for `list_directory` on `InMemoryVolume` and `LocalPosixVolume` (a
  counting allocator, the pattern in `crates/cmdr-search/src/test_alloc.rs`), as a ceiling a conversion may lower and
  never raise.
- The flip and the `cmdr-local` and `cmdr-operations` milestones run `index_benchmarks` and `transfer_benchmarks` before
  and after on the M1 (`docs/notes/performance/README.md` for how to read them) and record the numbers in a
  `docs/notes/` note linked from `crates/cmdr-fs/DETAILS.md`. A regression past noise stops the milestone.

### Keeping it that way

- **The type** does most of it: no `Volume` method takes a `Path`, and nothing turns a `Path` into an `AppPath` without
  a named constructor.
- **`app-path-std-bridge`** (new check, milestone 3, Go check runner): counts `legacy_unchecked` and `as_std_legacy` per
  file in a shrink-wrapping allowlist. It's the progress meter during the conversion; at zero, milestone 11 deletes both
  functions, and the check stays as a ban on `Path::new(<x>.as_str())` and `OsStr::new(<x>.as_str())`.
- **`app-path-native-crossing`** (new check, milestone 11): `to_native` and `from_native` only in allowlisted files (the
  native boundaries above), shrink-wrapping.

## Milestones

Sizes per `docs/tooling/backlog-board.md`. Windows test counts are from the builder's runs on 2026-11-18
(`docs/notes/windows-port/build-progress.md`, #1225's comment, #1232). Each milestone's agent runs its crates'
`cargo nextest` on the builder at its end when the lead hands it `ssh mami`, and reports pass/fail per named cluster.

Order and parallelism (review round 2 checked each "parallel" against the crate graph, `cargo metadata`, dev-deps
included; the hidden couplings it found are why waves B and C differ from round one's):

- **Wave A**: 2 (`LocalVolume`) lands FIRST, alone (262 files, mostly tests: everything else rebases onto it). Then 1
  (pins) and 3 (the type) in parallel: 1 is tests across the workspace, 3 is `cmdr-fs` plus `lib/path`. They meet in one
  file, `path_grammar_cases.json`: 1 adds the arithmetic rows, 3 the escape rows; 1 lands first and 3 rebases.
- **Then 4 alone** (the flip), with the smallest freeze (§ 4 "Freeze").
- **Then 5 alone** (`cmdr-fs` internals): every backend builds on `RemoteRoot` and `root_anchored`.
- **Wave B, four parallel worktrees**: 6 (network backends), 7 (device backends), 8 (virtual volumes and leaf crates
  that depend only on `cmdr-fs`), 9 (`cmdr-local`). ❗ Their crates are disjoint, but their DEPENDENTS aren't:
  `cmdr-operations` depends on `cmdr-local`, `cmdr-archive`, `cmdr-listing`, and `cmdr-operation-log`, and its tests on
  `cmdr-adb`, `cmdr-s3`, `cmdr-sftp`, `cmdr-smb`, and `cmdr-webdav`; `cmdr-restic`'s tests on `cmdr-local` and
  `cmdr-smb`. So the wave's rule: **a crate's non-trait `pub` signatures don't change in its wave-B milestone.** Bodies
  convert; a `pub fn` that should take `&AppPath` gains the typed form BESIDE the old one (the old one delegating
  through the bridge), and the consumer's milestone (10a, 10b, 10c/10d) switches its callers and deletes the old.
  `rename_local_exclusive` (9) is the known case. Without the rule, each wave-B branch edits `cmdr-operations` files and
  the four collide there.
- **Wave C, two parallel worktrees**: 10a (`cmdr-operations`, needs 8 and 9) and 10b (`cmdr-index` and `index-query`;
  `cmdr-index` depends only on `cmdr-fs` and `fsevent-stream`, dev-deps included, and walks the disk natively itself, so
  10b needs only 5 and may run inside wave B as a fifth worktree if an agent is free). Then **10e**: `cmdr-multi-rename`
  and `cmdr-listing-wasm`, moved out of 8: both depend on `cmdr-operations` (and the wasm crate on `cmdr-viewer`,
  `cmdr-multi-rename`, and `cmdr-operation-log`), so converting them before 10a would edit them twice.
- **Then 10c, then 10d** (the app; need 10a, 10b, 10e). Sequential, not parallel as round one had it: 10c owns
  `file_system/`, `commands/`, and `go_to_path/`, whose helpers 10d's `tools/` and `operations_host/` call, and 10c's
  `resolve_typed` is what 10d's MCP inputs use. 10d may start on a branch off 10c's first green push.
- **Then 11** (close).

### 1. Pin today's behavior, and the Windows targets

- **Size**: S.
- **Scope**: tests only, plus test helpers. (a) Extend `path_grammar_cases.json` with arithmetic rows (join a name,
  parent, file name, names, starts-with, strip-prefix, rebase, shown) for every root shape: `/`, `C:/`,
  `//server/share/`, and scheme roots for sftp (rooted at `/` and at a subfolder), webdav, s3, nfs (`%2F` export), mtp
  (with a pick segment), adb, restic, and an archive-inner path (`/x/a.zip/inner`). Rows carry the POSIX answer (pinned
  from today's `std::path` behavior on macOS and Linux) and the Windows answer (the target, asserted against the new
  type in milestone 3 on every OS, since it's pure). (b) Pin the current string helpers on macOS/Linux: `root_anchored`
  (including its relative and alias inputs), `RemoteRoot::{to_remote_path, to_app_path}`,
  `app_paths::{path_under, rebase}`, `twin_path::{folder_of, shown}`, `ChildName::under`, `volume::ids` exports with a
  trailing slash, `scan_walk`'s join, `patching`'s parent match, `InMemoryVolume::canonicalize` through a link. Many
  exist; add what's missing, name each so it reads as behavior. (c) The allocations-per-entry ceilings (§ "Cost"). (d)
  Ordering and equality pins: a `BTreeSet<PathBuf>` of `/a/b`, `/a-b`, `/a.b`, `/a b`, `/a`, `/a/b/c`, `/é`, `/z`, and
  `sftp://h/x` iterates in today's order, and a `HashSet<PathBuf>` holds `/a` and `/a/` as one. (e) For each store in §
  "Persisted formats", a round-trip pin: the strings the current write path stores read back to the same values (on
  POSIX this proves "parse then print is the identity" over real writers), plus a stored favorite of `/a/` opening `/a`.
  (f) **Non-UTF-8 names, Linux only** (`#[cfg(target_os = "linux")]`, a `caf\xe9.txt` file in a folder `d`, and a UTF-8
  sibling literally named `caf\u{FFFD}.txt` in a second fixture): pin today's outcome of (1) listing `d`, (2) copying,
  moving, and deleting `d` through the local engine (the child goes along: these are the regressions to guard), (3) the
  same with `d`'s listing cached in a pane (`walk_cached_entries`, today's bug: pin it with a comment naming milestone
  9), (4) a volume-engine copy of `d` from `LocalPosixVolume` to `InMemoryVolume` (what name the copy gets), (5) an op
  on the row path itself (it misses the file, or with the sibling present acts on the sibling: pin it as the bug it is),
  (6) the drive index scanning `d`, (7) the name-addressed protocols with `caf\xe9.txt` and `caf\xe8.txt` side by side:
  a resort with one of them selected (`resort_listing`: which row stays selected), the cursor restored by
  `cursorFilename`, `check_rename_validity` on either, and a transfer whose `preKnownConflicts` names one (#1236's
  cases, pinned as the bugs they are), (8) where those rows sort under each sort column. (g) **Symlinks and trailing
  slashes**: on Linux and macOS, a `stat`, a listing, and a delete of `/x/link/` (a symlink to a folder, with the
  trailing `/`) through each IPC entry point that takes a wire path (`commands/file_system`, the MCP tools, go-to-path),
  pinned as today's answer; and `/x/link/../y` and `../y` (from a pane inside `/x/link`) through the same entry points
  and through `from_user_input`. (h) **Unicode form**: on macOS (APFS) and Linux, a file created as NFD `café` and
  addressed as NFC: pin today's stat and listing answers. (i) **The Windows baseline**: the builder's full
  `cargo nextest run --workspace` with per-test outcomes, saved as `docs/notes/windows-port/app-path-baseline.md` (the
  failing test names grouped by crate and cluster, nothing else). Every later milestone diffs against it, so "turned
  green" and "newly red" are lists, not counts. Review round 4's lessons from the existing logs: (1) read outcomes from
  a nextest JUnit report (a `ci`-style profile with `junit.path` set, `--no-fail-fast`), never from console output:
  PowerShell wrapped the earlier logs at 98 columns and two are UTF-16, so names and panics had to be stitched back
  together; (2) tag each failure with its cause cluster and owner from § "Windows failures, mapped", including the ones
  that are NOT this plan's, so a milestone's diff never counts a #1226 or task-35 cell as its own; (3) about 150 of the
  failures panic with no cause in the message ("the transfer must complete", "the remote edit should commit"), so the
  report keeps each failing test's captured output, and a test whose output names no error is listed as "cause unread"
  with its nearest sibling's cluster, never silently filed under one; (4) run in a fresh checkout and remove
  `crates/cmdr-index/cmdr-*` afterwards: the index fixtures make their trees in the CWD, and a timed-out test (nextest
  kills it) never drops its `TempDir`.
- **Intentions**: make the flip and every crate conversion provably byte-identical on macOS and Linux.
- **Landmines**: pin reality, not the plan. If today's behavior looks wrong (a relative path anchored oddly), pin it
  anyway, add a comment, and tell the lead; fixing it is a separate change. Verify each new pin bites (break the helper
  locally, see red).
- **Test plan**: the new tests pass on macOS and Linux CI. Windows: the grammar table's Windows rows pass against
  today's `path_grammar` where it already answers (parse, parent, basename, shown, to/from native).
- **DONE**: every helper above has a pin, the case table has every shape, the alloc ceilings and ordering pins exist,
  and `pnpm check --fast` plus the touched crates' tests are green.
- **Done** (2026-11-18, branch `worktree-agent-a1e86a0d33a4ba4ab`, `c6fc36dd1d`..`6f955520c7`; tests only, 71 Rust cells
  plus 8 vitest cases and 119 case-table rows, none removed):
  - (a) `path_grammar_cases.json` gains 112 `arithmetic` rows (`std`: today's `std::path` answer, asserted on Unix;
    `target` or a typed `refused`: the type's answer, asserted against today's grammar for the local roots on every OS),
    25 with a `note` where the two differ on purpose. Decided here for milestone 3: a scheme root is
    `scheme://authority` (no trailing `/`), so `parent("sftp://h:22/home")` stays today's `sftp://h:22` byte for byte
    and an empty authority's root is `scheme://`.
  - (b) `crates/cmdr-fs/src/volume/path_helper_pin_test.rs` (15) plus cells in `patching_test.rs` and
    `scan_walk_test.rs`. (c) `list_alloc_ceiling_test.rs` in `cmdr-fs` and `cmdr-local`: one entry over none (a slope
    over many isn't stable: `InMemoryVolume`'s sort lowercases per comparison, and the comparison count follows
    `HashMap` order), 5 in memory, 9 on macOS's disk, 13 on Linux's. (d) `path_order_pin_test.rs`. (e) every store, 11
    cells. (f) the non-UTF-8 cells, 8 sub-items (`crates/cmdr-operations/src/non_utf8_name_tests.rs` and siblings).
    (g)/(h) 14 cells across `cmdr-local`, the delete engines, `stat_paths_kinds`, the MCP resolver and live-read gate,
    go-to-path, and 7 `userInput` rows. (i) `docs/notes/windows-port/app-path-baseline.md`, made by
    `scripts/windows-test-baseline` (`render`, `diff`, `explain` over the nextest JUnit report).
  - Wrong today, pinned as such, and who flips them: the row and name lossiness, `walk_cached_entries`, and the index's
    collapsed rows (9); the engine's row-path delete and the pre-skip match (10a); resort, cursor, and rename validity
    (10c); a trailing `/` and a `..` across a symlink at the wire (10c/10d), `root_anchored` and `path_under` passing
    `..` through, `folder_of` climbing above a scheme root, `sftp://h/x == sftp:/h/x` (the type), the operation log's
    drive and UNC roots (8). ❗ Two are live data loss, not only Windows debt: with the folder open in a pane, a local
    delete of a folder holding `caf\xe9.txt` reports success and deletes nothing (#1236), and the raw IPC `delete_files`
    of `x/link/` (a folder symlink with a trailing `/`) empties the link's target.
  - Where the code differed: MCP `delete`/`rename` take the pane selection, never a wire path, so only the raw IPC
    `delete_files` can send `x/link/` (question 9's "only an MCP input can" is wrong); item (f)(4)'s copy gets no name,
    it fails or copies the sibling; the pre-known-conflict bug sits in `build_pre_skip_set`; the protocol and sort cells
    run on in-memory rows on every OS; `PersistedTab` stores no `cursorFilename` and closed tabs aren't persisted;
    `path_exists` needs the volume manager, so stat is pinned through `stat_paths_kinds`; the full Windows workspace run
    needs `--exclude cmdr-fsevent-stream` (macOS-only, never built there before).
  - Verified: macOS `cargo nextest list --workspace` 16,515 (per touched crate: `cmdr` 6,777, `cmdr-fs` 1,016,
    `cmdr-index` 1,999, `cmdr-listing` 384, `cmdr-local` 150, `cmdr-operation-log` 167, `cmdr-operations` 2,583); the
    Desktop (Rust) job's 38 checks and the Desktop (Svelte) job's 34 with `--ci --fresh` green; `clippy-linux` and
    `rust-tests-linux` green. Every pin seen red under a deliberate break, except the two row-path deletes and the
    same-filesystem move guard (no code break flips them) and NFC/NFD (the filesystem's answer, covered on both OSes).
    Windows: the baseline run at `e03f087946`, 934 of 13,831 failing (869 failed, 65 timed out); the leftover cold-cover
    trees removed from `D:\src\cmdr` (46) and from this run's checkout (47).

### 2. `cmdr_local::LocalVolume`

- **Size**: S (mechanical; 262 files).
- **Scope**: `pub type LocalVolume = LocalPosixVolume` under `cfg(unix)` and `= LocalWindowsVolume` under Windows, in
  `crates/cmdr-local/src/platform/mod.rs`, with a doc saying when to name which. Every site that means "this machine's
  local disk" names `LocalVolume`: the 73 prod files and the tests that don't depend on POSIX. Tests that use modes,
  xattrs, inodes, errnos, `std::os::unix`, or the POSIX-only methods (`barriers_issued`, `with_barrier_interval`,
  `with_sidecar_probe`) keep `LocalPosixVolume`. A compile-time pin that both siblings have the shared inherent
  constructors (`local_folder`, `on_mount`, `with_listing_hook`) with the same signatures.
- **Intentions**: a consumer stops naming an OS. This is plan task 35/36's open question, now decided.
- **Landmines**: don't un-gate any `#![cfg(unix)]` test file here even if its only POSIX-ism was the name: it'd run on
  Windows and fail on #1225. List those files in a comment on #1225 instead; the milestone that converts their crate
  un-gates them. Doc comments that say `LocalPosixVolume` meaning the type itself stay.
- **Test plan**: macOS and Linux unchanged (rename only). Windows: the workspace still builds and clippy is clean on the
  builder; no new tests.
- **DONE**: `rg LocalPosixVolume` hits only POSIX-specific code, `cmdr-local`'s own files, and docs about the type;
  `apps/desktop/src-tauri/src/volumes/platform/` names `LocalVolume` where it can.
- **Done** (2026-11-18, branch `worktree-agent-a6fb518d61314b3ef`, `bc690227d6`): 91 files moved to `LocalVolume` (318
  sites), and the Windows arms of `volumes/platform/` and `local_disk/platform/` too; 56 files keep `LocalPosixVolume`
  (modes, xattrs, inodes, errnos, symlinks, `std::os::unix`, the POSIX-only methods, plus the POSIX type's own host
  suite and the inode-dedup cell); 100 more name it only in comments about the type or in the `#![cfg(unix)]` reason
  lines, which stay until each crate's milestone un-gates them. The rename is a rerunnable classifier (POSIX signals in
  a file's code, comments stripped, plus a hand-read keep and rename list), not per-site judgment. Where the code
  differed: the survey's 262 files are 247 Rust files outside `cmdr-local` today, and the gates are item-level too (12
  renamed files keep `#[cfg(unix)]` cells), not only file-level. Verified: workspace `cargo nextest list` identical
  before and after on macOS (16,482 tests), the Desktop (Rust) job's 33 checks green with `--ci --fresh`, `clippy-linux`
  and `rust-tests-linux` green, the zig cross-check `cargo check --all-targets --target x86_64-pc-windows-gnu` for
  `cmdr-local`, `cmdr-operations`, `cmdr-restic`, and the app, and clippy `--all-targets -D warnings` for the same four
  on the builder, whose nextest run (7,327 tests, 6,788 pass) is unchanged by construction
  (`docs/notes/windows-port/build-progress.md`). The still-gated files whose only POSIX-ism was the name (53 file-level,
  12 with cell gates) are listed on #1225.

### 3. The type

- **Size**: M.
- **Scope**: `cmdr_fs::app_path` per § "The type": `AppPath`, `AppPathBuf`, `RelativeAppPath`, `NotAnAppPath`, the
  operations, constructors, `Serialize` and `specta::Type` as `string`, `Deserialize` through `from_wire` (for
  in-process formats only; persisted DTOs keep `String`), `app_path!`, the transition bridge, and the
  `app-path-std-bridge` check with an empty allowlist. Fold `path_grammar::AppPath` into it (its 15 callers move);
  `ChildName::under` gains the `&AppPath` form beside the old one. Docs: `crates/cmdr-fs/DETAILS.md` gets an "App paths"
  section (the canonical home for this design once the plan is wiped), `CLAUDE.md` a one-line guardrail. **The frontend
  half of the escape lands here too** (moved from 9 in review round 2: the escape rows this milestone adds to the shared
  case table would otherwise fail `grammar.test.ts` until 9): `lib/path`'s `shownName` and `nativeNameLength`,
  `displayPath` mapping escapes to U+FFFD over any text, `t()` doing it over every param, and the escaped rows in
  `path_grammar_cases.json` (join, parent, basename, shown, the canonical form, a mixed-alphabet refusal). Inert until 9
  makes an escape exist; pure, so vitest proves it. **The test tempdir helper lands here too** (moved from 9 in review
  round 4: 10b may run inside wave B, before 9, and its 134 path failures are fixtures rooted in native tempdirs):
  `cmdr_fs::testing` (behind its feature, outside the wasm-clean `app_path` module) makes a tempdir and returns it with
  its app path (`from_native` after `GetLongPathNameW` on Windows, since a `TEMP` under an account name past eight
  characters is an 8.3 spelling), with a `tempdir_in` twin for the index fixtures that root in the CWD on purpose
  (`/tmp` is excluded on Linux and a symlink on macOS).
- **Intentions**: the whole vocabulary exists and is tested before anything uses it.
- **Landmines**: `Eq`, `Hash`, and `Ord` are component-wise (§ "Design decisions"), and `Hash` must agree with `Eq` on
  unnormalized bridge values. A pick segment (`␀4242`) is a valid name. Under `PathFlavor::Posix` `\` and `:` are name
  characters. `from_normalized` must reject every non-canonical spelling (property test: `from_normalized(s).is_ok()`
  iff `parse(s) == s`). The escape (§ "Names that aren't UTF-8") lives here, with `shown()`'s U+FFFD rendering and the
  canonical form; the frontend twin learns it in this same milestone. `to_native`'s refusals are typed (§ "The native
  boundary"). Keep it `wasm32`-clean (no `std::fs`; the escape works on bytes and `u16` units, not `OsStr`, so it
  compiles everywhere and Linux CI tests the Windows arm).
- **Test plan**: the case table from milestone 1 runs against the new type for BOTH flavors on every OS; property tests
  for normalize idempotence, `parent`/`join` inverse, `starts_with`/`strip_prefix` agreement, and `Eq`, `Hash` equality,
  and `Ord` equal to `PathBuf`'s on arbitrary POSIX-shaped text (including `//`, trailing `/`, `.`, `..`, space, `-`,
  `.`-led names, non-ASCII). The bridge's byte-identity property (§ "The type", "never normalizes"). The escape:
  `from_native` then `to_native` is the identity on arbitrary bytes (Unix) and arbitrary `u16` units (Windows, tested as
  data on every OS), two names differing only in invalid bytes get different spellings, a name holding a literal escape
  code point round-trips, `to_native(canon(s)) == to_native(s)` and `from_native(to_native(s)) == canon(s)` on arbitrary
  strings, `from_user_input` of text holding an escape-range code point names that LITERAL character, and a byte escape
  is `ForeignEscape` under `to_native` for Windows (a unit escape under POSIX). `to_native` refusal rows: `/x` on
  Windows, `C:/x/a:b`, `C:/x/a*b`, NUL, a 33,000-unit path, `//host:445/share/x` (`NoPortOnUnc`), and acceptance rows
  for `C:/x/CON`, `C:/x/a.`, and `//[::1]/D$/x` (the server's `:` is no name character). `to_native_plain` refusal rows
  (`NotPlainSafe`): `C:/x/a.`, `C:/x/a `, `C:/x/con.txt`, `C:/x/lpt1 .x`, `C:/x/CONIN$`, a 260-unit path; acceptance
  rows `C:/x/a.b` and `C:/x/console`. `from_native` refusal rows: `C:foo`, a bare `C:`, `foo`, `\\.\PhysicalDrive0`,
  `\\?\Volume{…}\x`, `\\?\GLOBALROOT\x`, `\\?\C:\x\..\y`, `\\?\C:\x\.\y`, `\\?\C:\x\\y`. Unicode: NFC and NFD `café` are
  unequal values. Windows builder: `cargo nextest run -p cmdr-fs` green with the new module.
- **DONE**: the module is complete, documented, and unused outside tests, `path_grammar` delegates to it, and the
  frontend's grammar suite still passes the shared table.

### 4. Flip the trait

- **Size**: L (mechanical, one branch, every crate; the lead plus eight agents in five tiers, below).
- **Scope**: every path-carrying `Volume` method takes `&AppPath` (`&[AppPathBuf]` for batches) and returns
  `AppPathBuf`/`&AppPath`, except link targets (§ "Design decisions"). The path-carrying types beside the trait
  (`CopyScanResult`, `BatchScanResult`, `routed.rs`, `root_echo.rs`, `mkdir_all.rs`) follow. All 183 impls change
  signature and open each overridden body with `let path = path.as_std_legacy();` (or the pair), so no body's arithmetic
  changes. Callers pass `&AppPath`: test literals via `app_path!` (by the codemod, below), runtime values via
  `AppPath::legacy_unchecked(&p)` where they hold a `PathBuf`, or the value itself where it already came from a volume.
  The `app-path-std-bridge` allowlist records the counts.
- **Intentions**: the trait is typed from here on, so no new code can pass a native path, and every later milestone is a
  crate-local, parallel conversion with a visible counter. Zero behavior change on any OS.
- **Landmines**: this is the one flag day, and it's unavoidable: `dyn Volume` has 183 impls and 61 path-carrying
  methods. Rejected alternative: a parallel `&AppPath` method beside each `&Path` one with defaults calling each other
  (doubles a 120-method trait, and a pair of defaults that call each other recurses forever when an impl overrides
  neither). Keep it small: no body logic edits, no renames, no drive-by fixes. `legacy_unchecked` must not validate or
  normalize (that would change POSIX behavior for `..` and trailing slashes), and must stay lossless for non-UTF-8 (a
  local-engine caller can hold a raw `read_dir` path). More:
  - **`app_path!` accepts only canonical literals** (it goes through `from_normalized`), so the sed fails loudly on a
    literal like `Path::new("/a/")`, `"/a/../b"`, `""`, `"."`, or `"photos"` (relative literals reach volumes today, see
    `root_anchored`) instead of silently testing a different path. Those few become `legacy_unchecked(Path::new(…))` by
    hand and are counted like any other bridge.
  - **Types beside the trait that serialize keep their field types** in this milestone (`ScanConflict` and every DTO
    stay `String`/`PathBuf`); only the in-process ones (`CopyScanResult`, `BatchScanResult`, `routed.rs`,
    `root_echo.rs`, `mkdir_all.rs`) move, and a map keyed on their paths keeps `PathBuf`'s dedup because `Hash`/`Eq` are
    component-wise.
  - **The codemod makes it boring** (review round 2). The flip is three artifacts, all on the branch:
    1. The hand-written trait change in `cmdr-fs` (`volume/mod.rs`, the in-process types beside it, and `cmdr-fs`'s own
       impls and tests), by the lead. Small and read in full.
    2. `scripts/app-path-flip/` (Go, deleted by milestone 5): a compiler-driven fixpoint. It runs
       `cargo check -p <crate> --all-targets --message-format=json`, and rewrites only the spans rustc points at, by
       error code: `E0053` on a `Volume` impl method (the param or return type: `&Path`→`&AppPath`,
       `&[PathBuf]`→`&[AppPathBuf]`, `PathBuf`→`AppPathBuf` with `.into_owned()` on the returned bridge), plus the body
       prelude `let path_std = path.as_std_legacy(); let path: &Path = &path_std;` ONLY when the body names the param
       (else `unused_variables` reddens clippy); `E0308` at a call whose expected type is `&AppPath`: a canonical string
       literal inside `Path::new(…)`/`PathBuf::from(…)` becomes `app_path!(…)`, anything else
       `&AppPath::legacy_unchecked(&…)`; a returned `AppPathBuf` used as a `Path` gets `.as_std_legacy()`. Re-run until
       the crate checks or no rule applies. Type-driven, so a native `Path::new` fixture that never meets a `Volume`
       method is never touched (a textual sed would have rewritten all 4,800 literals, native ones included).
    3. `residue.patch`: what the codemod can't do (generic `P: AsRef<Path>` helpers that forward to a volume, closures
       whose parameter types are inferred, macros, non-canonical literals). Written by the tier agents, kept as its own
       commit per crate.
  - **Agents, by tier** (from the crate graph, dev-deps included; each owns whole crates, so no two edit one file; each
    runs the codemod on its crates on a branch off tier 0, and the lead merges a tier before the next starts):
    - Tier 0, the lead: `cmdr-fs`.
    - Tier 1, four agents in parallel (each crate's deps, dev-deps included, are only `cmdr-fs` or each other within the
      agent's set): (A) `cmdr-local`, `cmdr-git`, `cmdr-archive`, `cmdr-viewer`, `cmdr-media-meta`, `cmdr-search`; (B)
      `cmdr-smb`, `cmdr-sftp`, `cmdr-webdav`, `cmdr-s3`, `cmdr-nfs`, `cmdr-adb`, `cmdr-mtp`; (C) `cmdr-index` and
      `index-query`; (D) `cmdr-listing`, `cmdr-brief-columns`, `cmdr-operation-log`, `operation-log-dump`.
    - Tier 2: (E) `cmdr-operations` (108 impls, the biggest; one agent, its tests split into subagent batches by
      directory, sequential on one branch) and (F) `cmdr-restic` (tests need `cmdr-local` and `cmdr-smb`), in parallel.
    - Tier 3: (G) `cmdr-multi-rename` then `cmdr-listing-wasm` (both depend on `cmdr-operations`). Small.
    - Tier 4: (H) the app `cmdr` (33 impls, 629 test call sites; it depends on every crate above), one agent with
      subagents by directory, sequential on one branch.
    - Realistic wall time: tier 0 half a day, the other tiers about a day together, mostly compile time; the slow lanes
      and the fixture lanes run on the M1 Mac before landing.
  - **The audit is a reproduction, not a reading** (replaces round one's line-classifying script): the lead re-runs the
    codemod on a clean checkout of the base plus artifact 1, then asserts
    `git diff base..flip == codemod_output + residue.patch` file by file. Everything mechanical is proven by
    re-execution; what a person and a fresh reviewer agent READ is artifact 1, the codemod's rules (each a
    type-preserving rewrite: the bridge byte-identity property test proves no value changes in transit), and
    `residue.patch`, whose own audit is: it touches no `assert*!` line and no expected value (`rg` over its `+`/`-`
    lines), and no line outside a function that names a `Volume` method or a bridge.
  - **Freeze** (review round 2: round one froze "the Rust crates for other branches until post-land checks pass",
    open-ended, which a one-person project with nightly agent runs can't hold). The codemod makes the flip regenerable,
    so other work keeps landing until the last moment:
    1. Build and green the flip on its branch while `main` moves. Whenever it falls behind, rebase artifacts 1 and 3
       (small) and RE-RUN the codemod on the new base instead of resolving conflicts in its output.
    2. The freeze is the landing window only: announce it (a pinned line in `docs/guides/multi-agent-refactors.md`'s
       status or the lead's session), schedule it outside the nightly agent window, regenerate on the current `main`,
       run the full gate (below), and land. About two CI cycles, a few hours. No `pnpm land` of any other Rust-touching
       branch inside it.
    3. After it lands, a branch that was in flight rebases and runs the same codemod (`scripts/app-path-flip/` stays
       until milestone 5 for exactly this), then its own residue by hand.
  - **Rollback**: everything the gate runs, it runs BEFORE landing, so there's no post-land window to wait out. If
    something red shows up after the land, `git revert -m 1` the merge while it's still the tip (clean); once other work
    is on top, fix forward with a pin.
- **Test plan**: before landing: the full Rust suite on macOS and Linux via `pnpm land` CI, plus
  `pnpm check --only-slow` (the slow Rust lanes), the Docker backend fixture lanes (SMB, SFTP, WebDAV, S3, NFS), and
  `desktop-e2e-linux`; milestone 1's pins are the proof, the bridge's byte-identity property test the proof that no
  value changed in transit, and the reproduction audit the proof that no body changed beyond `residue.patch`. Windows:
  the `windows-lane.yml` run (`gh workflow run windows-lane.yml --ref <branch>`: clippy on every member, compile only)
  green, and the builder's workspace run matches milestone 1's baseline list exactly (no fixes expected yet; a test that
  flips either way is a finding).
- **DONE**: no `Volume` method names `Path` except link targets; every impl compiles on all three OSes; bridge counts
  recorded per file; the audit reproduces and `residue.patch` is read and empty of logic.

### 5. `cmdr-fs` internals

- **Size**: M.
- **Scope**: `RemoteRoot` holds `AppPathBuf`s and does `AppPath` arithmetic (`to_remote_path(&AppPath)`,
  `to_app_path -> Option<AppPathBuf>`); `normalize_remote_path` stops using `Path::components` (server paths are POSIX,
  so a `/`-only normalizer); `app_paths.rs` folds into `AppPath` methods; `root_anchored` takes a wire string and
  returns `AppPathBuf` (callers with a typed path drop it); `scan_walk`, `patching`, `ids`, `twin_path`, `child_name`,
  `in_memory` (root, overrides, link resolution) convert. All `cmdr-fs` bridges go. Delete `scripts/app-path-flip/` (its
  in-flight-branch window is over). ❗ `RemoteRoot` and `root_anchored` are `pub` and every backend calls them: per the
  wave-B rule, add the typed forms beside the old and leave the backends' calls to 6, 7, and 8.
- **Intentions**: the shared vocabulary every backend builds on is correct on Windows.
- **Landmines**: `RemoteRoot`'s component-wise prefix match (`…:2222` vs `…:22`) and "`..` before the check" are data-
  safety rules; the new code keeps both and its tests stay. NFS app roots must keep `%2F` (no decoding).
  `InMemoryVolume` is the contract oracle: ❌ don't relax a contract to green a test.
- **Test plan**: Windows builder, the 24 `cmdr-fs` tests ignored for #1225 lose their `cfg_attr` and pass:
  `volume::remote_paths` (7), `volume::scan_walk` (9), `volume::patching` (3), `in_memory` canonicalize (1),
  `child_name` (1), `ids` (1), `twin_path` (2). macOS/Linux: pins unchanged.
- **DONE**: `rg '#1225' crates/cmdr-fs` is empty; `cmdr-fs`'s bridge count is zero.

### 6. Network backends: SFTP, WebDAV, S3, NFS

- **Size**: M (four crates, one agent with up to three subagents).
- **Scope**: each crate's bodies drop their bridges and do `AppPath` arithmetic; server paths come from `RemoteRoot`.
  `cmdr-nfs`' app-side pieces (the identity seam, places) stay where they are.
- **Landmines**: S3 keys aren't paths: `/` is a delimiter by convention and a key can end in `/` (a folder marker) or
  hold `//`. Keep the key layer string-based and convert only at the app-path edge. ❗ Two keys that normalize to one
  app path (`a//b` and `a/b`, `a/./b`, a key with a `..` segment) must not both list: an op on the shared path would hit
  the other object. The colliding or unrepresentable key is skipped with a typed count (as NFS skips a non-UTF-8 name),
  pinned by a test with both `a/b` and `a//b` in one bucket; first pin what today's listing does with them. WebDAV
  `href`s are percent-encoded; decode before `to_app_path`, never after, and a decoded name holding `/`, or equal to `.`
  or `..`, is dropped as today. SFTP and NFS names arrive as raw bytes: each backend either escapes a non-UTF-8 name (§
  "Names that aren't UTF-8") or skips it with a typed count, ❌ never `from_utf8_lossy`; pin today's behavior first and
  tell the lead which it picks. Escaping is the default for SFTP (byte escapes in its own alphabet, decoded back to
  bytes for the server); WebDAV and S3 are UTF-8-only, so a name carrying any escape is `NotRepresentable` on write (the
  cross-namer rule). SFTP overrides `parent_for_dot_dot` in 10c's commit, not here.
- **Test plan**: Windows: `cmdr-sftp` 7 (`paths_test`), `cmdr-webdav` 23, `cmdr-s3` 42 plus the 5 pause timeouts,
  `cmdr-nfs` 45 (30 with a `\` in the path, and 15 fake mounts answering `ExportNotFound`, not 7: the export is picked
  by a prefix match over the typed path). Not here (review round 4): `cmdr-nfs`'s three `errors_test` cells and
  `cmdr-adb`'s `an_unlisted_device_errno_arrives_as_its_linux_class`, a product bug, not a path (§ "Windows failures,
  mapped"); `cmdr-s3`'s `nanoseconds_survive_and_trailing_zeros_drop` (Windows' `SystemTime` holds 100 ns); `cmdr-nfs`'s
  `apply_metadata_dates_and_modes_a_folder` (modes) unless the baseline shows it path-shaped. macOS/Linux: each crate's
  suite plus its Docker fixture lane where CI runs one.
- **DONE**: the four crates' bridge counts are zero and their Windows suites green.

### 7. Device backends: SMB, MTP, ADB

- **Size**: M.
- **Scope**: as 6. `cmdr-smb`'s `to_smb_path` and anchored mounts; `cmdr-mtp`'s storage paths and pick segments;
  `cmdr-adb`'s device paths.
- **Landmines**: SMB maps `\` and other forbidden characters into the private-use area in `smb2`; an app path name with
  `\` must reach that mapping intact. MTP picks (`twin_path`) are the one NUL-bearing name. An SMB share mounted by the
  OS on macOS (`/Volumes/x`) is a POSIX-shaped app path through `LocalVolume`, not `smb://`. SMB names are UTF-16 on the
  wire: pin what `smb2` does with an unpaired surrogate today (lossy, or refused); the target is unit escapes, which may
  need an `smb2` change (David's crate, so it's fine to make; file it there). `cmdr-smb`'s UNC root matching folds
  server and share case (§ "Normalization", Case).
- **Test plan**: Windows: `cmdr-adb` 51 (the fake device answers `errno 30` to a write whose path gained a `\`, so those
  are paths too), `cmdr-smb` 13 (the anchored-mount prefix arithmetic over `/Volumes/…`), `cmdr-mtp` 1 (plus
  `host_seam_test`'s mutation count, to diagnose: it may not be a path).
- **DONE**: bridge counts zero; Windows suites green.

### 8. Virtual volumes and leaf crates

- **Size**: M.
- **Scope**: `cmdr-archive` (boundary detection, URL-rooted entry paths; the local backing file crosses at `to_native`),
  `cmdr-git` (portal paths app-side, `git2` native), `cmdr-restic` (hub and volume; fixtures stop rooting at
  `/Volumes/…` as native), `cmdr-viewer` (markdown link and image resolution as `AppPath` arithmetic; opening a local
  file crosses at `to_native`), `cmdr-listing` (and `cmdr-brief-columns`), `cmdr-operation-log` (scheme-rooted dir
  prefixes), `cmdr-search` (its two `Path` lines). `cmdr-multi-rename` and `cmdr-listing-wasm` moved to 10e (they depend
  on `cmdr-operations`).
- **Landmines**: `cmdr-archive`, `cmdr-listing`, and `cmdr-operation-log` are `cmdr-operations` dependencies: the wave-B
  rule holds (no `pub` signature changes; typed forms beside). The operation log's dir tree must keep
  `reconstruct_dir_path`'s exact output for every POSIX and scheme row (pinned), and must GAIN the Windows roots:
  `intern_dir`, `find_dir`, and `reconstruct_dir_path` take the `AppPath`'s root as ONE name, the way
  `split_scheme_root` keeps `scheme://authority` (`C:` reconstructs `C:/…`, `//srv/share` reconstructs `//srv/share/…`).
  Today `C:/x` comes back as `/C:/x` and `//srv/share/x` as `/srv/share/x`, a different path the rollback would act on
  (verified in `store/dirs.rs`, review round 1). No migration: no Windows install has rows, and POSIX rows keep their
  bytes (the root `""` row and the `/`-led names are unchanged). Restic snapshot paths are POSIX paths from the
  repository, whatever OS made the backup. Archive entry names come through the archive crate's sanitizer
  (`read/name/`); a name it can't make UTF-8 (a CP437 zip) keeps whatever the sanitizer answers today, pinned.
- **Test plan**: Windows: `cmdr-archive` 5 (its other two, the zip's Unix mode bits reading `0o666`, aren't paths),
  `cmdr-git` 1 (un-ignore), `cmdr-viewer` 13, `cmdr-listing` 2 (one is `foreign_path_test`; the stall-attribution one is
  not #1225 and may stay), `cmdr-operation-log` 3, and `cmdr-restic` 55 plus 9 timeouts (review round 4; the rest of
  that run's 61 and 10 are `cmdr-listing-wasm`'s, 10e). New on every OS (the store is pure SQL): intern, find, and
  reconstruct round-trip `C:/x/y`, `C:/`, `//srv/share/x`, and `//srv/share/`, and a rollback plan for a drive-rooted op
  targets the drive path.
- **DONE**: bridge counts zero in these crates; the named Windows clusters green; the operation log round-trips every
  root shape.

### 9. `cmdr-local`

- **Size**: M (review round 2 added the frontend wiring and the Linux E2E).
- **Scope**: both volumes take `&AppPath` natively: `resolve` and `local_windows/paths.rs` collapse into
  `AppPath::to_native` (the Windows `resolve_under` goes, since callers no longer pass relative paths); `reading.rs`
  builds `FileEntry.path` with one `AppPath::join` and `FileEntry.name` from the same escaped name;
  `rename_local_exclusive` gains a `&AppPath` form beside the `&Path` one (the wave-B rule; 10a switches its callers).
  Watch events coming back from the OS go through `from_native`. Frontend, same land: every name sink renders through
  `shownName` (the listing's Full and Brief name cells, the rename prefill and its "unchanged" check, search results,
  the selection summary, drag images), the backend's menu labels and Brief text widths through `shown_name`, and
  `validateFilename` measures with `nativeNameLength`.
- **Intentions**: the one local crossing per OS, and the listing hot path at one allocation per entry.
- **Landmines**: a non-UTF-8 name on Linux: stat through the raw `OsStr` (fix `fdfa5b58cb`) as today, and the row's
  `path` and `name` become the ESCAPED spelling (§ "Names that aren't UTF-8"), `name == path.file_name()`. That's loud
  rule 1's first exception: its own commit, with `reading_parity_test::a_non_utf8_name_lists_lossily` rewritten to say
  the path and name round-trip (the one pin this plan edits on purpose, with the lead's OK), the frontend sinks in the
  same commit (milestone 3 already shipped the helpers), and milestone 1's pins (f)(7)–(8) flipped: the resort keeps the
  selection on the row the person picked, the cursor and `check_rename_validity` address the right file, and the rows
  sort at their escape code point's place (accepted, pinned). The Windows reader's `child_path` stops joining lossily
  (an unpaired surrogate escapes too). `scan_for_conflicts_impl` builds the destination from the source PATH's
  `file_name()`, ❌ never the lossy `item.name`. Watch events from the OS go through `from_native`, which escapes, so an
  event for a non-UTF-8 name patches the row it listed. The two listers stay field-for-field identical. Run the alloc
  ceilings and benches (§ "Cost"). ❗ Tests hand `cmdr-local` `tempfile`'s NATIVE `C:\Users\…\Temp\.tmpXYZ` where an app
  path belongs, and `paths::native` refuses it as "not an absolute Windows app path". Two entry points, both typed
  `&Path`/`impl Into<PathBuf>` today: the free function `reading::list_directory_core(&Path, …)` (59 of the app run's 65
  panics with that message, by name: "the dir lists", "the scratch dir lists") and a `LocalVolume` rooted at or asked
  for a tempdir (the other six, plus the 84 in `cmdr-operations`, below). Review round 4 confirmed this from the
  builder's logs. So `list_directory_core` and the constructors (`local_folder`, `on_mount`) gain typed `&AppPath` /
  `AppPathBuf` forms beside the old ones (the wave-B rule), and tests switch to the tempdir helper of milestone 3 in
  their crate's milestone, which is why those panics turn green in 10a, 10b, 10c, and 10d rather than here.
- **Test plan**: Windows: `cmdr-local` 30 of 30 plus the conformance cells that only waited on paths, plus new NTFS
  cells (each reproduced by hand in review round 3): `dup` and `dup.` side by side, a delete of the row `dup.` removes
  `dup.` and leaves `dup`; `con.txt`, `NUL`, and `a ` list, stat, rename, and delete as files; a row path holding `:`
  never reaches the OS (`ForbiddenCharacter`, and no stream appears on the base file); `a<D800>b` and `a<D801>b` list as
  two rows with distinct escaped paths, and rename and delete each touch only its own file. macOS/Linux:
  `reading_parity_test`, the conformance suite, alloc ceilings lowered. Linux: milestone 1's non-UTF-8 pins (f) flip
  from "misses the file" to "acts on exactly that file": stat, open, rename, and delete the row path reach
  `caf\xe9.txt`, the `caf\u{FFFD}.txt` sibling is untouched, and two names differing only in invalid bytes list with
  distinct paths. `desktop-e2e-linux`: the display catch-all (§ "The escape on the wire and in the frontend": no escape
  code point in the DOM's text, `caf\u{FFFD}.txt` shown, delete of that row removes `caf\xe9.txt` and leaves the
  sibling). That E2E is the first end-to-end proof for #1236; close #1236 when 10a also lands.
- **DONE**: `cmdr-local` has no bridges, `rename_local_exclusive` accepts an app path on Windows, and no
  `to_string_lossy` builds a path in `cmdr-local` (names for display only).

### 10a. `cmdr-operations`

- **Size**: L (1,910 prod and 4,557 test `Path` lines, 108 test volumes).
- **Scope**: the volume engine and the local engine take `AppPath`; the local engine's fast paths (`local_disk/`, the
  copy strategies, trash, cloud trash, the move sweep, validation) cross at `to_native` per OS call. The in-flight
  ledger keeps native `PathBuf`s. Un-gate the `#![cfg(unix)]` test files whose only POSIX-ism was `LocalPosixVolume`
  (milestone 2's list).
- **Landmines**: "EVERY local write lands via `overwrite::stage_and_land_file`": the landing's paths become app paths,
  the temp's stays the engine's. `DestNameIndex` and `item_identity` compare names, not paths: keep them on `&str`, fed
  from the path's `file_name()` (escaped, lossless; `FileEntry.name` equals it from milestone 9 on, but the path is the
  canonical source), ❌ never from a SHOWN name, or a copy of `caf\xe9.txt` lands as `caf\u{FFFD}.txt`, possibly over a
  real file of that name. Switch `rename_local_exclusive`'s callers to the typed form and delete the `&Path` one; same
  for every wave-B typed-beside pair this crate calls. A destination that can't store a source name's escapes refuses
  that item typed and counts it on the op's summary (the cross-namer rule). Rollback planner and journal rows carry app
  paths into the operation log (pinned). Big enough to split by subagent (transfer, delete and trash, archive edit,
  rename and create, scans), sequential on one branch. Non-UTF-8 safety (milestone 1's pins (f)):
  - The native walks (`scan_walker.rs`, `delete/walker.rs`, the move sweep, `scan_dry_run.rs`, `reversal.rs`) stay
    native inside, and any path they hand to a typed API goes through `from_native`, which escapes. ❌ A walk never
    drops a child it can't type: there's no such child, by construction; if a backend's walk does skip one, the skip is
    counted and reported on the op, never silent, because a copy that reports success while missing a file is the data
    loss a person can't see.
  - `walk_cached_entries` (the listing-oracle short-circuit) builds each child from `entry.path` via `from_normalized`,
    which carries the escape after milestone 9; that fixes today's bug where a pane open on the folder made the scan
    record a lossy, nonexistent path. Its `is_left_out` check takes the path's `file_name()`, not `entry.name`.
  - The move sweep's ledger stays the safety net: it deletes only what landed, so a child the copy missed stays.
  - Windows aliases (§ "One file, several app paths"): `validate_destination_not_inside_source` canonicalizes both sides
    natively after `to_native`; the destination conflict scan and the landing's collision read the stored name, so a
    source named `LONGFI~1.TXT` meets `Long File Name.txt` as a conflict on that name (§ "The native boundary", 8.3).
  - The Recycle Bin, once Windows has one (today `move_to_trash_sync` refuses `TrashNotSupported` there), goes through
    `to_native_plain`, so a name that can't survive the plain form is refused per item, never recycled as its neighbor.
- **Test plan**: Windows: the 289 failures' path clusters (review round 4, from the builder's log: 68 name the
  finalize's native `C:\…` refusal, 16 more a native tempdir handed to `LocalVolume`, about 45 local-engine flows whose
  panic carries no cause but whose siblings show the finalize, 37 a `\` joined into an in-memory or remote app path,
  mostly `transfer::volume`, and 25 `archive_edit::remote` cells over a remote fixture), plus a Windows-only cell in a
  tempdir on the system drive (the builder's `TEMP` is on `C:`, where 8.3 is on; the cell skips, saying so, when
  `GetShortPathNameW` returns no short name): a copy of a file named `LONGFI~1.TXT` into a folder holding
  `Long File Name.txt` reports a conflict and leaves its bytes alone. Not in scope, about 60 that stay red here: the
  `MountedVolumeInside` refusals (25, task 35's mount table and file id); the in-flight-temps test ledger that can't be
  emptied while a connection holds it (25, "start the test ledger empty: Access is denied", #1226's family); node
  identity answering `Unverifiable` (about 8: `ledger::tests`, `reversal::tests`'s drift cells, `source_binding::tests`,
  task 35's `file_identity`); `delete::trash`'s missing-path cell (no Recycle Bin yet); and `archive_edit::fresh_zip`'s
  Unix mode bits. macOS/Linux: full suite, `transfer_benchmarks` before/after.
- **DONE**: bridge count zero in `cmdr-operations`; a local copy and move land on the builder's NTFS.

### 10b. `cmdr-index`

- **Size**: M.
- **Scope**: the local scanner, reconciler, and watcher ingest native paths through `from_native` at the walk and event
  edges and hold app paths after; `store/paths.rs` resolves `&AppPath` by names; the exclusion policy's `/`-prefixed
  rules become app-path rules; media and importance stores keep their `TEXT` columns.
- **Landmines**: the drive index's key decision stands (lowercase, Unicode form untouched). The cold-drive fixture roots
  a tempdir; on Windows that's a drive app path, so the index root is that path, not `/`. Some of that may need task
  34's drive volumes: if so, say which tests and stop there. The walker's `RawDirEntry.path` is raw today and the index
  stats every child through it; at the edge, `from_native` escapes a non-UTF-8 name, so the row's `name` changes from
  the lossy spelling to the escaped one (a cache: the reconciler sees one remove and one add, then converges; pin that
  it converges and doesn't churn every pass, which a lossy name compared against a raw disk name would). Same for media
  and importance `path` columns (§ "Persisted formats").
- **Test plan**: Windows, #1232's clusters as review round 4 counted them from the builder's log (87 failures and 47
  timeouts, all fixtures whose tree is a native tempdir in the CWD, `D:\src\cmdr\crates\cmdr-index\cmdr_test_…`, read
  against the index's `/`-rooted path space, so rows land as `/D:\src\…` or not at all): `reconcile::*` (41),
  `lifecycle::phases::tests` (19), `lifecycle::cover` (8 plus the 46 cold-drive timeouts, as far as task 34 allows),
  `scanner::convergence_tests` (6), `scanner::policy_tests` (4), `watch::branches` (4) and
  `watch::event_loop::tests::replay_chunks` (3, with task 12's smoke test), `partial_agg` (1), the concurrency stress
  cell (1), and one reconciler timeout. The fixtures switch to milestone 3's `tempdir_in` helper. Not here: 20 that are
  #1226's family (`importance::store::legacy_split` is 17, not 3, plus `handle::tests::building_splits_…` and two
  database deletes while a connection holds the file); `store::tests::path_resolution` (3), which aren't paths at all
  (the `*_off_macos` cells expect Linux's binary compare, and Windows now folds case: a cfg fix, #1232); and the
  cold-drive cell that renames a mount point with handles open (Windows refuses the rename). macOS/Linux: suite plus
  `index_benchmarks` before/after.
- **DONE**: bridge count zero in `cmdr-index` and `index-query`; #1232 updated with what's left.

### 10e. `cmdr-multi-rename` and `cmdr-listing-wasm`

- **Size**: S. Moved out of 8 in review round 2: both depend on `cmdr-operations` (the wasm crate also on `cmdr-viewer`,
  `cmdr-multi-rename`, and `cmdr-operation-log`), so they convert after 10a.
- **Scope**: as 8: bodies drop their bridges; `cmdr-multi-rename`'s `names_file.rs` crosses at `to_native` for the names
  file it owns. `cmdr-listing-wasm` builds for `wasm32` (no `to_native` there at all: it has no OS).
- **Test plan**: Windows: `cmdr-listing-wasm`'s 6 failures and 1 timeout (`cmdr-multi-rename` had none on the builder;
  review round 4). macOS/Linux: both suites, plus the wasm build.
- **DONE**: bridge counts zero in both.

### 10c. The app, part one: file system and listings

- **Size**: M.
- **Scope**: `apps/desktop/src-tauri/src/` `file_system/`, `listing_host/`, `volumes/`, `commands/`, `go_to_path/`,
  `favorites/`, `recents/`, `file_viewer/`, `native_drag/`, `about/`, `network/`. IPC arguments become app paths at the
  command edge through `from_wire`; OS integrations cross at `to_native`.
- **Landmines**: the resolver's un-hinted fallback (`resolve_path_to_volume` walks the mount table to `/`) is what D1
  and `remote_paths.rs` defend against; keep it answering exactly as today on macOS/Linux. Two-layer timeouts on path
  resolution stay. `root_volume`'s Windows answer is task 34's, not this plan's. `from_wire` at the command edge is loud
  rule 1's second exception: a trailing `/` is dropped and a `..` name is refused (§ "Normalization"), so a command that
  took `/x/link/` or `/x/link/../y` answers differently. Land that as its own commit after the mechanical conversion,
  with milestone 1's pins (g) flipped deliberately and the lead's OK; until then a command that needs the old answer
  keeps `legacy_unchecked`. In a second commit of the same exception: `resolve_typed` (§ "Typed and pasted `..`") in
  `go_to_path/`, used by go-to-path, the CLI's path arguments, and deep links; the `Volume::parent_for_dot_dot` trait
  method with its lexical default and its two overrides (`cmdr-local`'s Unix volume, `cmdr-sftp`; both crates are done
  by then, so this is a small, owned edit there); go-to-path's `CLAUDE.md` "never `canonicalize`" rule rewritten to the
  new one. The UNC root fold (`same_root`) lands with the resolver's volume pick.
- **Test plan**: Windows: the app failures in these directories (about 72 of the 188 in `cmdr`, review round 4: 11
  native-tempdir panics in `file_system/watcher*` and `network/smb_mount_gate`, the rest `std::path` on app paths, most
  in `network/`, `go_to_path/`, `about/`, and `commands/`; `about/`'s 13 carry no path in the panic and are the least
  certain; milestone 1's baseline lists them), and the `#![cfg(unix)]` files here gated only for #1225 un-gate. Not
  here: the root volume's lookups (`resolve_location`, `a_home_relative_path_resolves_to_root`), task 34's. macOS/Linux:
  suite and `desktop-e2e-linux` (it touches listings); pins (g) and (h) per entry point, an IPC delete of `/x/link/`
  removes the link and leaves the target's contents, and § "Typed and pasted `..`"'s cells.
- **DONE**: bridge count zero in these directories.

### 10d. The app, part two: operations host, tools, and the rest

- **Size**: M. After 10c (review round 2: it calls 10c's `file_system/` helpers and `resolve_typed`); it may start on a
  branch off 10c's first green push.
- **Scope**: `operations_host/`, `tools/` (MCP), `agent/`, `search/`, `listing_index_sizes/`, `multi_rename/`,
  `services_menu/`, and whatever 10c didn't own.
- **Landmines**: the MCP tools take paths from an agent, the input most likely to carry `..`, a trailing `/`, or a
  relative path; they go through `resolve_typed` (a `..` is the volume's answer; past the root is `OffRoot`) with the
  same deliberate-change commit as 10c, and a refusal is a typed tool error, never a silent re-anchor onto the root
  (today's `root_anchored` joins a relative path onto the root). Agent surfaces never carry an escape: outputs use the
  shown form plus `unaddressable`, and an input holding U+FFFD that a sibling's shown name matches is `AmbiguousName` (§
  "The escape on the wire and in the frontend", Agent surfaces).
- **Test plan**: Windows: the remaining app failures that name "not an absolute Windows app path" (54 of the 65:
  `multi_rename/` 44, `tools/actions/multi_rename_test.rs` 9, `redact/` 1; review round 4 by name, none in
  `operations_host/`) or a `\photos`-style path; `cmdr` totals reported against the baseline. macOS/Linux: suite, plus
  an MCP write given `/x/link/../y` lands where `ls` says and one given a path past the root refuses typed; Linux: an
  MCP listing of the non-UTF-8 fixture shows `caf\u{FFFD}.txt` flagged `unaddressable`, and an MCP delete of
  `caf\u{FFFD}.txt` with both files present refuses as `AmbiguousName`.
- **DONE**: the app's bridge count is zero.

### 11. Close

- **Size**: S.
- **Scope**: delete `legacy_unchecked` and `as_std_legacy`; turn `app-path-std-bridge` into the permanent ban; add
  `app-path-native-crossing` with its allowlist; remove every remaining `#1225` ignore; update D1 and tasks 34–36 in the
  Windows plan, `crates/cmdr-fs/DETAILS.md` § "App paths" (the durable home), `apps/desktop/src/lib/path/DETAILS.md` (a
  pointer to the Rust twin), and `docs/notes/windows-port/build-progress.md` with the builder's full run. Then wipe this
  spec per `docs/specs/DETAILS.md`.
- **Test plan**: the builder's full `cargo nextest run` over the workspace, totals logged; macOS/Linux full CI.
- **DONE**: `rg 'legacy_unchecked|as_std_legacy|#1225'` is empty; #1225 closes; #1232 holds only what's not paths.

## Test strategy

Review round 2. Which suites carry the weight, where the holes are, and what each Windows run proves.

**The safety nets** (each named milestone's gate runs them):

- Milestone 1's pins and the shared case table: the byte-identity proof on macOS and Linux.
- `InMemoryVolume`'s contract suite and `cmdr-local`'s conformance suite (`reading_parity_test`, the listers' parity):
  the oracle every backend is held to.
- `cmdr-operations`' engine suite (1,734 tests) plus `transfer_benchmarks`: copy, move, delete, the ledger, rollback.
- The Docker fixture lanes (`rust-fixtures-lane.yml`: SMB, SFTP, WebDAV, S3, NFS servers): the only place a backend's
  real wire spelling meets the type.
- `desktop-e2e-linux` (`e2e-linux-lane.yml`): the only Linux end-to-end, so the only place a non-UTF-8 name reaches the
  DOM.
- `pnpm check --only-slow` before the flip's land and after the last milestone.

**The gaps** (why round one's three live Linux bugs went uncaught: every non-UTF-8 test was a unit test of the reader,
none acted on the row):

- No test acts on a row whose name isn't UTF-8, or on two rows whose lossy names collide. Closed by milestone 1 (f) and
  milestone 9's E2E.
- No test drives a name-addressed protocol (resort, cursor, rename validity, `preKnownConflicts`) with colliding names.
  Closed by milestone 1 (f)(7).
- No test covers a trailing `/` or `..` through an IPC or MCP entry point onto a symlink. Closed by milestone 1 (g).
- No Windows lane runs TESTS: `windows-lane.yml` is compile and clippy only, so every Windows count comes from the
  builder (`ssh mami`). Milestone 1 (i)'s baseline list makes those counts diffable; each milestone's agent reports its
  diff against it, by test name.
- No test checks a frontend surface for a leaked private-use code point. Closed by milestone 9's DOM catch-all.

**Windows failures, mapped** (review round 4, from the builder's own logs of the 2026-11-18 runs, clustered by the panic
text and assertion diffs; `D:\logs\{batch1,final,w2-index,w4-nextest2}…` and `D:\tmp\{ops,g2}-nextest.log`; milestone
1's baseline replaces these counts with names). The causes:

- **N, a native tempdir where an app path belongs** ("not an absolute Windows app path"): the test hands `cmdr-local`
  `C:\Users\…\Temp\.tmpX` (`list_directory_core`, a `LocalVolume` root or argument), or the index fixture roots its tree
  at `D:\src\cmdr\crates\cmdr-index\cmdr_…`. Round three's reading is confirmed, widened from "volume roots" to
  `list_directory_core` too. Fix: milestone 3's tempdir helper, then each crate's milestone.
- **F, the local engine's finalize** hands `rename_local_exclusive` a native path: 9 (typed form) then 10a.
- **S, `std::path` arithmetic on an app path**: a `\` joined into a POSIX, in-memory, or scheme path (`/srv/data\x`,
  `sftp://h:22\srv`), or a POSIX literal that `Path::is_absolute` calls relative on Windows. The owning crate's
  milestone.
- **Not this plan**: root-volume lookups (task 34), `MountedVolumeInside` and `Unverifiable` node identity (task 35),
  open-file deletes and renames (#1226's family), and the Windows-only test-fixture and product cases listed per crate.

Per crate:

- `cmdr-fs` 24 ignored: 5.
- `cmdr-sftp` 7 (S), `cmdr-webdav` 23 (S), `cmdr-s3` 42 (S) plus 5 timeouts, `cmdr-nfs` 45 (30 S, 15 `ExportNotFound`
  from the export prefix match): 6. Not 6: `cmdr-s3`'s nanosecond cell (Windows' 100 ns `SystemTime`), `cmdr-nfs`'s
  three `errors_test` cells and one modes cell.
- `cmdr-adb` 51 (S; the fake device's `errno 30` writes too), `cmdr-smb` 13 (S), `cmdr-mtp` 1 (S): 7. Not 7:
  `cmdr-adb`'s device-errno cell; `cmdr-mtp`'s `host_seam_test` mutation count until diagnosed.
- `cmdr-archive` 5 (S), `cmdr-git` 1, `cmdr-viewer` 13 (S: markdown links and images resolve to `Blocked`),
  `cmdr-listing` 1 (`foreign_path_test`), `cmdr-operation-log` 3, and `cmdr-restic` 55 plus 9 timeouts (S, rooted at
  `/Volumes/…`): 8. Not 8: `cmdr-archive`'s two Unix-mode cells, `cmdr-listing`'s stall attribution (`Unknown` for
  `Drive`).
- `cmdr-local` 30: 9.
- `cmdr-operations` 289: 10a for about 230 (F 68, N 16, S 37, about 45 local-engine flows with no cause in the panic,
  and 25 `archive_edit::remote` cells over a remote fixture, likely S). Not 10a, about 60: `MountedVolumeInside` 25
  (task 35), the in-flight-temps test ledger 25 (#1226's family), node identity `Unverifiable` about 8 (task 35), trash
  1, Unix modes 1.
- `cmdr-index` 111 plus 47 timeouts (#1232): 10b for 87 and the 47 (N); not 10b: 20 (#1226's family: `legacy_split` 17,
  `building_splits` 1, two database deletes), 3 case-collation cells (not paths), and one mount-point rename with
  handles open.
- `cmdr-listing-wasm` 6 plus 1 timeout (S): 10e. `cmdr-multi-rename` had no failures.
- The app `cmdr` 186 plus two timeouts (of the 235 failures; the other 49 are `cmdr-nfs`): N 65 (`multi_rename/` 44,
  `tools/actions/multi_rename_test.rs` 9, `file_system/watcher*` 10, `redact/` 1, `network/smb_mount_gate` 1), S about
  95 (`network/`, `go_to_path/`, `about/`, `commands/`, `tools/`, `server_volumes/`, `priority/`, `agent/`, `search/`
  goldens), root-volume lookups 8 (task 34), the two timeouts (a dead-share card, an agent-cancelled walk; unread), and
  about 17 that aren't paths (below). So 10c about 72, 10d about 89.

Not paths, found in the logs, each with its owner:

- **Product bug, every network or device backend that maps a remote errno**: `cmdr-nfs` `errors_test` (3) and `cmdr-adb`
  `an_unlisted_device_errno_arrives_as_its_linux_class`. The server's or the phone's errno is Linux-numbered, and the
  code reads it through the HOST's `libc` constants and `io::Error::from_raw_os_error`, which on Windows means a Win32
  code (16 is `ERROR_CURRENT_DIRECTORY` there, not `EBUSY`). So on Windows a busy phone or an NFS refusal gets the wrong
  error class. Goes to a new sentence in the Windows plan's "Windows build state" (D2's backends), with an issue.
- **Windows-only test expectations**: `crash_reporter` stamps and `cmdr-s3` nanoseconds (100 ns `SystemTime`);
  `cmdr-index` `path_resolution` (3: `*_off_macos` expects Linux's binary compare, Windows now folds); `data_wipe`'s
  plan tests (5) and `tools::sensitive` (1), which build POSIX home and XDG literals for the app's OWN dirs, native by §
  "App path or native path"; `subprocess` (2, no `echo` program); one `HOME is set` cell. #1232 and the Windows build
  state.
- **Windows behavior the product hasn't met yet**: `instance_lock` (2, the holder's pid reads `None`),
  `logging::rotating_file` (1, a rotation while another writer holds the file), the menu's "Undo delete" (no Recycle
  Bin), the zip Unix-mode cells. Their Windows plan tasks.
- **#1226's family is bigger than the plan said**: 25 in `cmdr-operations`, 20 in `cmdr-index`, 3 in the app.
- **Leftovers on the builder**: about 40 `crates/cmdr-index/cmdr-cold-cover-*` trees in `D:\src\cmdr`, left by the
  timed-out cold-drive tests (milestone 1 (i) cleans up after its run).

## Review questions

The planner's five, answered in review round 1 (correctness and data safety), then what's still open for the lead.

1. **The POSIX `/` root on Windows: decided, keep it as a value, fenced at both edges.** Data safety only needs that a
   `/x` never reaches a Windows OS call (where it would mean the current drive's root) and never arrives from the wire
   (where it would be resolved to some local volume). `to_native` refuses it typed and `from_wire` parses with the
   host's flavor, which refuses it, so only the in-memory volume and test code make one (§ "The type"). Rejected: a
   `mem://` root and rewriting 4,800 literals, which buys nothing the two fences don't, at the cost of every fixture.
2. **Non-UTF-8 names: decided, lossless, not lossy.** The premise was wrong: the local engine's walks and the index
   carry raw bytes today, so a lossy type would regress folder copies, and the lossy row is itself a bug that can act on
   a different file. § "Names that aren't UTF-8". Rejected: keeping it lossy plus an issue.
3. **The flip: decided, one L, with a revert-ready landing.** A split would mean a half-typed trait whose two halves
   disagree about one path between landings, and a shim whose defaults call each other. What makes one L safe is that
   it's provably mechanical (the diff audit), provably value-preserving (the bridge byte-identity test), and revertible
   (it lands alone, behind a freeze). Milestone 4.
4. **`local_path`, `stands_in_for_own_disk`, `local_backing_file`: decided, app paths.** The engine builds a native path
   as "the volume's local path plus the op's tail", which done natively is the very `std::path` arithmetic this plan
   removes; and the answers are compared against the op's app paths (is this source inside that volume; can a move be a
   rename). A native answer on Windows (`\\?\C:\x`) would compare unequal to `C:/x` and quietly change that routing.
   Each crosses at `to_native` at its OS call. Rejected: native answers.
5. **`RelativeAppPath`: decided, keep it.** As `&str`, a tail can carry a leading `/`, a `..`, or an empty name, and
   `join_relative(&str)` would be the `Path::join("/x")` hazard again (the base replaced, or the path climbing off its
   volume in `rebase`). As a type it's a validated, no-dot, no-leading-slash `str` newtype, near-free.

Round one's four, decided by the lead (2026-11-18) and written in by review round 2:

6. **UNC root case: decided, fold the root, keep the spelling.** Server and share compare case-insensitively (Unicode
   simple case folding) in every root match, and keep their case as typed or reported; `Eq` stays exact. §
   "Normalization", Case.
7. **The escape code points: decided, as proposed**, after review round 2 checked them against Unicode's rules and the
   prior schemes, and added the canonical form that makes `to_native` injective. § "Names that aren't UTF-8".
8. **Rule 1's exceptions: decided, inside this effort**, each its own characterized commit with a before/after pin (loud
   rule 1). Review round 2 folded the typed-`..` resolver into the second.
9. **Today's live bugs: the non-UTF-8 family is #1236** (filed in review round 2, with the name-addressed protocols
   round two found). The trailing-`/` case (a wire path `/x/link/` stats and walks the link's target) has no issue: the
   frontend never sends one (rows are built by `join`), so only an MCP input can, and 10d fixes it.

Round two's three, decided by the lead (2026-11-18) and written in by review round 3:

10. **The trailing-`/` MCP case: decided, it rides milestone 10d**, with no separate issue.
11. **Cross-namer copies: decided, refusal only.** Offering a fallback name ("land it as `caf\u{FFFD}.txt`") stays out
    of this plan.
12. **Typed `..`: decided, a leading `..` follows the pane** (lexical, like the `..` row and `cd`); every other `..`
    follows the volume's OS. Review round 3 ran the Windows half: lexical is what Win32 does through junctions and
    directory symlinks (§ "Typed and pasted `..`").

Round three's two, decided by the lead (2026-11-18) and written in by review round 4:

13. **Per-folder case sensitivity on NTFS: decided, Windows plan task 35.** A folder WSL made (or one switched with
    `fsutil setCaseSensitiveInfo`) can hold `A` and `a`, which a volume-wide `NameRules` and the drive index's lowercase
    key would merge, and so would the conflict scan. The local Windows volume reports case sensitivity as a property PER
    DIRECTORY (`FileCaseSensitiveInformation`), and the index and the conflict scan ask it there. Not this plan: app
    paths stay exact, so nothing here makes it worse.
14. **`to_native_plain`'s refusal: decided, split by layer.** This plan owns the `NotPlainSafe` refusal TYPE (milestone
    3, with its reasons: trailing dot or space, reserved device name, length). The tasks that build its consumers
    (trash, open with, child processes, and the rest of § "The native boundary"'s list) own the user-facing wording, so
    no message key lands here.

## Invariants

- macOS and Linux listings, wire paths, store rows, and sort orders are byte-identical before and after every milestone
  (milestone 1's pins), except loud rule 1's three named, pinned changes.
- `bindings.ts` is byte-identical; no DTO changes type on the wire.
- No store migrates.
- No `From`/`AsRef`/`Deref` conversion exists between `Path` and `AppPath`.
- `to_native`/`from_native` appear only in allowlisted boundary files, and refuse typed, never with a bare `None`.
- `from_native` then `to_native` is the identity on every byte; no path is ever built from a lossy spelling.
- Every native name has exactly one app spelling (the canonical form), and `FileEntry.name == path.file_name()`.
- No surface a person or an agent reads shows a raw escape code point; typed text never carries a live one.
- A typed `..` is resolved by the volume that holds the path, never by text alone across a link.
- The bridge never normalizes: `legacy_unchecked(p).as_std_legacy()` has `p`'s exact bytes.
- A walk never drops a child silently; a skip is counted and reported on the op.
- App path equality and hashing are exact (no case or Unicode folding); folding takes the volume's `NameRules`.
- `AppPath`'s `Eq`, `Hash`, and `Ord` equal `PathBuf`'s component semantics on POSIX-shaped UTF-8 paths.
- `from_wire` never resolves `..`; `to_native` never yields a drive-relative or rooted-without-drive Windows path, nor a
  name holding `:`.
- No path reaches a Windows API without `\\?\` unless `to_native_plain` made it, and that form never names a different
  file than the verbatim one (trailing dot or space, reserved names, and length refused).
- Nothing decides "same file" or "inside" by app path equality; on Windows those are canonical native paths or file
  identity.
- The operation log reconstructs every root shape it interned.
- A link target is never an app path.
- Every listed entry costs at most the allocations milestone 1 pinned.
- `RemoteRoot` refuses a path off its root, matched by whole components after `..`, in both directions.

## Review log

- **Round 1, correctness and data safety** (2026-11-18). Verified in code: the local engine's walks, the move sweep, and
  the index walker carry raw `OsStr` paths; `walk_cached_entries` and `scan_for_conflicts_impl` use lossy names;
  `intern_dir`/`reconstruct_dir_path` know only `/` and scheme roots; `path_grammar` resolves `..` lexically. Changed:
  non-UTF-8 names go from "lossy, as today" to a lossless escape inside the type (§ "Names that aren't UTF-8", loud rule
  2), and the bridge with it; `Eq`/`Hash` go from bytewise to component-wise so bridge values dedup as `PathBuf` did;
  new § "Normalization" (`from_wire` refuses `..`, drops a trailing `/` as a deliberate pinned change) and § "The native
  boundary" (typed refusals, `:` and the alternate-data-stream hazard, `/x` on Windows never reaching the OS, trailing
  dots and reserved names reachable but not creatable); the operation log gains drive and UNC roots (milestone 8); S3
  key collisions (milestone 6); the flip's diff audit, `app_path!` canonical-only rule, freeze, and revert path
  (milestone 4); pins (d)–(h) in milestone 1; the five review questions answered and four opened. Renumbered this
  decision D5 to D6 in the Windows plan (the menu model's branch holds D5).
- **Round 2, the frontend and the wire, and the execution's feasibility** (2026-11-18). Verified in code:
  `resort_listing` re-maps the selection by name and `check_rename_validity` rebuilds the old path from a name (so
  `FileEntry.name` is an address, and round one's "name stays lossy" left a live wrong-file bug); the frontend's row
  actions send `entry.path` and `cmdr-media://` carries tokens (no path URL-encoding anywhere); `displayPath` already
  fronts every shown path; `shortenMiddle` slices by UTF-16 unit (cuts emoji today); the crate graph, dev-deps included
  (`cargo metadata`); `windows-lane.yml` is compile-only. Checked against outside sources: Nerd Fonts 3's Material
  Design block sits in plane 15 (its glyph-set wiki). Assumed, not run: every Linux reproduction (no Linux box in this
  round). Changed: the escape gained a canonical form (round one's spelling was injective only one way: escapes that
  decode to valid UTF-8 aliased the plain text) and two alphabets with a cross-namer refusal; the ranges are settled
  with the Unicode checks and the rejected schemes; `FileEntry.name` carries the escape too, display goes through
  `shownName`/`displayPath`/`t()` with a Linux DOM catch-all; typed text never carries a live escape; agent surfaces
  show the shown form and refuse ambiguous U+FFFD input; new § "The escape on the wire and in the frontend" (every stop,
  every persisted value decided); new § "Typed and pasted `..`" (the volume answers `..`; round one kept go-to-path
  lexical and refused `..` elsewhere); UNC root folding written in (the lead's decision); the order: 2 first, the wave-B
  "no `pub` signature changes" rule, 10b free to run with wave B, `cmdr-multi-rename` and `cmdr-listing-wasm` moved to a
  new 10e after 10a, 10c before 10d, the frontend escape helpers moved from 9 to 3; milestone 4 gained a compiler-driven
  codemod, five agent tiers, a reproduction audit in place of the line-classifier, and a freeze shrunk to the landing
  window; milestone 1 gained pins (f)(7)–(8) and the Windows baseline list (i); new § "Test strategy" (nets, gaps, every
  Windows cluster mapped to its milestone); questions 6–9 answered, 10–12 opened. Filed #1236 for the live non-UTF-8
  bugs.
- **Round 3, Windows-native correctness on the real machine** (2026-11-18). A std-only Rust probe (`rustc`, no cargo)
  plus PowerShell on the builder, Windows 10 Pro 22H2 build 19045, NTFS (`D:` 8.3 off, `C:` 8.3 on), in a scratch
  folder, all removed after (shares, `subst`, and folders checked gone). Each claim, ran or not:
  - Ran: drive forms (`C:`, `C:foo`, `/x` → current drive, `\\?\D:` without `\`, lowercase drive, `/`, `.`, and `\\`
    inside a verbatim path); the long-path ceiling (206 at 32,750 units) and plain-past-260 failing without
    `LongPathsEnabled`'s help; trailing dots and spaces and six reserved names through `\\?\`, and the plain form
    hitting the WRONG file (`dup.` read and deleted `dup`, `nul.txt` written to the void, `rb.` recycled `rb`);
    alternate data streams with and without `\\?\`, on files and folders; 8.3 aliases (listing children under a short
    spelling, `canonicalize` lengthening it, a truncating write and a rename through a short name hitting the long-named
    file); unpaired surrogates (create, list via `std` and `FindFirstFileW`, lossy collision, rename, delete, a literal
    U+10E800 name); `..` through a junction and a directory symlink, plain, slashed, verbatim, and a `del` through one;
    UNC loopback, `C$`-style admin shares, server and share case, IPv4, IPv6 `[::1]` and `ipv6-literal.net`, a port
    (refused), a `$` share (works), a trailing-space share (Windows refuses to create one), `canonicalize` of a loopback
    share and a `subst` drive; a child process's working directory as verbatim, past 260, and UNC.
  - Not run: per-folder case sensitivity (`setCaseSensitiveInfo` refused, no WSL feature), DFS (no namespace), Samba
    share names with a trailing space, `SHFileOperation`/`IFileOperation` given `\\?\` directly (the .NET wrapper
    refused it first), and the failing-test sample: the builder ran another agent's 25-minute clippy and nextest for the
    whole 30-minute wait, so the logs (`D:\logs\w4-nextest2.log`, `D:\tmp\ops-nextest.log`) weren't read. The 65-panic
    cluster's cause is read from code (milestone 9's constructor landmine) and stays a hypothesis until milestone 1's
    baseline names the tests.
  - Changed: `:` refusal scoped to names (the IPv6 server keeps its `:`); `NoPortOnUnc`; the plain-form hazard and a new
    `to_native_plain` with `NotPlainSafe` for every API that can't take `\\?\` (open question 14); `from_native` refuses
    a bare `C:` and the verbatim `.`/`..`/empty names, with the corrected reason (NTFS refuses them, not a literal
    name); the length rule maps the OS's 206 instead of trusting 32,767; 8.3 aliases expanded at inbound crossings, and
    a stored-name conflict rule so a short-name-shaped destination never overwrites the long-named file (milestone 10a
    cell); new § "One file, several app paths" (the into-itself guard stays native and canonical, the loopback-UNC gap
    goes to task 35's identity); the Windows lexical `..` confirmed, with the verbatim reason it must happen in the app;
    NTFS cells for milestone 9; the constructor-root landmine and test-tempdir helper in milestone 9; two invariants.
    Questions 10–12 written in as the lead decided them; 13 (per-folder case) and 14 opened.
- **Round 4, the milestone mapping against the real failing tests** (2026-11-18). Read the builder's existing nextest
  logs, nothing rerun (the logs match the code this plan builds on: the app run at `df87f01096`, the others from the
  same day's legs): the app and `cmdr-nfs` (`D:\logs\w4-nextest2.log`), `cmdr-operations` (`D:\tmp\ops-nextest.log`),
  `cmdr-index` (`D:\w2-index.log`), `cmdr-restic`/`cmdr-listing-wasm`/`cmdr-multi-rename` (`D:\tmp\g2-nextest.log`), the
  backends (`D:\logs\batch1-nextest.log`), and `cmdr-listing` after its fixture fixes (`D:\logs\final-nextest.log`).
  Each failure's panic text and assertion diff were parsed out (the logs are PowerShell-wrapped, two UTF-16) and
  clustered; § "Windows failures, mapped" holds the table. Confirmed: the 65 "not an absolute Windows app path" panics
  are tests handing a native tempdir where an app path belongs, by name; corrected: they sit in the app's
  `multi_rename/` (44), `tools/actions/multi_rename_test.rs` (9), and `file_system/watcher*` (10), none in
  `operations_host/`, and 59 of them enter through `cmdr_local::reading::list_directory_core`, not a volume root.
  Changed: milestone 9's landmine names `list_directory_core`; the tempdir helper moves to milestone 3 (10b may run
  before 9, and all 134 of `cmdr-index`'s path failures are fixtures with native trees in the CWD); every milestone's
  Windows test list carries round four's counts and its not-this-plan cells; #1226's family is 48 cells, not 3
  (`legacy_split` alone is 17); `cmdr-index`'s `path_resolution` cells aren't paths; `cmdr-multi-rename` had no
  failures; NFS's `ExportNotFound` is 15, not 7; milestone 1 (i) reads outcomes from JUnit, tags the not-this-plan
  cells, and cleans up the index's CWD trees. Found and handed off: backends that read a Linux-numbered remote errno
  through the host's `libc` and `from_raw_os_error` (wrong error class on Windows; NFS and ADB), the Windows-only test
  expectations, and the about 40 leftover cold-drive trees in the builder's checkout. Questions 13 and 14 written in as
  the lead decided them. Readiness: ready to execute once PR #1233 lands; no design changed, only counts, owners, and
  where the helper lands.
