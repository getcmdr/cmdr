# App path plan

App paths (`/`-separated and self-rooted: `/Users/x`, `C:/Users/x`, `//server/share/x`, `sftp://u@h:22/srv/x`,
`mtp://â¦`) travel through the Rust code as `std::path::Path`/`PathBuf`, so on Windows every `join`, `parent`,
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
- Shape of this doc: `docs/guides/multi-agent-refactors.md` Â§ "The loop".

## Loud rules

1. **macOS and Linux behave byte-for-byte as before at every milestone.** Same listings, same paths on the wire, same
   rows in every store, same sort orders. The characterization suite (milestone 1) is the proof; â never edit a pinned
   expectation to green a conversion. If a pin is wrong, stop and tell the lead. The three deliberate exceptions, each
   its own commit with its own before/after pin (decided by the lead, 2026-11-18: all three stay inside this effort): a
   name that isn't UTF-8 gets a lossless path AND name (Â§ "Names that aren't UTF-8", milestone 9; Linux and remote
   servers only), an input with a trailing `/` or a `.`/`..` name is normalized, resolved through its volume, or refused
   at the boundary (Â§ "Normalization" and Â§ "Typed and pasted `..`", milestones 10c/10d), and the operation log roots
   drive and UNC paths correctly (milestone 8, Windows only). Milestone 5 adds two, signed off by the lead (2026-11-19):
   the scan walk's canonical child spelling and its typed refusal of a listed name that isn't one plain name (milestone
   5's "Intended behavior changes").
2. **â Never lossy past the OS boundary.** A native name or path that the type can't hold losslessly is a typed
   refusal (`NotAnAppPath`, a counted skip in a walk), never a `to_string_lossy` spelling: a lossy path can name a
   DIFFERENT file (a sibling literally called `caf\u{FFFD}.txt`). That includes `FileEntry.name`: it carries the same
   escaped spelling as the path's last name (`name == path.file_name()`, always), because the listing protocols address
   rows BY NAME (Â§ "Name-addressed protocols"). Only the SHOWN text is lossy (`shown()`, `shownName`, `displayPath`).
3. **The wire doesn't change.** `FileEntry.path`, every IPC DTO, and `bindings.ts` stay `string`. `bindings.ts` is
   byte-identical after every milestone.
4. **No store migrations.** Persisted formats keep their exact serialized shape (Â§ "Persisted formats" proves none is
   needed). A milestone that finds it needs one stops and asks.
5. **â No `From<PathBuf>`, `From<&Path>`, `AsRef<Path>`, `AsRef<OsStr>`, or `Deref<Target = Path>` on the app path
   types, ever**, not even "temporarily". Construction goes through the named constructors in Â§ "The type"; the one
   transition bridge is named, counted, and deleted at the end.
6. **â No `std::path` arithmetic on an app path.** Once a body holds an `&AppPath`, its `join`/`parent`/`file_name`/
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

- `crates/cmdr-fs/src/volume/mod.rs` lines 40â2410: about 120 methods, **61 carry a path**. 54 take one (`&Path`, a
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
- Totals: about 8,000 prod lines and 9,800 test lines name `Path`/`PathBuf`. Test literals: `Path::new("/â¦")` 3,525,
  `PathBuf::from("/â¦")` 1,262, and 38 scheme literals built as `Path`.
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
  `cmdr_local::rename_local_exclusive` a native `C:\â¦` path, which refuses it: no local copy or move lands on Windows.
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
  trailing `/` or `//` never reaches a row) and the typed path writes the same rows. â It knows only two roots: a
  `scheme://authority` head kept as one name, else `/`. A Windows `C:/x/y` interns `C:`, `x`, `y` and reconstructs as
  `/C:/x/y`; `//srv/share/x` reconstructs as `/srv/share/x`, a different path that a rollback would hand to a volume.
  Milestone 8 makes a drive or UNC root one name, like a scheme root; POSIX rows keep their bytes (verified in code,
  review round 1).
- **Non-UTF-8 names in stores**: the drive index, the media index, importance, and the operation log hold the lossy
  spelling today (TEXT columns, names from the lossy row or `to_string_lossy`). With the lossless spelling (Â§ "Names
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
- **MTP pick segments** (`twin_path.rs`): a row path can carry a component of NUL plus an id
  (`/DCIM/â4242/IMG_1.JPG`). The app path must accept it, `parent` must keep today's answer (the pick), and
  `twin_path::folder_of` and `shown` stay the pick-aware layer on top.
- **`root_anchored`** (`volume/mod.rs` ~2441) accepts relative paths and the root aliases `""`, `.`, `/`, and joins
  anything off-root onto the root. `RemoteRoot::to_remote_path` accepts relative paths too. So relative paths DO reach
  `Volume` methods today (from backends' own code and tests).
- **Ordering**: `PathBuf`'s `Ord` is component-wise (`/a/b` < `/a-b`, `/a/b` < `/a.b`, `/a/b` < `/a b`), a string's is
  bytewise (`/a b` < `/a-b` < `/a.b` < `/a/b`, since space, `-`, and `.` sort below `/`). Within a component both
  compare bytes, so non-ASCII UTF-8 orders the same either way. Six `BTreeMap`/`BTreeSet`s key on `PathBuf`
  (`cmdr-index` `reconciler.rs`, `cmdr-listing-wasm` `folder_sizes.rs`, tests), `in_flight_records` derives `Ord`, and
  233 `HashMap`/`HashSet`s key on `PathBuf`.
- **`LocalPosixVolume`** is named in 262 files (901 lines, 408 `LocalPosixVolume::â¦` calls): 189 test files, 73 prod
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
  host it never crosses to the OS (`to_native` refuses it, Â§ "The native boundary") and never enters from the wire
  (`from_wire` parses with the host flavor, which refuses a bare `/x`), so only `parse(_, PathFlavor::Posix)`, the
  in-memory volume, and `app_path!` make one there. Review question 1, answered below.
- `C:/` (drive, letter uppercased; `C:` alone parses to it, `C:x` is refused) and `//server/share/` (UNC, one root, with
  or without its trailing `/`): parsed only under `PathFlavor::Windows`. Under `Posix`, `//server/share/x` normalizes to
  `/server/share/x` and `C:/x` is no app path, as the grammar does today.
- `scheme://authority/` (any RFC 3986 scheme): the root is `scheme://authority`, so `parent` clamps there. An empty
  authority (`scheme:///x`, root `scheme://`) is a valid value, pinned, and no backend mints one. Where a remote VOLUME
  is rooted (`sftp://u@h:22/srv/data`, `mtp://<device>/<storage>`) is deeper and stays `RemoteRoot`'s and the backend's
  business; an archive inside a remote file (`sftp://â¦/x/a.zip/inner`) is the same scheme path with more names.

A name may hold any character but `/` (and NUL, except as the first character of an MTP pick segment). `\` and `:` are
ordinary in a name: a Linux SFTP server can list `a\b`, and `a:b` must never read as a drive. Drive and UNC paths refuse
`\` anywhere, as the grammar does, since NTFS can't hold one. A name may hold the escape code points of Â§ "Names that
aren't UTF-8".

### Names that aren't UTF-8

Decided in review round 1 (loud rule 2; the rejected alternative was "stay lossy, as today", see the survey for why
today isn't a safe baseline). Ranges settled and the canonical form added in review round 2 (the lead's decision 3).

- **The spelling**: `from_native` decodes each name and spells every unit its decoder rejects as ONE plane-16
  private-use code point. Two alphabets, disjoint:
  - **Byte escapes**, U+10FE00 + the byte (U+10FE80âU+10FEFF): a byte a UTF-8 decoder rejects (always
    `0x80`â`0xFF`). Made by byte-native namers: a Unix host's local disk, SFTP, NFS.
  - **Unit escapes**, U+10E800 + (unit â 0xD800) (U+10E800âU+10EFFF): an unpaired UTF-16 surrogate. Made by
    UTF-16-native namers: a Windows host's local disk, and SMB if `smb2` hands one up (milestone 7 pins what it does).
  - `to_native` maps each back, so the round trip is the identity on every byte or unit.
- **Why these ranges** (validated against Unicode 16 in review round 2):
  - Both sit in Supplementary Private Use Area-B (U+100000âU+10FFFD), so every escape is a valid Unicode scalar value:
    a Rust `String`, valid UTF-8 for serde_json, SQLite `TEXT`, a JS string (one surrogate PAIR, never a lone one, so
    `encodeURIComponent`, `TextEncoder`, and `JSON.parse` all accept it), XML's `Char`, and the macOS and Windows
    clipboards. Neither range touches the plane's two noncharacters (U+10FFFE, U+10FFFF) or any other of Unicode's 66
    noncharacters.
  - Private use has no case mapping, no decomposition, and no canonical combining class, so `to_lowercase`,
    `toLowerCase`, `normalize('NFC'|'NFD')`, and HFS+'s NFD storage leave an escape untouched; `NameRules` folding and
    the frontend's `foldedForCompare` can't merge or split one.
  - Plane 16 is the quietest private-use space: the BMP's U+E000âU+F8FF holds Font Awesome and Nerd Fonts glyphs
    (people put those in folder names), Apple's SFM and Samba's `catia` map NTFS-forbidden characters to
    U+F001âU+F0FF, and Nerd Fonts 3 moved Material Design Icons to plane 15 (U+F0001âU+F1AF0, per its glyph-set
    wiki, checked 2026-11-18). Nothing we know of assigns plane 16.
  - Rejected, PEP 383's `surrogateescape` (U+DC80âU+DCFF) and Markus Kuhn's UTF-8b, the same scheme: lone surrogates
    aren't scalar values, so neither a Rust `String` nor strict JSON can carry them, and a JS string holding one makes
    `encodeURIComponent` throw. Rejected, WTF-8 (Rust's `OsStr` on Windows): it's not UTF-8, so it can't cross IPC or
    sit in a `TEXT` column, and it only covers surrogates, never bytes. Rejected, MirBSD's OPTU-8 (U+EF80âU+EFFF): the
    right idea in the crowded BMP private-use area.
- **One canonical spelling per native name** (review round 2; round one's text was injective one way only). Two rules
  together make `to_native` injective over canonical text, so equal native names have equal app paths and the reverse:
  - `from_native` escapes a code point of EITHER alphabet that a name holds as valid text, via its own units (four byte
    escapes on a byte-native namer, two unit escapes on a UTF-16 one). So decoding gives back exactly what the disk
    holds.
  - A name whose escapes decode to units the decoder would have ACCEPTED (`[U+10FEC3, U+10FEA9]` decodes to `C3 A9`,
    which is `Ã©`) is not canonical: `parse` canonicalizes each name as `escape(unescape(name))` (so that text becomes
    `Ã©`), and `from_normalized` refuses it. Same for a name mixing the two alphabets (`MixedEscape`, refused: no namer
    makes one). PEP 383 leaves exactly this hole open (a string of surrogates that spells valid UTF-8 encodes to the
    same bytes as the plain text), so two spellings could name one file and `Eq`, the dedup sets, and the operation log
    would split them.
  - The properties, in milestone 3: `from_native(to_native(s)) == canon(s)` and `to_native(canon(s)) == to_native(s)`
    for arbitrary strings; `from_native` then `to_native` is the identity on arbitrary bytes and arbitrary `u16`s; two
    names differing only in invalid bytes get different spellings; a name holding a literal escape code point
    round-trips.
- **Text a person types never holds an escape.** `from_person_typed`, `from_external_text`, and `AppNameBuf::from_typed`
  (rename, new file, new folder, the transfer's destination name) escape any escape-range code point as LITERAL text
  (its own UTF-8 bytes as byte escapes), so typed text always means what it says, and the escape syntax can't be typed
  or pasted into an address. Round one's "accepted edge" (typed escape code points read as raw bytes) is reversed. Only
  backend-spelled text (`from_wire`, `from_normalized`) carries live escapes.
- **Cross-namer copies**: a name holding escapes of an alphabet the destination can't store (byte escapes to NTFS, SMB,
  WebDAV, or S3; unit escapes to a Unix disk, SFTP, or NFS; any escape to a UTF-8-only protocol) is a typed per-item
  refusal at the destination (`NativeRefusal::ForeignEscape` locally, `NotRepresentable` from a backend), counted and
  reported on the op's summary, â never written as the escape code point or as U+FFFD. Today the volume engine writes
  the lossy name, and can land it over a real `caf\u{FFFD}.txt`. Landing it under its shown name is a choice for a
  person to make later (a conflict-style prompt), not this plan.
- **What people see**: every escape renders as U+FFFD, exactly what today's row shows. Rust: `shown()` and
  `shown_name()`. Frontend: `shownName(name)` and `displayPath(text)` (which `shownPath`, `t()`'s params, and Copy path
  all pass through). Â§ "The escape on the wire and in the frontend" lists every stop.
- **The wire stays `string`** (`bindings.ts` unchanged); only the bytes of a path or name holding such a name change,
  and those rows never reached their file before.
- **Who makes escapes**: `cmdr-local` (both readers and every walk that hands a path to a typed API), the bridge (so the
  flip can't lose a raw path), and any network backend whose protocol carries raw names (SFTP and NFS bytes, SMB
  UTF-16): each one escapes or skips with a typed count, â never `from_utf8_lossy` (milestones 6 and 7 decide per
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
- **The operation log**: old rows for those names stay unrollbackable, as they are today; new rows roll back. â The
  reverse case is a data-loss one (review 2's N7, decided by the lead, 2026-11-18): an old row can hold a plane-16 code
  point as LITERAL text, and read live `[U+10FEC3, U+10FEA9]` names `Ã©`, a real, different file, so a rollback could
  delete it. So the log stamps an escape mark in its `meta` (`escape_mark_last_literal_op_id`, the newest op id on
  record, once, at the first open with `cmdr_fs::app_path::ESCAPES_LIVE` true), and rows at or below it read through
  `AppPathBuf::from_literal_store` (`crates/cmdr-operation-log/DETAILS.md` Â§ "The escape mark"). Shipped with the
  type's second review; inert until 10f flips the const. Tag writes journal here, so the tags undo is covered too.
- **Why the other stores need no mark** (each checked in the type's second review): an old value read live names a
  different path only if it literally holds a plane-16 code point, and none of them acts on a file unseen. Tabs and
  closed tabs, favorites, recents, and go-to-path's recent list only NAVIGATE: the pane shows what it opened, and
  nothing is changed without a person seeing it. The drive index, media index, and importance are caches checked against
  the disk: a wrong key is one remove and one add, then convergence. Ask Cmdr transcripts hold shown text, which never
  re-addresses.
- **Ask Cmdr chat transcripts**: hold whatever text the agent saw (shown form, below); nothing re-addresses from them.

**Display.** Every surface renders an escape as U+FFFD; â never the raw private-use glyph (a box, or nothing):

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
  apps get the real file: built from the app path, â never from the shown text.
- Logs and crash reports: `Display` is the escaped wire spelling inside `LogPrivate`; fine for a log, and reports redact
  paths anyway.
- **The catch-all**: a Linux E2E (`desktop-e2e-linux`, milestone 9) lists a fixture folder holding `caf\xe9.txt`, opens
  rename, the delete dialog, the operation log, and search on it, and asserts the DOM's text holds no code point in
  U+10E800âU+10EFFF or U+10FE80âU+10FEFF and shows `caf\u{FFFD}.txt`. A sink that forgot `shownName` fails it.

**Frontend arithmetic.** `lib/path` splits on `/` and slices at `/` boundaries, so an escape (one code point, a
surrogate pair in UTF-16) can't be cut there. Checked, each safe: `parsePath`/`normalizePath`/`joinPath` (split and join
on `/`), `foldedForCompare` (private use has no case or decomposition), `pathRelativeTo` (slices at the ancestor's
length, which ends on a code-point boundary), sorting (the backend sorts; `localeCompare` on favorites puts escapes in
one consistent spot), and URL encoding (no path is URL-encoded: `cmdr-media://` carries tokens; a scheme path's `%XX` is
the backend's). Not safe: `shortenMiddle` slices by UTF-16 unit and can cut ANY surrogate pair, an emoji today (a
separate live bug, reported to the lead in review round 2); it runs on shown text, where every escape is already U+FFFD
(BMP), so the escape adds no new case, but the emoji one is live. The shared case table gains escaped rows (milestone
3): join, parent, basename, `shown` (U+FFFD), `normalized` (canonical), and a non-canonical row (`[U+10FEC3, U+10FEA9]`
normalizes to `Ã©`), written as JSON `ô»`-style surrogate-pair escapes so the file stays ASCII.

**Input.** Typed text never carries an escape (Â§ "Names that aren't UTF-8"): go-to-path, rename, new file and folder,
the transfer destination, and the CLI go through `from_person_typed` (go-to-path), `from_external_text` (the CLI), or
`AppNameBuf::from_typed` (one name). Paste and drop of FILES arrive as native paths or file URLs and cross at
`from_native` (escaping). Deep links arrive as text: `from_external_text`. Rename's "unchanged" check compares the
edited text with `shownName(originalName)`, â never `originalName` (an escaped original never equals its prefill, so
Enter would rename `caf\xe9.txt` to `caf\u{FFFD}.txt`); a real edit lands the typed text, U+FFFD included, which is the
honest outcome of renaming what the person saw (and the usual way to fix such a name). `validateFilename`'s 255-byte
rule measures the escaped name by its NATIVE length (one byte per byte escape), â never `TextEncoder` over the escape
(four bytes each): a `nativeNameLength` helper next to `shownName`.

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
  special meaning). Under `Windows`, a leading `//` is the UNC root, never collapsed. â S3 keys are the exception:
  `a//b` and `a/b` are two objects, so the S3 key layer never goes through `parse` (milestone 6).
- **`.` names**: dropped (always the same directory).
- **`..` names**: `from_wire` REFUSES one (`NotAnAppPath::DotName`) instead of resolving it lexically. On a local POSIX
  volume the OS resolves `/a/link/../x` physically, to the link target's parent, and a lexical resolve would retarget an
  IPC delete or MCP write to `/a/x`, a different file. Lexical resolution stays only where it is the safety rule
  (`RemoteRoot`, which then refuses off-root). Text a person or an agent types goes through the volume-aware resolver in
  Â§ "Typed and pasted `..`" (review round 2 replaced round one's "keep `from_user_input`'s lexical answer"), so
  `from_wire`'s refusal only ever meets a frontend or backend bug, and logs as one.
- **A trailing `/`**: dropped. This changes which file a POSIX call touches when the path is a symlink to a folder:
  `lstat("/a/link/")` follows the link, `lstat("/a/link")` doesn't, so today a wire path with a trailing `/` stats, and
  a recursive delete walks, the link's TARGET. Dropping it names the link, the safe direction (deleting a link must
  never empty its target). Rows never carry a trailing `/` (they're built by `join`), so only wire input changes;
  milestone 1 pins today's answer on Linux and macOS and milestones 10c/10d flip it deliberately.
- **Unicode form**: never touched. `cafÃ©` in NFC and NFD are two values; on APFS and HFS+ both open one file, on ext4
  two. Equality is exact, so a typed NFC path and a listed NFD path compare unequal, as `PathBuf`s do today; a folded or
  normalization-insensitive compare is the volume's `NameRules` business.
- **Trailing dots and spaces on Windows** (decided by the lead after the type's second review, 2026-11-18): every path
  is LITERAL except text a person typed. A listed name, a verbatim or PLAIN path the OS reports (`from_native`: a
  `read_dir` child under a plain root joins `dup.` verbatim, and so do watchers, dialogs, `%TEMP%`, and a drop), an MCP
  tool's argument, a deep link, a CLI argument, and every persisted value keep every dot and space, so `dup` and `dup.`
  stay two files, and such a name has no plain form (`to_native_plain` refuses it `NotPlainSafe`; it goes through the
  verbatim one). Only text typed into the address bar or a go-to dialog reads as Explorer's address bar reads it,
  through its own constructor, `from_person_typed`: each name's trailing dots and spaces go (`C:\x\dup.` opens `dup`),
  and a name of only dots and spaces is refused `BlankName`. The UNC server and share, the drive, and the `.` and `..`
  names aren't trimmed. Review round one's M5 stripped in `from_native` too; review two (N1, N8) showed that names `dup`
  beside a listed `dup.` (a walk under a plain root, an agent echoing a listed path), so it's undone.
- **Case**: never folded, except the drive letter, uppercased (Windows reads `c:` and `C:` as one drive and the OS
  reports either). A UNC server and share keep their case as typed or reported (decided by the lead, 2026-11-18,
  question 6): `Eq` and `Hash` stay exact, and every ROOT match (the resolver picking a volume for a path, `starts_with`
  against a volume root, `RemoteRoot`-style prefix checks, `same_root`, and the frontend's `isPathOnVolume` for a
  `//server/share/` root) compares the server and share components with Unicode simple case folding, since both are
  case-insensitive on the wire (DNS and NetBIOS names; SMB share names). Below the root, case is the volume's
  `NameRules`, as everywhere. The API (added in the type's review): `same_root` and `strip_volume_prefix` /
  `is_on_volume` fold the server with ASCII case (DNS and NetBIOS) and the share with Unicode simple case folding (so
  the Kelvin sign is `k`, and `Ä°` is not `i`), compare the names below the root exactly, and compare a scheme root
  exactly (every backend mints its authority lowercase); `starts_with` / `strip_prefix` stay exact and never match
  across roots. Ran (review round 3): `\\?\UNC\LOCALHOST\d$\x` opens `\\localhost\D$\x`, and a lowercase letter in
  `\\?\d:\x` opens the drive. â Not run, and not volume-wide: Windows 10 1803+ lets a FOLDER be case-sensitive
  (`fsutil file setCaseSensitiveInfo`, what WSL sets on folders it creates), so one NTFS volume can hold `A` and `a`
  side by side in that folder. The builder refused the switch (no WSL feature installed). A per-volume `NameRules` folds
  the two together there, and the drive index's lowercase key collides; Windows plan task 35 makes it a per-directory
  property of the local Windows volume (question 13).

### Typed and pasted `..`

Review round 2. People type and paste `..` often (`../foo` from the pane, a path copied from a build log like
`/opt/app/lib/../share`), and agents do too. Refusing it is wrong UX; resolving it by text alone is wrong semantics
across a symlink on POSIX, where the OS resolves `/a/link/..` to the link TARGET's parent. The rule is "`..` means what
the volume holding the path means", answered by the volume at the input boundary, once, before the path becomes an
`AppPathBuf`:

- **One resolver**: `resolve_typed(text, base: Option<&AppPath>) -> Result<AppPathBuf, TypedPathRefusal>` in the app's
  input layer (go-to-path's module), called by go-to-path, the CLI's path arguments, deep links, and the MCP tools' path
  inputs. It expands `~`, parses with `from_external_text`'s flavor rules (`from_person_typed`'s for the address bar and
  go-to dialogs) but KEEPS `.`/`..` names as tokens, picks the volume for the path (the resolver,
  `resolve_path_to_volume`), then walks the names left to right holding a normalized prefix.
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
  timeout's answer, â never a lexical guess.
- **Pins**: milestone 1 (g) pins today's lexical answers; milestone 10c lands this as loud rule 1's second exception,
  its own commit, flipping (g) where a symlink is crossed, with new cells: `/x/link/../y` on Linux and macOS lands on
  the target's sibling; `C:/x/link/../y` on Windows (a junction) lands on `C:/x/y`; `../y` from a pane inside `/x/link`
  lands on `/x/y`; a missing prefix still reaches its nearest ancestor.

### The native boundary

`to_native(&self) -> Result<PathBuf, NativeRefusal>` and
`from_native(&Path, PathFlavor) -> Result<AppPathBuf, NotAnAppPath>`, both typed (loud rule 2; â no `Option` that a
caller turns into "not found"):

- **Scheme path**: `NativeRefusal::NotLocal`.
- **Wrong flavor**: a POSIX-shaped path on a Windows host is `WrongFlavor`, â never `\x`: Windows reads a rooted path
  with no drive as the CURRENT drive's root, so `/Users/x` would open whatever drive the process cwd is on
  (`GetFullPathNameW("/x")` answered `C:\x`; ran, review round 3). A drive or UNC path on a POSIX host is `WrongFlavor`
  too.
- **Characters NTFS and Win32 can't hold** (in the NAMES of drive and UNC paths): `:`, `*`, `?`, `"`, `<`, `>`, `|`,
  control characters, and NUL are `ForbiddenCharacter`. â `:` above all: through `\\?\` AND through a plain path,
  `C:/x/a:b` writes the alternate data stream `b` of file `a`, so a write lands INSIDE another file and no listing shows
  it; if `a` doesn't exist, the write creates an empty `a` to hold the stream; folders take streams too, and a trailing
  `a:` is `ERROR_INVALID_NAME` (ran, review round 3). NUL is refused on POSIX too (an MTP pick never reaches
  `to_native`). â The check covers names only, never the UNC server: `//[::1]/share/x` is a valid root, and both
  `\\[::1]\D$\x` and `\\?\UNC\[::1]\D$\x` open the share (ran, review round 3).
- **A UNC server with a port** (`//host:445/share`, `//host@445/share`) is `NativeRefusal::NoPortOnUnc`: the Windows 10
  SMB client takes no port in a path (both spellings answer `ERROR_BAD_NETPATH`, ran, review round 3), so that text
  belongs to the app's own SMB backend (`smb://host:445/share`), and the resolver routes it there.
- **Trailing dots and spaces, and reserved device names** (`CON`, `NUL`, `COM1`, `con.txt`, `lpt1 .x`, `CONIN$`): NOT
  refused here. Through `\\?\` they reach the exact file, the only way to open or delete one that WSL or a Linux tool
  made. Creating one is refused earlier, by `NameRules` (Windows plan task 14). Pinned both ways: `to_native` of
  `C:/x/CON` and `C:/x/a.` is the verbatim path, and create and rename refuse those names. Ran on NTFS (review round 3):
  each of those six names creates, lists, reads, and deletes through `\\?\`.
- **â The plain form names a DIFFERENT file** (ran, review round 3), which is why `to_native` is verbatim-only and why
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
  typed per-item answer ("Cmdr can't hand this name to Windows"), â never a fallback to stripping `\\?\` by hand
  (`dunce::simplified` and the like), which is exactly the wrong-file hazard above. UNC working directories are another
  limit: `cmd.exe` started in `\\localhost\D$\x` falls back to `C:\Windows` with a warning (ran), so "open a terminal
  here" on a UNC path needs `pushd` or a terminal that takes UNC. Its call sites join `app-path-native-crossing`'s
  allowlist (milestone 11). This plan ships the type and its reasons; each consumer's Windows plan task writes the words
  a person sees (question 14).
- **Length**: a pure fence at 32,767 UTF-16 units including the `\\?\` prefix is `TooLong`, but NTFS stops earlier: a
  `create_dir` at 32,750 units answered `ERROR_FILENAME_EXCED_RANGE` (206, `InvalidFilename`) where 32,649 worked (ran,
  review round 3, 100-unit steps). So the OS's 206 maps to the typed `NameTooLong` too, â never a hard-coded exact
  limit. On POSIX the OS answers `ENAMETOOLONG` as today. â The builder has `LongPathsEnabled` on (its setup script),
  user machines default it off: a plain path past 260 that a third-party crate opens works on the builder and fails for
  users (an exe without the opt-in got `ERROR_PATH_NOT_FOUND` from `GetFileAttributesW` on a 300-unit plain path, while
  `std::fs` added `\\?\` itself and succeeded; ran). Cmdr's `windows/app.manifest` declares `longPathAware`, which helps
  only with the registry switch on.
- **Escaped names**: decoded back to raw bytes (Unix) or surrogate units (Windows).
- **`from_native` refuses**, typed: a relative path, a drive-relative `C:foo` AND a bare `C:` (both mean that drive's
  CURRENT directory to Win32: `GetFullPathNameW("D:foo")` answered `D:\foo` only because `D:`'s cwd was its root; ran,
  review round 3; `parse`, `from_external_text`, and `from_person_typed` keep reading a typed `C:` as `C:/`, which is
  what a person means), `\\.\` device paths, `\\?\Volume{â¦}` and `\\?\GLOBALROOT\â¦`, a UNC server of `?` or `.`, and
  a `.`, `..`, or empty name inside a verbatim path. Win32 doesn't treat those as literal names: NTFS refuses them
  (`\\?\D:\x\.\y` is `ERROR_PATH_NOT_FOUND`, `\\?\D:\x\\y` and a verbatim `..` `ERROR_INVALID_NAME`, a verbatim `..`
  over UNC `ERROR_BAD_PATHNAME`, `\\?\D:` without its `\` `ERROR_INVALID_PARAMETER`; ran, review round 3), so the
  refusal only types what the OS would say. Today's grammar answers `None` for most of these; the conversion only types
  the reason.
- **8.3 short names are aliases** (ran on `C:`, where 8.3 creation is on, as it is by default on a Windows 10 system
  drive; the builder's `D:` has it off; review round 3). `from_native` is pure, so it can't tell `C:/x/LONGFO~1` from
  `C:/x/Long Folder Name`: `read_dir` through the short spelling returns children under it (a second app path for every
  file inside), and `canonicalize` gives back the long one. The listing never shows a short name, so a row never holds
  one; the inbound crossings that didn't come from a listing can (an OS-reported path like `%TEMP%` for an account name
  past eight characters, a drop from a legacy app, a CLI argument, a deep link, a typed or pasted path). Two rules:
  - The Windows local volume's inbound native crossings (`from_native` callers outside a listing walk) expand short
    names with `GetLongPathNameW` on the longest existing prefix first, so an OS-reported path lands on the spelling the
    listing shows.
  - â A destination NAME can alias a different file: through `LONGFI~1.TXT`, a `create_new` answered `AlreadyExists`,
    a truncating write REPLACED `Long File Name.txt`'s bytes, and a rename renamed it (ran). A source literally named
    `LONGFI~1.TXT` (from SFTP, a zip, a Linux disk) copied into that folder passes a name-folded conflict check
    (`NameRules` folding doesn't relate the two) and lands on the person's file. So on Windows the destination conflict
    scan and the landing's collision answer read the STORED name (`find_stored_spelling`, `cFileName` from
    `FindFirstFileW`), and a stored name that differs from the destination name beyond the volume's folding is a
    conflict on the stored name, â never an overwrite through the alias. Milestone 10a, with a Windows-only cell.

**Operations** (all on `&AppPath`, none allocate unless they return an owned path):

- `as_str()`, `root() -> &AppPath`, `kind() -> PathKind`, `is_root()`.
- `parent() -> Option<&AppPath>` (a slice; `None` at the root), `file_name() -> Option<&str>`, `names()` (an iterator of
  `&str`).
- `join(&self, name: ChildName) -> AppPathBuf` is THE join (one plain name, so it can't climb or replace).
  `join_relative(&self, rel: &RelativeAppPath) -> AppPathBuf` for a validated multi-name tail (the remote-root and
  `rebase` cases). â No `join(&str)`: `Path::join("/x")` replacing the base is exactly the surprise to remove.
- `starts_with(&self, ancestor: &AppPath) -> bool` (whole names, exact),
  `strip_prefix(&self, ancestor) -> Option<Cow<RelativeAppPath>>` (borrowed for canonical values), `relative_to` as the
  same thing named for readers; folded twins take `&NameRules`.
- `rebase(&self, old_root, new_root) -> Option<AppPathBuf>` (replaces `app_paths::rebase`).
- `to_native` and `from_native`: Â§ "The native boundary" (today's `path_grammar` bodies, moved, with typed refusals).
- `Display`: the wire spelling (for logs, wrapped in `LogPrivate` as today). `shown()`: what a person reads, the ONE
  formatter (Windows `\`, picks taken out through `twin_path::shown`), the twin of the frontend's `shownPath`.
- `Eq`, `Hash`, `Ord`: **component-wise, the HYBRID reading** (decided by the lead after the type's second review,
  2026-11-18; round one said POSIX rules everywhere, which broke Windows dedup, and the first review's fix said the
  host's rules everywhere, which on Windows made `/d/a\b == /d/a/b` and so hid the "`\` joined into an in-memory path"
  class this plan exists to fix). Host rules apply only to text that host's OS really opens; everything else reads with
  the app's own grammar (`/` the only separator): on macOS and Linux every non-scheme text is the OS's, so POSIX
  components (the root first, `/` as `RootDir`, which sorts below any name, so `/x` < `sftp://â¦` as `PathBuf` has it;
  a leading `//` is a plain root, as std reads it, so a sloppy `//Users` bridge value equals `/Users`); on Windows std's
  Windows components (both separators, the drive and UNC and verbatim prefixes as `Prefix`, ordered as std orders them)
  for drive, UNC, verbatim, device, `\`-led, and relative text, so a bridge value `C:\x` equals `C:/x` and `C:\x\`
  there, as `PathBuf` does, but a POSIX-kind `/x` (the in-memory volume's and the fixtures') reads with the app's
  grammar, so `/d/a\b != /d/a/b` there. Empty and `.` names skipped, `..` kept. A scheme root is ONE component on every
  host (as std components `sftp:///x` would equal `sftp://x/`). Still one function of the text, so `Eq` is an
  equivalence, `Hash` agrees, and `Ord` is total and `Equal` exactly when `Eq` holds (property-tested under both hosts'
  rules on every OS, `properties_test.rs` Â§ `hybrid_comparison`, plus the `PathBuf` property on the Windows builder for
  the text Win32 opens). On a normalized value `Eq` is the same as comparing bytes; on a bridge value made from an
  unnormalized `PathBuf` (`/a/`, `/a/./b`) it still equals what `PathBuf` equals on that host, so a `HashSet` keyed on
  the typed path dedups exactly as today (survey; property-tested against `PathBuf` on macOS, Linux, and the Windows
  builder). The cost, accepted: a UNC value on a POSIX host (made only by `parse(_, Windows)`, so test-only there)
  compares as the POSIX path with its text. Within a name, bytes compare as `OsStr` bytes do. The one known divergence:
  an escaped non-UTF-8 name (Â§ "Names that aren't UTF-8") sorts by its escape code point, above every raw byte, where
  `PathBuf` sorted the raw byte; it's a consistent total order, and no path order is persisted (the `BTree*`s are
  in-memory), so it's accepted. The bridge escapes already, so it holds for bridge values since the trait flip
  (`properties_test.rs` Â§ `bridge_unix` states it). â Nothing sorts typed paths at the flip; the first milestone that
  does (a `sort()` or a `BTree*` keyed on app paths, likely 9 or 10a) moves escaped names in that order and pins it.

**Construction**, each named and greppable, â no `From` impls:

- `AppPathBuf::parse(text: &str, flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath>`: normalizes per Â§
  "Normalization" (a `..` name is refused, not resolved).
- `AppPathBuf::from_wire(text: &str)`: `parse` with `PathFlavor::HOST`, for IPC arguments and stored strings.
- `AppPathBuf::from_external_text(text: &str, flavor, namer: Namer)`: outside text a machine handed over (an MCP
  argument, a deep link, a CLI argument) with NO `..` left (separators turned on Windows, `.` dropped, escape-range code
  points escaped as literal text through `namer`, the VOLUME's namer, which `resolve_typed` knows), every name literal;
  a `..` is refused here, since resolving it needs a volume: the app's `resolve_typed` (Â§ "Typed and pasted `..`")
  tokenizes first and calls this on each resolved prefix. `AppNameBuf::from_typed(text, namer)` is the same rule for one
  name.
- `AppPathBuf::from_person_typed(text: &str, flavor, namer: Namer)`: the same, plus, on Windows, for a drive or UNC
  path, each name's trailing dots and spaces go, as Explorer's address bar reads them (Â§ "Normalization"). ONLY the
  address bar and the go-to dialogs call it; `resolve_typed` takes which of the two its caller is.
- `AppPath::from_normalized(text: &str) -> Result<&AppPath, NotAnAppPath>`: zero-copy validation for a string that is
  already canonical for THIS host (a `FileEntry.path` a backend built); `FileEntry::app_path()` uses it. On macOS and
  Linux a drive or UNC shape is refused (`//a/b/x` there is a sloppily joined POSIX path, never a share); on Windows the
  POSIX `/` root stays valid. `from_normalized_as(text, flavor)` is the other host's, for tests.
- `AppPathBuf::from_native(&Path)` (this host's flavor; `from_native_as(&Path, flavor)` for a test of the other
  grammar), `RemoteRoot::to_app_path(...)`, `Volume::root()`, and the derivations above.
- **Names**: `AppName` (borrowed) / `AppNameBuf` hold one name in APP spelling: `AppName::from_app_spelling(&str)` takes
  a backend-spelled name (a `FileEntry.name`, escapes live), `AppNameBuf::from_native_os(&OsStr)` (and
  `from_native_bytes`, `from_native_units`) a name a native API holds (escapes made, a literal escape code point escaped
  as literal), `from_typed` a typed one. `AppPath::join` takes only `&AppName`. `ChildName` (over `&OsStr`) is the
  `std::path` world's name and has NO app-path join (decided in the type's review, 2026-11-18): reading its text either
  way would be a guess, and guessing "app spelling" respells a live escape as a literal one once escapes exist (a copy
  of `caf\xE9.txt` would land as a file literally named with U+10FEE9). Its one door is
  `AppNameBuf::from_native_os(child.as_os_str())`, explicitly native; 10a replaces `ChildName::new(&entry.name)` with
  `AppName::from_app_spelling(&entry.name)`.
- **Typed arithmetic on a bridge value is right in release builds too** (decided by the lead after the type's second
  review, 2026-11-18; the first review's fix was a `debug_assert!`, which left `C:\x\y` with no parent and under no
  `C:\x` in release, N6). `is_canonical()` picks the path: a canonical value reads its text directly; a bridge value
  (`/a//b/`, `C:\x\y`) reads through the same hybrid components `Eq` uses (`app_path/lenient.rs`). Slices (`parent`,
  `file_name`, `names`, `root`, `kind`, `is_root`) answer what `Path` answers, cut from the text; `join`,
  `strip_prefix`, `strip_volume_prefix`, `same_root`, and `rebase` read the bridge value as the canonical place it names
  (`to_canonical()`), so their output is canonical (`rebase` never builds `/new//b`). `strip_prefix` and
  `strip_volume_prefix` answer `Option<Cow<RelativeAppPath>>`, borrowed for canonical values, owned
  (`RelativeAppPathBuf`) for a bridge one. A bridge value with no canonical twin (no root, `C:x`, a device path, a `..`)
  joins with a typed refusal and sits under nothing: by text a `..` can name a different file across a symlink. A
  property holds every bridge value's `parent`, `file_name`, and `starts_with` to `Path`'s on Unix.
- Tests only (`any(test, feature = "testing")`): `app_path!("/a/b")` returns a `&'static AppPath`-like value, panicking
  on a bad literal, so the 4,800 test literals convert by sed.
- **The transition bridge, deleted in milestone 11**: `AppPath::legacy_unchecked(&Path) -> Cow<'_, AppPath>` and
  `AppPath::as_std_legacy(&self) -> Cow<'_, Path>`. Both exist so the trait can flip without a semantic change anywhere;
  each call is counted by the `app-path-std-bridge` check and goes to zero crate by crate.
  - `legacy_unchecked` wraps UTF-8 text as is, borrowed: unvalidated, unnormalized (a trailing `/`, `//`, `.`, `..`, and
    a Windows `\` all survive). Non-UTF-8 input is escaped (owned), â never lossy: the local engine's walks hand raw
    `PathBuf`s from `read_dir` to code that will call `Volume` methods, and a lossy bridge would make those calls miss
    the file, or hit a sibling named with U+FFFD. Rejected: "lossy, as the wire does" (the planner's first draft).
  - `as_std_legacy` borrows when the text holds no escape, and decodes to the exact bytes (owned) when it does.
  - The checkable proof that the bridge never normalizes (milestone 3): a property test over arbitrary `OsString`s
    (Unix: any bytes, invalid UTF-8, `//`, trailing `/`, `.`, `..`, `\`; Windows: any UTF-16 units, unpaired surrogates,
    `\`, `C:foo`, `\\?\`) that `legacy_unchecked(p).as_std_legacy()` has the same `as_os_str()` BYTES (Unix
    `as_bytes()`, Windows `encode_wide()`) as `p`. â Never assert it with `Path ==`: `Path`'s equality ignores a
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
  then `canonicalize`), â never one canonical native path against an app path, and the UNC-alias gap goes to task 35's
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
  can address a different file. Â§ "Names that aren't UTF-8".
- **`from_wire` refuses `..`; it doesn't resolve it.** Rejected: lexical resolution everywhere (the grammar's current
  behavior), which retargets a local IPC path across a symlink. Â§ "Normalization".
- **Typed `..` is the volume's answer** (review round 2). Rejected: refusing it (people type and paste it all the time),
  lexical everywhere (wrong across a POSIX symlink), and `canonicalize` (needs the whole path to exist and rewrites
  every link). Â§ "Typed and pasted `..`".
- **`FileEntry.name` is lossless too** (review round 2). Rejected: round one's "name stays lossy display text"; the
  listing protocols address rows by name (survey), so a lossy name is an address, and a resort can move a selection to
  another file. Rejected also: a separate `displayName` field (changes `bindings.ts` and every row's size for a case
  that's one in a million rows). Display goes through `shownName`, and a Linux E2E catches a sink that doesn't.
- **Escapes in plane 16, with a canonical form** (review round 2). Â§ "Names that aren't UTF-8" holds the ranges, the
  Unicode checks, and the rejected schemes (PEP 383, WTF-8, OPTU-8).
- **Typed native refusals.** Rejected: `Option` (a `None` becomes "not found" upstream, hiding an ADS-shaped name or a
  wrong-flavor path). Â§ "The native boundary".
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
  functions, and the check stays as a ban on `Path::new(<x>.as_str())` and `OsStr::new(<x>.as_str())`. Its
  `wrapperCalls` budget counts each call through a named wrapper over the bridge (`std_of`, `into_std`, `on_volume`,
  `seam_path`, `engine_path`), so the meter reads direct plus wrapper crossings (#1469), and its `stdJoin` budget counts
  the write engine's untyped `std::path` joins of a name onto a folder, the Windows `/share\name` trap.
- **The escape gate** (added in the type's review, 2026-11-18; rebuilt after its second review, the lead's decision 2).
  The hazard: app TEXT holding a live escape turned into a `std::path` value carries the escape's own UTF-8 bytes, so
  `std::fs` opens a file literally named with it (`/d/caf\u{10FEE9}` becomes four byte escapes, a "not found", or a
  write under a mangled name). For a non-UTF-8 name that's no worse than today's lossy text (which names the U+FFFD
  sibling), but for a valid-UTF-8 name holding a plane-16 code point it's a regression, so escapes must not exist until
  no app text can reach a `std::path` constructor.
  - **â A text scan can't be that gate, and doesn't pretend to be.** Review 2 (N2) found the first scan's 101 sites a
    fraction: no lexical rule sees app text arrive through a `let`, a closure, `.into()`, an `impl AsRef<Path>`
    parameter, or a `format!` join. So `app-path-std-bridge`'s text budgets are a TRIPWIRE and a meter: `appTextToPath`
    (a `FileEntry.path`/`.name` or an `.as_str()` handed to `Path::new`, `PathBuf::from`, `OsStr::new`,
    `OsString::from`, or `ChildName::new`; 101 when seeded) and `pathFromBareText` (a bare path- or name-shaped
    identifier, `PathBuf::from(&path)`, `Path::new(&dest_dir)`, or `.map(PathBuf::from)`; 973 when seeded in the second
    review, many native). While either is above zero the check refuses a production escape producer, whatever its
    spelling: a call, a function pointer (`.map(AppPathBuf::from_native)`), or a bare `escape_bytes(` after a `use`. It
    also refuses outright (production and tests) OS text handed to an app-spelling door on one line
    (`AppName::from_app_spelling(os.to_str()?)`, `from_normalized(&p.to_string_lossy())`; N5).
  - **The sound gate is the type's**, in two halves. (1) **No conversion**: no app-path type implements `AsRef<Path>`,
    `AsRef<OsStr>`, `AsRef<str>`, `Borrow<str>`, `Deref<Target = str>`, `Into<PathBuf>`, `Into<OsString>`, or
    `Into<String>`, held at compile time (`app_path/tests.rs`, `no_app_path_type_converts_to_std_path_or_its_text`), so
    `Path::new(&p)`, `PathBuf::from(p)`, `.into()`, and an `impl AsRef<Path>` parameter fed a TYPED value don't compile.
    That's sound for every carrier that is typed, so 10aâ10e make every carrier of app text typed: `FileEntry.path`
    and `.name` (10f's first commit, before any producer), every IPC parameter (`from_wire` at the command edge, 10c),
    and every store read. (2) **The exits, banned by type**: the only ways back to text are `as_str`, `into_string`, and
    `Display`. 10f's second commit adds them to `clippy.toml`'s `disallowed-methods` (type-resolved, so a function
    pointer or a `let` hop can't hide one) and replaces `Display` with a `wire()` view under the same ban, with a
    per-site `#[expect(clippy::disallowed_methods, reason = "â¦")]` only at the IPC serializer, the store writers, and
    `LogPrivate`. With both halves on, an escape can reach a `std::path` constructor only through a reviewed `expect`.
  - **Until then, the non-escaping crossing**: `AppPathBuf::from_native_unicode` and
    `AppNameBuf::from_native_os_unicode` are `from_native` for a name that is plain Unicode text, and a typed refusal
    (`NotAnAppPath::NotUnicode`) for one that would need an escape. They make no escape, so they aren't producers.
    Milestones 9, 10a, and 10b cross with them; on `NotUnicode` the caller keeps that child native (its `PathBuf`, or a
    bridge value) and counts it, â never drops it. 10f swaps each for `from_native`.
  - `legacy_unchecked` escapes a non-UTF-8 path too, so the bridge stays lossless, and it isn't counted as a producer:
    its escapes go back to the OS through `as_std_legacy`, the transition's only exit for a bridge value, and its text
    exits fall under (2). Counting it would block milestone 4, the flip, which is all bridge calls.
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
- **Then 4 alone** (the flip), with the smallest freeze (Â§ 4 "Freeze").
- **Then 5 alone** (`cmdr-fs` internals): every backend builds on `RemoteRoot` and `root_anchored`.
- **Wave B, four parallel worktrees**: 6 (network backends), 7 (device backends), 8 (virtual volumes and leaf crates
  that depend only on `cmdr-fs`), 9 (`cmdr-local`). â Their crates are disjoint, but their DEPENDENTS aren't:
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
- **Then 10f** (the escape lands; added in the type's review, 2026-11-18): every escape producer, which used to ride 6,
  7, and 9, waits for the type-level escape gate and for the text budgets to reach zero (Â§ "Keeping it that way"), so
  6, 7, and 9 convert their bodies WITHOUT producing escapes (SFTP and NFS keep today's answer for a non-UTF-8 name, the
  local readers their lossy row), and 10aâ10e drive the text-to-path sites to zero.
- **Then 11** (close).

### 1. Pin today's behavior, and the Windows targets

- **Size**: S.
- **Scope**: tests only, plus test helpers. (a) Extend `path_grammar_cases.json` with arithmetic rows (join a name,
  parent, filename, names, starts-with, strip-prefix, rebase, shown) for every root shape: `/`, `C:/`,
  `//server/share/`, and scheme roots for sftp (rooted at `/` and at a subfolder), webdav, s3, nfs (`%2F` export), mtp
  (with a pick segment), adb, restic, and an archive-inner path (`/x/a.zip/inner`). Rows carry the POSIX answer (pinned
  from today's `std::path` behavior on macOS and Linux) and the Windows answer (the target, asserted against the new
  type in milestone 3 on every OS, since it's pure). (b) Pin the current string helpers on macOS/Linux: `root_anchored`
  (including its relative and alias inputs), `RemoteRoot::{to_remote_path, to_app_path}`,
  `app_paths::{path_under, rebase}`, `twin_path::{folder_of, shown}`, `ChildName::under`, `volume::ids` exports with a
  trailing slash, `scan_walk`'s join, `patching`'s parent match, `InMemoryVolume::canonicalize` through a link. Many
  exist; add what's missing, name each so it reads as behavior. (c) The allocations-per-entry ceilings (Â§ "Cost"). (d)
  Ordering and equality pins: a `BTreeSet<PathBuf>` of `/a/b`, `/a-b`, `/a.b`, `/a b`, `/a`, `/a/b/c`, `/Ã©`, `/z`, and
  `sftp://h/x` iterates in today's order, and a `HashSet<PathBuf>` holds `/a` and `/a/` as one. (e) For each store in Â§
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
  and through `from_external_text`. (h) **Unicode form**: on macOS (APFS) and Linux, a file created as NFD `cafÃ©` and
  addressed as NFC: pin today's stat and listing answers. (i) **The Windows baseline**: the builder's full
  `cargo nextest run --workspace` with per-test outcomes, saved as `docs/notes/windows-port/app-path-baseline.md` (the
  failing test names grouped by crate and cluster, nothing else). Every later milestone diffs against it, so "turned
  green" and "newly red" are lists, not counts. Review round 4's lessons from the existing logs: (1) read outcomes from
  a nextest JUnit report (a `ci`-style profile with `junit.path` set, `--no-fail-fast`), never from console output:
  PowerShell wrapped the earlier logs at 98 columns and two are UTF-16, so names and panics had to be stitched back
  together; (2) tag each failure with its cause cluster and owner from Â§ "Windows failures, mapped", including the ones
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
    25 with a `note` where the two differ on purpose. This branch proposed a scheme root of `scheme://authority` (no
    trailing `/`, today's `std` parent byte for byte); the type, built in parallel, spells it `scheme://authority/`, and
    the integration (2026-11-18) kept the type's: every root ends in `/`, today's `RemoteRoot::app_root` of a
    server-rooted volume is already `sftp://u@h:22/`, and `Eq` holds the two spellings equal, so a parent never
    mismatches its volume's root. The three scheme-parent rows carry that `note`, and `join("C:/x", "a\\b")` names the
    type's real refusal, `Backslash`. Since the merge, the pin asserts every row against the type itself.
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
    drive and UNC roots (8). â Two are live data loss, not only Windows debt: with the folder open in a pane, a local
    delete of a folder holding `caf\xe9.txt` reports success and deletes nothing (#1236), and the raw IPC `delete_files`
    of `x/link/` (a folder symlink with a trailing `/`) emptied the link's target. `main` fixed the second at the
    engine's starts (`crates/cmdr-operations/src/wire_paths.rs`, `80dd3735cc`), so its delete cell
    (`a_delete_of_a_folder_link_with_a_trailing_slash_removes_only_the_link`) now pins the fixed answer.
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
- **Scope**: `cmdr_fs::app_path` per Â§ "The type": `AppPath`, `AppPathBuf`, `RelativeAppPath`, `NotAnAppPath`, the
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
- **Landmines**: `Eq`, `Hash`, and `Ord` are component-wise (Â§ "Design decisions"), and `Hash` must agree with `Eq` on
  unnormalized bridge values. A pick segment (`â4242`) is a valid name. Under `PathFlavor::Posix` `\` and `:` are name
  characters. `from_normalized` must reject every non-canonical spelling (property test: `from_normalized(s).is_ok()`
  iff `parse(s) == s`). The escape (Â§ "Names that aren't UTF-8") lives here, with `shown()`'s U+FFFD rendering and the
  canonical form; the frontend twin learns it in this same milestone. `to_native`'s refusals are typed (Â§ "The native
  boundary"). Keep it `wasm32`-clean (no `std::fs`; the escape works on bytes and `u16` units, not `OsStr`, so it
  compiles everywhere and Linux CI tests the Windows arm).
- **Test plan**: the case table from milestone 1 runs against the new type for BOTH flavors on every OS; property tests
  for normalize idempotence, `parent`/`join` inverse, `starts_with`/`strip_prefix` agreement, and `Eq`, `Hash` equality,
  and `Ord` equal to `PathBuf`'s on arbitrary POSIX-shaped text (including `//`, trailing `/`, `.`, `..`, space, `-`,
  `.`-led names, non-ASCII). The bridge's byte-identity property (Â§ "The type", "never normalizes"). The escape:
  `from_native` then `to_native` is the identity on arbitrary bytes (Unix) and arbitrary `u16` units (Windows, tested as
  data on every OS), two names differing only in invalid bytes get different spellings, a name holding a literal escape
  code point round-trips, `to_native(canon(s)) == to_native(s)` and `from_native(to_native(s)) == canon(s)` on arbitrary
  strings, `from_external_text` of text holding an escape-range code point names that LITERAL character, and a byte
  escape is `ForeignEscape` under `to_native` for Windows (a unit escape under POSIX). `to_native` refusal rows: `/x` on
  Windows, `C:/x/a:b`, `C:/x/a*b`, NUL, a 33,000-unit path, `//host:445/share/x` (`NoPortOnUnc`), and acceptance rows
  for `C:/x/CON`, `C:/x/a.`, and `//[::1]/D$/x` (the server's `:` is no name character). `to_native_plain` refusal rows
  (`NotPlainSafe`): `C:/x/a.`, `C:/x/a `, `C:/x/con.txt`, `C:/x/lpt1 .x`, `C:/x/CONIN$`, a 260-unit path; acceptance
  rows `C:/x/a.b` and `C:/x/console`. `from_native` refusal rows: `C:foo`, a bare `C:`, `foo`, `\\.\PhysicalDrive0`,
  `\\?\Volume{â¦}\x`, `\\?\GLOBALROOT\x`, `\\?\C:\x\..\y`, `\\?\C:\x\.\y`, `\\?\C:\x\\y`. Unicode: NFC and NFD `cafÃ©`
  are unequal values. Windows builder: `cargo nextest run -p cmdr-fs` green with the new module.
- **DONE**: the module is complete, documented, and unused outside tests, `path_grammar` delegates to it, and the
  frontend's grammar suite still passes the shared table.
- **Done** (2026-11-18, branch `worktree-agent-afa89bb10a0f54613`, on milestone 2's branch): `cmdr_fs::app_path` with
  `AppPath`/`AppPathBuf`, `RelativeAppPath`, `AppName`/`AppNameBuf`, the escape, the pure native conversions plus the
  host `to_native`/`to_native_plain`/`from_native` behind a `platform/` seam, the bridge, serde/specta, `app_path!`,
  `testing::AppTestDir` (+ `TestDir::new_in`), `app-path-std-bridge` (empty allowlist), the frontend's
  `lib/path/escape.ts`, the `escapes`/`escapedUserInput` table sections, and the docs (`crates/cmdr-fs/DETAILS.md` Â§
  "App paths"). Where the code differed from this plan, deliberately:
  - `join` takes an `AppName` (a canonical name type), and is fallible: a drive or UNC path refuses a name holding `\`
    (`NotAnAppPath::Backslash`), the source-listing-from-another-OS case. `ChildName` (over `&OsStr`, used by the
    transfer engine) stays as is, with no app-path join (Â§ "The type", Names; the review found the `under_app` this
    milestone first shipped respelling a live escape).
  - `path_grammar`'s text helpers keep their exact answers (lexical `..`, the frontend twin's semantics; the shared
    table pins them), so "delegates" means: its old struct folded away and its callers (`tilde`,
    `local_windows/paths.rs`, the Windows volume arm, a conformance helper) moved to the text helpers, which changes no
    behavior on any OS. The typed `to_native`'s refusals reach the local Windows volume in milestone 9, as planned.
  - The typed path reads a one-letter scheme as a drive (`C://x` is `C:/x`, what `from_person_typed` makes of a typed
    `C:\\x`); the text helpers keep the old reading.
  - Comparison follows the host's `Path::components` (Â§ "The type", `Eq`), a scheme root as one component. A typed
    `strip_prefix` never matches across roots.
  - A typed escape-range code point is escaped through the namer of the path it lands in (unit escapes on a Windows
    drive or UNC path, byte escapes elsewhere), not always byte escapes: NTFS can hold the literal scalar, and byte
    escapes there would be `ForeignEscape`.
  - No `starts_with_folded`/`eq_folded`: `NameRules` carries no case fold today (case sensitivity is
    `LocationInfo.case_sensitive`), so the folded twins wait for a consumer. `same_root` (the UNC root fold) is here.
  - The review round on the type (2026-11-18) added: the host-flavored comparison and `from_normalized`, the root check
    in `strip_prefix`, `strip_volume_prefix` / `is_on_volume`, Win32's trailing-dot reading of plain and typed Windows
    text, the UNC server checks, a case-free `\\?\UNC\`, `from_user_input`'s `namer`, `extension`, `file_stem`,
    `with_file_name`, `from_native` host-only, the escape gate, and the `app_path_native` fuzz target. Verified:
    `cargo nextest run -p cmdr-fs` on macOS (1,054 tests), the allocation cells (`join` one allocation, today's row path
    three, arithmetic and bridge zero), `clippy`, the zig cross-check
    `cargo check --all-targets --target x86_64-pc-windows-gnu` for `cmdr-fs` (with `testing`) and `cmdr-local`, the wasm
    build of `cmdr-fs`, every Desktop (Rust) and Desktop (Svelte) lane check with `--ci --fresh`, `clippy-linux`,
    `rust-tests-linux`, and on the Windows builder (`7eba6045c3`) `cargo nextest run -p cmdr-fs --features testing` 858
    of 858 (26 skipped: the `#1225` ignores milestone 5 lifts) and
    `cargo clippy -p cmdr-fs -p cmdr-local --all-targets -D warnings` clean.
  - The second review round (2026-11-18, integrated on one branch with milestone 1's pins and `main`; the lead's six
    decisions) changed: trailing dots and spaces stay literal everywhere but `from_person_typed` (`from_user_input`
    split into it and `from_external_text`; N1, N8); the hybrid comparison (N4); release-correct arithmetic on bridge
    values through `lenient.rs`, with `strip_prefix` answering a `Cow` and a new `RelativeAppPathBuf` (N6); the escape
    gate rebuilt as the type's compile-asserted half plus a tripwire scan (`pathFromBareText` beside `appTextToPath`),
    with `from_native_unicode` / `from_native_os_unicode` for 9, 10a, and 10b (N2, N3); `AppName::new` renamed
    `from_app_spelling` and the OS-text hop refused (N5); the operation log's escape mark and `from_literal_store` (N7);
    the scheme root kept as `scheme://authority/` against milestone 1's proposal (milestone 1's record says why). Per
    finding: `docs/notes/windows-port/app-path-type-review-fixes.md`.

### 4. Flip the trait

- **Size**: L (mechanical, one branch, every crate; the lead plus eight agents in five tiers, below).
- **Scope**: every path-carrying `Volume` method takes `&AppPath` (`&[AppPathBuf]` for batches) and returns
  `AppPathBuf`/`&AppPath`, except link targets (Â§ "Design decisions"). The path-carrying types beside the trait
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
    `root_anchored`) instead of silently testing a different path. Those few become `legacy_unchecked(Path::new(â¦))`
    by hand and are counted like any other bridge.
  - **Types beside the trait that serialize keep their field types** in this milestone (`ScanConflict` and every DTO
    stay `String`/`PathBuf`); only the in-process ones (`CopyScanResult`, `BatchScanResult`, `routed.rs`,
    `root_echo.rs`, `mkdir_all.rs`) move, and a map keyed on their paths keeps `PathBuf`'s dedup because `Hash`/`Eq` are
    component-wise.
  - **The codemod makes it boring** (review round 2). The flip is three artifacts, all on the branch:
    1. The hand-written trait change in `cmdr-fs` (`volume/mod.rs`, the in-process types beside it, and `cmdr-fs`'s own
       impls and tests), by the lead. Small and read in full.
    2. The codemod (Go; removed with milestone 8, last in the tree at `4fcd2231d0`): a compiler-driven fixpoint. It runs
       `cargo check -p <crate> --all-targets --message-format=json`, and rewrites only the spans rustc points at, by
       error code: `E0053` on a `Volume` impl method (the param or return type: `&Path`â`&AppPath`,
       `&[PathBuf]`â`&[AppPathBuf]`, `PathBuf`â`AppPathBuf` with `.into_owned()` on the returned bridge), plus the
       body prelude `let path_std = path.as_std_legacy(); let path: &Path = &path_std;` ONLY when the body names the
       param (else `unused_variables` reddens clippy); `E0308` at a call whose expected type is `&AppPath`: a canonical
       string literal inside `Path::new(â¦)`/`PathBuf::from(â¦)` becomes `app_path!(â¦)`, anything else
       `&AppPath::legacy_unchecked(&â¦)`; a returned `AppPathBuf` used as a `Path` gets `.as_std_legacy()`. Re-run
       until the crate checks or no rule applies. Two rules settled in the type's third review (2026-11-18), both pinned
       in `crates/cmdr-fs/src/app_path/flip_shapes_test.rs`:
       - **A borrowed return** (`root() -> &AppPath`, `stands_in_for_own_disk()` and
         `local_backing_file() -> Option<&AppPath>`): `&AppPath::legacy_unchecked(&self.root)` is a temporary (`E0515`),
         so on an `E0515` or `E0308` in such a method the codemod retypes the FIELD the method returns from `PathBuf` to
         `cmdr_fs::app_path::BridgedPath` (native path plus its app spelling, made once, lossless) and returns
         `self.root.app()` (`.then_some(self.archive_path.app())`, `.as_ref().map(BridgedPath::app)` for an `Option`
         field). Every body use of the field compiles unchanged through `Deref<Target = Path>`; each constructor's
         `root: path` becomes `root: BridgedPath::new(path)` (an `E0308` there); a `.clone()` or move of the field into
         a `PathBuf` slot is residue (`.to_path_buf()`). A root that delegates to a helper (`self.root.app_root()`,
         `RemoteRoot`) retypes the helper's field the same way and the helper returns `&BridgedPath`. A literal root
         (`Path::new("/")`, the test volumes) becomes `AppPath::from_normalized("/").expect("a canonical literal")`, a
         `&'static AppPath` in production too. A root with no field to hold it (`self.root.path()` on a `TempDir`) gains
         a `BridgedPath` field beside it in its constructor: residue. â No borrowed `legacy_unchecked_ref`: it has no
         answer for a non-UTF-8 mount point. `BridgedPath::new` is counted by `app-path-std-bridge` like the bridge's
         two functions, and each crate's own milestone turns the field into a plain `AppPathBuf`.
       - **A `std::path` method on a receiver the flip typed** (`let r = volume.root(); r.starts_with(&pathbuf)`, the
         in-process types, a `listing_path` result): the `E0308` "expected `&AppPath`" rule applies ONLY when the callee
         is a `Volume` method or a helper listed beside the trait. When the callee is an `AppPath` / `AppPathBuf` method
         that `Path` also has (`starts_with`, `strip_prefix`, `relative_to`, `join`, `parent`, `file_name`, `extension`,
         `file_stem`, `with_file_name`, `ends_with`, `components`, `rebase`), the codemod rewrites the RECEIVER to
         `.as_std_legacy()` and leaves the argument native, so `Path` answers exactly as today; â never the argument
         to `legacy_unchecked`. The two answer differently on bridge values (`/a/../b/c` under `/a`, a relative
         `photos/x` under `photos`, `//Users/d/Docs` under `/Users`: `Path` says yes, the typed method no), so the lift
         would be a silent behavior change. Same for residue: an app-path value meeting `Path` arithmetic always drops
         to `as_std_legacy()`. The residue audit greps `legacy_unchecked(` used as an argument to
         `starts_with|strip_prefix|relative_to|rebase|is_on_volume|same_root|join|ends_with`, and every hit is a
         finding. The typed twins are for each crate's own later milestone, which flips its pins on purpose.
         Type-driven, so a native `Path::new` fixture that never meets a `Volume` method is never touched (a textual sed
         would have rewritten all 4,800 literals, native ones included).
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
    3. After it lands, a branch that was in flight rebases and runs the same codemod (it stayed in the tree for exactly
       this), then its own residue by hand.
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
- **Tier 0 done** (2026-11-18, branch `worktree-agent-a6403541e656ed355`): artifact 1 (`cmdr-fs`), artifact 2 (the
  codemod), and `cmdr-archive` as the worked example; the tier agents' runbook was a handoff note in
  `docs/notes/windows-port/`, removed with the codemod (both last at `4fcd2231d0`). Where the code differed from this
  plan:
  - `local_path` flips to `Option<AppPathBuf>` (it's the volume's root, an app path); `read_link_target`'s answer and
    `create_symlink`'s `target` stay `Path`. "`CopyScanResult`'s `source_path`" is `ExtractedFile.source_path`.
  - The helpers beside the trait (`fold_batch`, `tally_by_listing`, `ScanBoundary::subtree`, `root_echo`,
    `ResolvedVolume::passthrough`, `create_directory_all`, `MakesDirectories`, `PatchSource::patch_display_path`) keep
    `std::path` signatures and bridge inside, so no backend call to them changes. `ScanSource::scan_stat`/`scan_list`
    and `PatchSource::patch_stat` DID flip, in tier 1: their impls forward to a typed `Volume` read, and a bridged
    argument there is a temporary the returned future borrows (the handoff note says why no `async move` could fix it).
    `RemoteRoot::app_root` answers a `BridgedPath`, per the borrowed-return rule.
  - The codemod needed rules beyond the four listed: a typed param handed back (`path_app`, so a delegating test double
    returns the typed param, not a temporary), an `async fn` helper's call moved into its future (E0515), `assert_eq!`
    operands dropped to std (rustc points inside the std macro, so the call site is lifted out of the expansion),
    imports, and a clippy phase applying clippy's own fix for the lints a rewrite causes. Body errors are skipped while
    their method's E0053 is pending (rustc checked them against the old signature).
  - **No typed twin compiles by accident** (decided by the lead after tier 0, 2026-11-18): the methods `AppPath` shares
    with `Path` (`parent`, `file_name`, `extension`, `file_stem`, `with_file_name`, `join`, `starts_with`,
    `strip_prefix`, `relative_to`, `rebase`) moved off the inherent impl onto `cmdr_fs::app_path::PathArithmetic`, so
    `x.file_name().is_some()` on a typed `x` is E0599 (the codemod's receiver rule then drops `x` to `as_std_legacy()`)
    instead of silently answering the typed way. Only `cmdr-fs`'s app path module and its pins import the trait. Each
    crate's own conversion milestone (5â10e) imports it where it adopts the typed methods on purpose, and flips its
    pins in the same commit; â no import before that. Chosen over clippy `disallowed-methods`: seven crates lift that
    lint for their whole test build (`#![cfg_attr(test, allow(clippy::disallowed_methods))]`, for the clock and spawn
    bans), so tests would have escaped it, and a type-level gate needs no lint run. Pinned by `cmdr-archive`'s
    `a_typed_root_has_no_path_shaped_arithmetic_in_reach`.
  - The work comes in passes: a crate that dev-depends on itself builds its tests only once its lib checks, so the
    commits alternate codemod output and residue (two codemod passes and one residue commit for `cmdr-archive`); the
    audit replays them in order rather than one `residue.patch`.

### 5. `cmdr-fs` internals

- **Size**: M.
- **Scope**: `RemoteRoot` holds `AppPathBuf`s and does `AppPath` arithmetic (`to_remote_path(&AppPath)`,
  `to_app_path -> Option<AppPathBuf>`); `normalize_remote_path` stops using `Path::components` (server paths are POSIX,
  so a `/`-only normalizer); `app_paths.rs` folds into `AppPath` methods; `root_anchored` takes a wire string and
  returns `AppPathBuf` (callers with a typed path drop it); `scan_walk`, `patching`, `ids`, `twin_path`, `child_name`,
  `in_memory` (root, overrides, link resolution) convert. All `cmdr-fs` bridges go. Delete the codemod (its
  in-flight-branch window is over). â `RemoteRoot` and `root_anchored` are `pub` and every backend calls them: per the
  wave-B rule, add the typed forms beside the old and leave the backends' calls to 6, 7, and 8.
- **Intentions**: the shared vocabulary every backend builds on is correct on Windows.
- **Landmines**: `RemoteRoot`'s component-wise prefix match (`â¦:2222` vs `â¦:22`) and "`..` before the check" are
  data- safety rules; the new code keeps both and its tests stay. NFS app roots must keep `%2F` (no decoding).
  `InMemoryVolume` is the contract oracle: â don't relax a contract to green a test.
- **Test plan**: Windows builder, the 24 `cmdr-fs` tests ignored for #1225 lose their `cfg_attr` and pass:
  `volume::remote_paths` (7), `volume::scan_walk` (9), `volume::patching` (3), `in_memory` canonicalize (1),
  `child_name` (1), `ids` (1), `twin_path` (2). macOS/Linux: pins unchanged.
- **DONE** (the lead's sign-off, 2026-11-19): every `#1225` ignore in `cmdr-fs` is gone (24 cells run on every OS); the
  shared arithmetic is typed, with typed twins beside the `std::path` forms other crates still call
  (`crates/cmdr-fs/DETAILS.md` Â§ "App paths" lists them); the bridge count fell 217 â 211 and what's left leaves with
  the milestones below. What converted: `RemoteRoot` and `normalize_remote_path` (POSIX bytes split on `/` alone, plus
  `app`/`to_remote`/`to_app`, the last a typed `RemoteAppRefusal` for a server name that needs an escape),
  `anchor_at_root` beside `root_anchored` (a typed `AnchorRefusal`: a `..`, or another volume's path), the scan walk,
  the listing patcher, and `unpicked_app` (one `ListedFolder` join rule, the folder checked once per level),
  `twin_path`'s typed twins, `InMemoryVolume::canonicalize` (encoded bytes split on `/`), and `ChildName` (host flavor:
  on Windows `\` and `:` refused).
- **Intended behavior changes, on every OS** (each its own commit, beside loud rule 1's three exceptions):
  - A walk under a top spelled `/dot/.` asks for `/dot/sub` where it asked for `/dot/./sub`: the same folder, by its
    canonical spelling (the milestone-1 pin flipped).
  - A listed name that isn't one plain name (`""`, `.`, `..`, `a/b`, a NUL) fails a walk as `InvalidName`, naming the
    name and the folder that listed it. Safer than before: `Path::join` of `""` or `.` re-listed the same folder
    forever, `..` walked the parent (which holds the folder again), and `a/b` counted a different subtree. No real
    server lists one: S3, NFS, and ADB drop them at the listing; SFTP, WebDAV, git, and restic only from a hostile or
    broken source, whose copy `ChildName` already refused. A skip would let a delete or a copy call a partial folder
    whole, so it fails.
- **What still bridges, and the milestone that takes it** (each goes with its last caller; converting it here would
  raise the callers' budgets):
  - The old `std::path` forms' one crossing each: `RemoteRoot`'s `app_root`/`to_remote_path`/`to_app_path` (6: SFTP,
    WebDAV, NFS, S3; 7: ADB; 10c/10d: the app), `root_anchored` (6, 7: SMB and ADB, 9: `cmdr-local`, 10a, 10c/10d), and
    the scan walk's `std::path` entry points (6, 7: ADB, SMB, MTP, 8: git, restic, archive, 9, 10a).
  - The `ScanRecorder`/`ScanOutages` callbacks: they take `std::path` and live in the engines (10a).
  - `InMemoryVolume`'s `PathBuf`-keyed store and builders (`with_root(impl Into<PathBuf>)`), used by every crate's tests
    (11, after the last crate's tests move).
  - The `conformance` suites (113), driving other crates' volumes through their fixtures: `cmdr_fs::volume::conformance`
    flips to app paths in 11's first commit (9 converted `cmdr-local` without it), and each backend's
    `conformance_test.rs` crossings go in that commit.
  - `tally_by_listing`, `mkdir_all`, `root_echo`, `routed`: they read `FileEntry.path` text (10f).
  - `app_paths.rs`: its string helpers are right on Windows already and `AppPath` has the typed answers (`strip_prefix`,
    `rebase`, `is_on_volume`); it goes when its callers move (8: `cmdr-operation-log`, 10a, 10c/10d: the app).
- **The codemod** stayed until no open PR merged `main` through it, and went with milestone 8, its handoff note too.
- The three pin files' `#1225` comments stay: they pin `std::path`'s own answers, Unix-only by design.

### 6. Network backends: SFTP, WebDAV, S3, NFS

- **Size**: M (four crates, one agent with up to three subagents).
- **Scope**: each crate's bodies drop their bridges and do `AppPath` arithmetic; server paths come from `RemoteRoot`.
  `cmdr-nfs`' app-side pieces (the identity seam, places) stay where they are.
- **Landmines**: S3 keys aren't paths: `/` is a delimiter by convention and a key can end in `/` (a folder marker) or
  hold `//`. Keep the key layer string-based and convert only at the app-path edge. â Two keys that normalize to one
  app path (`a//b` and `a/b`, `a/./b`, a key with a `..` segment) must not both list: an op on the shared path would hit
  the other object. The colliding or unrepresentable key is skipped with a typed count (as NFS skips a non-UTF-8 name),
  pinned by a test with both `a/b` and `a//b` in one bucket; first pin what today's listing does with them. WebDAV
  `href`s are percent-encoded; decode before `to_app_path`, never after, and a decoded name holding `/`, or equal to `.`
  or `..`, is dropped and counted (`mapping::members`), in a relative or an absolute-URL href alike. SFTP and NFS names
  arrive as raw bytes: each backend either escapes a non-UTF-8 name (Â§ "Names that aren't UTF-8") or skips it with a
  typed count, â never `from_utf8_lossy`; pin today's behavior first and tell the lead which it picks. Escaping is the
  default for SFTP (byte escapes in its own alphabet, decoded back to bytes for the server); WebDAV and S3 are
  UTF-8-only, so a name carrying any escape is `NotRepresentable` on write (the cross-namer rule). SFTP overrides
  `parent_for_dot_dot` in 10c's commit, not here. â The escaping itself lands in 10f (the escape gate); here the
  bodies convert and a non-UTF-8 name keeps today's answer.
- **Test plan**: Windows: `cmdr-sftp` 7 (`paths_test`), `cmdr-webdav` 23, `cmdr-s3` 42 plus the 5 pause timeouts,
  `cmdr-nfs` 45 (30 with a `\` in the path, and 15 fake mounts answering `ExportNotFound`, not 7: the export is picked
  by a prefix match over the typed path). Not here (review round 4): `cmdr-nfs`'s three `errors_test` cells and
  `cmdr-adb`'s `an_unlisted_device_errno_arrives_as_its_linux_class`, a product bug, not a path (Â§ "Windows failures,
  mapped"); `cmdr-s3`'s `nanoseconds_survive_and_trailing_zeros_drop` (Windows' `SystemTime` holds 100 ns); `cmdr-nfs`'s
  `apply_metadata_dates_and_modes_a_folder` (modes) unless the baseline shows it path-shaped. macOS/Linux: each crate's
  suite plus its Docker fixture lane where CI runs one.
- **DONE**: the four crates' bodies bridge nothing (the 5 test-side sites left each have an owner, below) and their
  Windows suites are green but for the 5 non-path cells named in the Windows results.
- **Done** (2026-11-19, branch `worktree-agent-ac797eccf4593af1f`; the lead on SFTP, one helper each on WebDAV, S3, and
  NFS): every body and crate helper takes `&AppPath`, server paths come from `RemoteRoot::to_remote`, and the server
  side is POSIX text split on `/` alone (a stat's name came from `Path::file_name`, which splits on `\` on Windows).
  Bridge calls 911 â 5 (SFTP 232 â 0, WebDAV 194 â 2, S3 339 â 2, NFS 146 â 1), `appTextToPath` 5 â 0,
  `pathFromBareText` 53 â 7 (the `conformance` suites' `std::path` arguments and two fixture key or temp paths).
  Decisions:
  - **`root()` and every row's `FileEntry.path` keep `RemoteRoot`'s own text** (`app_root`, `to_app_path`), â never
    the typed `to_app`'s canonical one: NFS's export root is `nfs://h:2049/Public/` with its trailing `/` on the wire
    (`server-path-utils.ts` compares the root by string), and a canonical root drops it. Pinned
    (`the_export_root_keeps_its_trailing_slash_on_the_wire`; S3's rows as literal strings,
    `a_rows_text_is_the_normalized_server_path_under_the_prefix`). S3's `root()` alone is a typed value built at connect
    (`paths::Place`), the same text as `app_root`, which is why its place refuses text the type can't hold (below). The
    typed `to_app` answers only the listing patcher (`patch_app_path`), whose matches are `PathBuf`'s component-wise
    ones; a server path whose text needs an escape keeps its row and only loses its patch (the pane re-lists).
  - **`cmdr-fs` gained typed cores for the shared bodies**: `patching::patch_*_at` with `PatchSource::patch_app_path`
    (the `std::path` forms stay for ADB and the in-memory panes, 7 and 11), and `mkdir_all` typed only
    (`MakesDirectories::remote_path_of(&AppPath)`, `create_directory_all_at`), since its four implementors are these
    crates. Each crossing goes through one named helper, so neither file's budget rose; the listing host, which files
    listings by `std::path`, is `patch_mutation_at`'s one crossing until the app's host converts (10c).
  - **Tests name files through the root** (`testing::app_at`/`at`/`child` per crate), since a typed path is absolute;
    what a volume does with relative or `..` TEXT a `std::path` caller still sends is pinned through `RemoteRoot`'s
    `std::path` form, the same rules, and a bare server path as a typed POSIX value.
  - **NFS's poller keys what it saw by server path** (the store's spelling and the listing's typed path both come down
    to it), so two spellings of one shown folder share one `GETATTR` and both hear its change.
  - **Non-UTF-8 names keep today's answer** (no escapes, `ESCAPES_LIVE` stays `false`): SFTP's crate still fails the
    session on one, NFS skips it counted. Until escapes go live, `RemoteRoot::to_remote` sends an escape-range code
    point as its own UTF-8 bytes, the `std::path` door's answer
    (`until_escapes_go_live_an_escape_range_name_reaches_the_server_as_its_literal_text`); decoding it addressed a
    look-alike sibling (`caf\u{10FEC3}\u{10FEA9}` reached `cafÃ©`). â For 10f: once live, `to_remote` decodes through
    `from_utf8_lossy`, so an escaped name must be refused there (`NotRepresentable`) for WebDAV and S3 before escapes go
    live, or a key could map to a U+FFFD sibling.
- **Intended behavior changes** (each red first):
  - WebDAV: a member a server names `.` or `..` (a `%2E%2E` href) is no row. It listed as the folder itself or its
    PARENT, so deleting that row acted on a folder nobody picked; this section said "dropped as today", but it wasn't
    (`paths_test::a_member_named_dot_or_dot_dot_is_no_row`). The same holds in an absolute-URL href (IIS): `decode_href`
    keeps dot segments as sent, where `url::Url` resolved `â¦/box/%2e%2e` to the parent and listed it under the
    parent's name. And a member holding an encoded `/` (`a%2Fb`) is no row either: decoded, it listed as `b`. Both are
    counted (`mapping::members`, a debug line per listing), pinned per server spelling with crafted `multistatus` bodies
    (`members_test.rs`).
  - S3: a listed name that can't be addressed (empty, `.`, `..`, the collisions `a//b`, `a/./b`, `a/../b`) was already
    skipped, and now the skip is counted (`PageChildren::unaddressable`, a debug line per listing;
    `listing_test::names_that_cant_be_addressed_are_counted`). Today's answer is pinned first (`key_collision_test.rs`:
    only `a/b` lists, and an op on it reaches only `a/b`). What lists doesn't change. A folder holding only such keys
    lists empty, and its delete now says why it refuses (`VolumeError::HoldsUnlistedItems` â
    `WriteOperationError::FolderHoldsUnlistedItems`, "looks empty, but the storage keeps items in it under names Cmdr
    can't show"), where it answered a bare `NotEmpty`. â Not done, and more than a small change: the LISTING saying "N
    items can't be shown" needs a count on the listing result (`Volume::list_directory` returns rows alone) carried
    through `cmdr-listing`'s events to a quiet pane note; no backend has that road today (NFS's non-UTF-8 skips and
    WebDAV's members are debug lines too). It wants its own issue, after 10c moves the listing host to app paths.
  - Edge cases with no real-world source: an S3 bucket or key id holding a NUL or an escape-range code point
    (U+10E800âU+10EFFF, U+10FE80âU+10FEFF) refuses the connect (`NoSuchBucket` / `KeysRejected`), since `root()`
    needs a typed value; no provider allows one, and non-ASCII buckets and key ids connect as before
    (`paths_test::a_place_the_app_path_cant_hold_refuses_the_connect`).
- **What still bridges, and who takes it**: `testing::scratch_dir` in WebDAV and NFS, the `std::path` twin the app's
  backend suites call (10d switches its callers and deletes both); the `conformance` crossings in WebDAV's and S3's
  `conformance_test.rs` (11, whose first commit flips `cmdr_fs::volume::conformance` to app paths); S3's
  `live_hostile_test.rs` `dotted`, two live cells that hand the volume a `..`, which only a bridge value can spell (11,
  which turns them into pins that a typed path refuses `..` before any request).
- **Windows results** (2026-11-19, the builder, `--profile windows-baseline`, the four crates only, 0 self-skipped):
  `main` at `450e3ab9f2` 56 red (milestone 5 had already taken 71 of the 127 the triage counted here), this conversion
  at `31f8887eca` 5 red, none path-shaped: NFS's three `errors_test` cells and
  `mutation_test::delete_removes_a_file_an_empty_folder_and_a_link_but_never_recurses` (Win32 145 for `NOTEMPTY`), and
  S3's `nanoseconds_survive_and_trailing_zeros_drop`. Counts per crate:
  `docs/notes/windows-port/windows-test-failures.md` Â§ "By milestone".
- **Verified**: clippy `-D warnings` (default and `--all-features`), nextest `cmdr-fs` + the four crates 1,963 passed
  (328 Docker or live cells skipped), `cmdr` checks against them, the zig cross-checks for Windows and Linux,
  `pnpm check --fast`, `app-path-std-bridge`, `file-length`, the Docker fixture lane for the four crates, and the
  Windows builder (above).

### 7. Device backends: SMB, MTP, ADB

- **Size**: M.
- **Scope**: as 6. `cmdr-smb`'s `to_smb_path` and anchored mounts; `cmdr-mtp`'s storage paths and pick segments;
  `cmdr-adb`'s device paths.
- **Landmines**: SMB maps `\` and other forbidden characters into the private-use area in `smb2`; an app path name with
  `\` must reach that mapping intact. MTP picks (`twin_path`) are the one NUL-bearing name. An SMB share mounted by the
  OS on macOS (`/Volumes/x`) is a POSIX-shaped app path through `LocalVolume`, not `smb://`. SMB names are UTF-16 on the
  wire: pin what `smb2` does with an unpaired surrogate today (lossy, or refused); the target is unit escapes, which may
  need an `smb2` change (David's crate, so it's fine to make; file it there). The unit escapes themselves land in 10f.
  `cmdr-smb`'s UNC root matching folds server and share case (Â§ "Normalization", Case).
- **Test plan**: Windows: `cmdr-adb` 51 (the fake device answers `errno 30` to a write whose path gained a `\`, so those
  are paths too), `cmdr-smb` 13 (the anchored-mount prefix arithmetic over `/Volumes/â¦`), `cmdr-mtp` 1 (plus
  `host_seam_test`'s mutation count, to diagnose: it may not be a path).
- **DONE**: bridge counts zero; Windows suites green.
- **Done** (2026-11-21, branch `worktree-agent-a82b4cda924d76541`, PR #1302; the lead on SMB, one helper each on ADB and
  MTP): every `Volume`, `ScanSource`, and `PatchSource` body and crate helper takes `&AppPath`, server and device paths
  are `/`-split text, and nothing reads an app path through `std::path` but the named crossings below. Bridge calls 854
  â 13 (SMB 691 â 10, ADB 102 â 2, MTP 61 â 1), `appTextToPath` 12 â 0, `pathFromBareText` 249 â 6 (SMB 3,
  ADB 3: the adb binary's native path). The durable design: `crates/cmdr-smb/DETAILS.md` Â§ Decisions,
  `crates/cmdr-adb/DETAILS.md`, `crates/cmdr-mtp/src/connection/DETAILS.md`. Decisions:
  - **Row and root text stay today's** (milestone 6's rule): SMB rows keep `to_display_path`'s text, ADB's root the bare
    `adb://<serial>` (a typed root would be `adb://<serial>/`, and a pane compares the root by string), MTP's rows their
    storage URL text. Only the listing-cache patches are typed.
  - **SMB reads paths by their text**: `smb_path_of` keeps the root aliases, a bridge value's relative text, and
    `Path::strip_prefix`'s whole-component match and remainder trim, byte for byte; a share path's name splits on `/`
    alone (`smb_file_name`). The copy-scan walk joins by `ListedFolder` (the shared walk's rule, `InvalidName` for a
    name that isn't one plain name).
  - **MTP's session layer is typed too**: storage paths are canonical app paths under a POSIX `/` (one door,
    `connection/storage_path.rs`), so the path â handle and listing caches key one object by one value. That is what
    `host_seam_test`'s mutation count and `same_name_test`'s twin rename were: `Path::join` wrote `\` on Windows.
  - **Escape-range text stays literal until escapes go live** (`RemoteRoot::to_remote`'s rule) in SMB and ADB; MTP
    decodes it for lookups and new names (`device_name`, `device_spelling`). â For 10f, and a regression from `main`
    before milestone 6 (verified with a probe, review round): the app's only constructor is `legacy_unchecked`, which
    escapes a literal escape-range code point into four byte escapes, so an app-origin value holding one reaches SFTP,
    WebDAV, S3, NFS (since 6 landed), SMB, and ADB as escape text (`NotFound`), where `as_std_legacy` decoded it back;
    MTP's `listing_path` and report URLs carry the literal text, which the bridge reads as other bytes. Milestone 6's
    review flipped `to_remote` to literal on the premise that a row's text reaches a volume as is, which the app never
    does today. Fail-closed, and no real name holds plane-16 private use. #1310 holds the fix (one escaping door for
    listed names, decoding at every `to_remote`) and its pins.
- **Byte-exact evidence** (throwaway differential harnesses against today's `std::path` code on macOS, kept in the
  lead's scratchpad): SMB 5,075,828 cases (wire path, share file name, the walk's listing round trip, patch parent and
  name, the walk's child join), 0 differences; ADB 33,146,552 (device path, row paths and names, patch paths, `root()`),
  0; MTP 9,274,006, with 8,222 differences on canonical input, all in three classes: a device name holding a literal
  escape-range code point (8,151: the listing path and notify URLs now carry the row's literal text, where the bridge
  re-escaped it; the 10f note above), another device's URL handed to this storage's volume (69, never in practice), and
  the storage root as an object (2: its parent `mtp://mtp-0-1/` against `mtp://mtp-0-1`, the same listing). Bridge-only
  input (`//`, `/./`, a trailing `/`, `..`) now answers the canonical place, the same one by components; SMB's patch
  parents differ in spelling only there (and a pane's listing key is canonical, so the patch lands where it used to
  miss). Docker: `rust-integration-tests` scoped to `cmdr-smb` and `cmdr-adb` green (two cells starved under load passed
  alone).
- **Intended behavior changes** (each red first): MTP, an object a device names `..`, ``, `.`, or `a/b` lists no row (it
  listed `("..", "/Documents/..")`, a path naming the folder's parent), and a folder holding only such objects, which
  lists empty, refuses its delete as `HoldsUnlistedItems` (S3's refusal for its unlisted keys, review round; a tree
  delete takes what a path names first), â never deleting the unlisted object. On `main` such a child's delete
  answered `ObjectNotFound` (`Path::file_name` of the joined path never matched), so no object was ever deleted through
  it either way. SMB's walk refuses such a listed name as `InvalidName`, as the shared walk does (no SMB server lists
  one).
- **What still bridges, and who takes it**: SMB's mount root (`MountAnchor.mount_path`, a native OS mount point, held as
  `BridgedPath`), `rerooted` (the registry promotes between native mount points), the watcher (its paths are
  listing-host keys), `mutation::listing_key` and `scan::for_boundary` (the listing host and the `ScanRecorder`), and
  `testing`'s `std::path` forms the app's suites call (`hash_volume_file`, `cleanup_test_prefix`): 10c/10d. ADB's root
  `BridgedPath` (the bare `adb://<serial>` text, which only the bridge holds) and one conformance cell that hands in a
  `..` (11). MTP's `connection::std_path_for_host`, every `&Path` host-seam call (the listing host, `ScanBoundary`,
  `ResolvedMtpObject.path`): â a new per-file entry (`connection/mod.rs`, 0 â 1) that replaces 61 calls over 12
  files, the one budget line that grew (the check's own rewrite reproduces it, review round). It goes with its last
  seam: the listing host's `directory_changed` / `authoritative_listing` (10c), `ScanBoundary`'s `ScanRecorder` (10a),
  and `ResolvedMtpObject.path`, which `cmdr-index`'s host reads (10b). The app's `UnverifiedSmbMount`
  (`network/smb_mount_gate.rs`) is the app's: its crossings are into the volume manager's `std::path` API (10c).
- **Landmines checked**: smb2 maps `\` and the other forbidden characters to the private-use area and back, so a listed
  `a\b 12:00.txt` reaches it intact (pinned, `paths_test`). smb2 today REFUSES a name with an unpaired surrogate: every
  UTF-16 name decode is `String::from_utf16` mapped to `invalid_data`, so one such entry fails the whole listing (and a
  watcher batch). Filed as vdavid/smb2#47 for 10f's unit escapes. The UNC root fold waits for Windows direct sessions,
  which don't exist: a mapped drive stays on the OS mount.
- **Fixed in the review round** (MTP): `resolve_handle_to_path` cached a walked leaf under the shared name, so an
  `ObjectAdded` twin took the name and the existing row's delete reached the newcomer, against the #523 rule
  (`PathHandleCache::learn_resolved`, `a_handle_walk_never_ties_a_shared_name_to_one_twin`); a create, upload, or rename
  whose name needs an escape got no listing patch
  (`host_seam_test::a_new_object_whose_name_needs_an_escape_patches_its_pane`).
- **Windows**: `clippy-windows` green; the CI Windows lane (compile only) on the PR. On the builder afterwards
  (2026-11-21, `main` at `97befe7c07`, `docs/notes/windows-port/windows-test-failures.md` Â§ "By milestone"): 64 of the
  66 cells this milestone owns are green (`cmdr-smb` 13 â 0, `cmdr-mtp` 2 â 0, `cmdr-adb` 52 â 3). The two path
  cells left, `conformance_test::create_directory_all_{refuses_a_file_in_the_way,goes_through_a_link_to_a_folder}`, are
  the shared conformance assertions' `std::path` join (`notes.txt\inside`), milestone 11's; the third is the Linux errno
  cell, owned elsewhere.
- **Verified**: nextest `cmdr-smb`, `cmdr-adb`, `cmdr-mtp` 615 passed (190 Docker skipped); clippy `-D warnings`
  (workspace `pnpm check clippy`, the crates with `--all-features`); `pnpm check --fast`, `app-path-std-bridge`,
  `file-length`, `jscpd-rust` (three new trait-boilerplate pairs, each with its reason, the backends' bodies now
  token-identical without their preludes: `cmdr-mtp` â `cmdr-smb` `volume_impl.rs` 19 lines, `cmdr-adb` â
  `cmdr-webdav` `volume_impl.rs` 108, `cmdr-adb` â `cmdr-sftp` `mutation.rs` 21; the ADB â archive pair went).

### 8. Virtual volumes and leaf crates

- **Size**: M.
- **Scope**: `cmdr-archive` (boundary detection, URL-rooted entry paths; the local backing file crosses at `to_native`),
  `cmdr-git` (portal paths app-side, `git2` native), `cmdr-restic` (hub and volume; fixtures stop rooting at
  `/Volumes/â¦` as native), `cmdr-viewer` (markdown link and image resolution as `AppPath` arithmetic; opening a local
  file crosses at `to_native`), `cmdr-listing` (and `cmdr-brief-columns`), `cmdr-operation-log` (scheme-rooted dir
  prefixes), `cmdr-search` (its two `Path` lines). `cmdr-multi-rename` and `cmdr-listing-wasm` moved to 10e (they depend
  on `cmdr-operations`).
- **Landmines**: `cmdr-archive`, `cmdr-listing`, and `cmdr-operation-log` are `cmdr-operations` dependencies: the wave-B
  rule holds (no `pub` signature changes; typed forms beside). The operation log's dir tree must keep
  `reconstruct_dir_path`'s exact output for every POSIX and scheme row (pinned), and must GAIN the Windows roots:
  `intern_dir`, `find_dir`, and `reconstruct_dir_path` take the `AppPath`'s root as ONE name, the way
  `split_scheme_root` keeps `scheme://authority` (`C:` reconstructs `C:/â¦`, `//srv/share` reconstructs
  `//srv/share/â¦`). Today `C:/x` comes back as `/C:/x` and `//srv/share/x` as `/srv/share/x`, a different path the
  rollback would act on (verified in `store/dirs.rs`, review round 1). No migration: no Windows install has rows, and
  POSIX rows keep their bytes (the root `""` row and the `/`-led names are unchanged). Restic snapshot paths are POSIX
  paths from the repository, whatever OS made the backup. Archive entry names come through the archive crate's sanitizer
  (`read/name/`); a name it can't make UTF-8 (a CP437 zip) keeps whatever the sanitizer answers today, pinned.
- **Test plan**: Windows: `cmdr-archive` 5 (its other two, the zip's Unix mode bits reading `0o666`, aren't paths),
  `cmdr-git` 1 (un-ignore), `cmdr-viewer` 13, `cmdr-listing` 2 (one is `foreign_path_test`; the stall-attribution one is
  not #1225 and may stay), `cmdr-operation-log` 3, and `cmdr-restic` 55 plus 9 timeouts (review round 4; the rest of
  that run's 61 and 10 are `cmdr-listing-wasm`'s, 10e). New on every OS (the store is pure SQL): intern, find, and
  reconstruct round-trip `C:/x/y`, `C:/`, `//srv/share/x`, and `//srv/share/`, and a rollback plan for a drive-rooted op
  targets the drive path.
- **DONE**: bridge counts zero in these crates; the named Windows clusters green; the operation log round-trips every
  root shape.
- **Done** (2026-11-21, branch `worktree-agent-a7270d9360407bb2c`, PR #1303; the lead on `cmdr-operation-log` and
  `cmdr-listing`, one helper each on `cmdr-restic`, `cmdr-archive`, and `cmdr-git` + `cmdr-viewer`; `cmdr-search` and
  `cmdr-brief-columns` held no `std::path` value to convert). Bridge calls 336 â 29 (`cmdr-archive` 100 â 6,
  `cmdr-restic` 132 â 2, `cmdr-git` 35 â 1, `cmdr-viewer` 8 â 2, `cmdr-listing` 49 â 17, `cmdr-operation-log` 12
  â 1), `appTextToPath` 9 â 1, `pathFromBareText` 37 â 23; no budget rose anywhere. The codemod and its handoff
  note went first (`scripts/app-path-flip/`, last at `4fcd2231d0`). What converted:
  - **The operation log's roots**: on Windows a drive (`C:/`) and a UNC share (`//server/share/`) are each one name in
    the dir tree, ending in `/` (no real name can), under the host's rules only, so POSIX rows keep their bytes; the
    leaf joins as text (`join_leaf_text`). The rollback planner's acts read and compare app paths, crossing from the
    journal's `PathBuf` once (`rollback/target.rs`' `app_path_of`).
  - **Archive, restic, git, viewer**: bodies and helpers do `AppPath` arithmetic, `BridgedPath` roots became
    `AppPathBuf`, the local `.zip` crosses at `to_native` once, `git2`/gix get native paths at `to_native`. Typed forms
    beside the old: `ResticVolume::new_at`, `ResticRepos::{confirm_repo_at, serves_confirmed_at, overlay_row_at}`,
    `FixtureRepo::{write_into_at, sync_into_at}`; `cmdr-archive`'s `new_at` and six boundary `_at` forms stay
    `pub(crate)` (its surface cap has no headroom; each turns `pub` with its first outside caller); `cmdr-git` adds none
    (its docs forbid callerless `pub`; 10c/10d add them).
  - **The listing engine**: a watcher's removal or rename matches the row by name (`mutate::last_name`), never
    `parent.join(name)`; its API, host seams, and `ListingPath` still carry `std::path` for the app's listing host, so
    each volume call crosses once.
- **Byte-exact on macOS and Linux** (throwaway differential harnesses, old `main` code against new, generated inputs):
  operation-log interning and leaf join 239,004 cases, 0 differ; the rollback planner's containment and case-twin search
  3,575,605, differing only in the two intended changes below; listing removal 489,354, 0; `cmdr-archive` 3,873,528, 0
  (2,080 set aside: a typed split answering `None` where the old split's root text named no file, so its confirm
  answered `None` too); `cmdr-restic` 1,834,880 answers over 200,000 requests with roots built the app's way, 0 (69,581
  from root shapes no route makes); `cmdr-viewer` 2,386,272, 0 outside the intended classes; `cmdr-git` 8,565, 0 for
  canonical roots and real names.
- **Intended behavior changes, on every OS** (each pinned, red on `main`; all for input no real operation produces):
  - Rollback: a recorded path holding a `..` is outside every root (`Failed`, where `Path::starts_with` credited
    `/Volumes/x/../y` as already gone under `/Volumes/x`); a case twin directly under a remote root lists that root as a
    volume spells it (`sftp://nas/`, where `Path::parent` said `sftp://nas`); two roots never twin.
  - Viewer: a decoded link name that is no name (`a%2F..%2F..`, `bad%FF`, `%00`) resolves nowhere (it climbed past the
    root, or opened the U+FFFD sibling); a picture target holding an escape-range code point is blocked; a link through
    an escaped `/` names its file canonically (`a/b`, was `a//b`). The `text_urls` pin went with the deleted wasm text
    twin it tested.
  - Git: a tree name that is no name (`..`, empty, NUL) makes no row (it became the parent folder or `/abs`).
  - Restic: a parent `data/` listing's name that isn't one plain name is skipped, never joined.
- **Windows**: `pnpm check clippy-windows` green on the merged branch. On the builder afterwards (2026-11-21, `main` at
  `97befe7c07`, `docs/notes/windows-port/windows-test-failures.md` Â§ "By milestone"), green: `cmdr-archive` 11 â 0
  (the zip-creator pair too), `cmdr-git` 136 of 136 (the un-ignored cell passes), `cmdr-operation-log` 3 â 0,
  `cmdr-listing` 3 â 1 (`stall_test`, as expected), `cmdr-restic` 67 â 1, `cmdr-viewer` 13 â 1 â 0. The viewer's
  last cell was its test's: the wire pins opened files by native text, which `from_wire` refuses, so a Markdown picture
  read `blocked` (fixed in the test, `26e751e85c`; 402 of 402 after). Restic's last,
  `read_stream::tests::the_chunks_come_out_in_order`, has no path cause left: it passes alone and runs past the 8 s cap
  under parallel load on the 2-core box.
- **What still bridges, and who takes it**: the listing engine's 16 crossings and the archive host routing's (10c); the
  rollback planner's one (`RollbackUnit` and `InverseAct` carry `PathBuf` for `cmdr-operations`, 10a, which also builds
  the typed unit through the escape mark's `StoredPathReading`, before 10f); restic's and git's host seams (10c/10d);
  the viewer's host seams (10d); archive's `ScanRecorder` crossing (10a); the test-only crossings into `std::path`
  fixtures (`conformance`, `InMemoryVolume::set_stat_failing`, 11).
- **Found on the way, not fixed here**: restic can't browse a repo on a `scheme://` parent (`restic_route_candidates`
  reads components, so `sftp://h/x` becomes `sftp:/h/x`; #1308); 10c's typed root fixes it. `cmdr-restic`'s
  `tree::child` `AmbiguousName` error names the wrong path (kept byte-exact). â On macOS `cmdr-fs`'s lenient reading
  of a bridge value led by `//` with three or more names is a UNC root (`//a/b/c`: kind `Unc`, root `//a/b/`, parent
  `//a/b/`) while `//a/b` reads as POSIX (root `/`), which disagrees with Â§ "App paths"' "a leading `//` is a plain
  root" on POSIX (verified on macOS 27 at this branch's tip, a throwaway example). Only bridge text reaches it; none of
  this milestone's crossings makes such text. Its fix needs a design call, since every Windows-arm test on a POSIX host
  builds such a value: #1309.

### 9. `cmdr-local`

- **â The escape moved to 10f** (the type's review, 2026-11-18): this milestone converts the bodies and the one
  crossing per OS, but its readers don't produce escapes yet; the non-UTF-8 row stays lossy, as today, until the
  consumers stop turning app text into `std::path` values (Â§ "Keeping it that way", the escape gate). Every typed
  crossing here is `from_native_unicode` / `from_native_os_unicode` (watch events included), never `from_native`: on
  `NotUnicode` the reader keeps today's native handling for that child, counted. Read every "escaped" and every
  "`from_native`, which escapes" below as 10f's.

- **Size**: M (review round 2 added the frontend wiring and the Linux E2E).
- **Scope**: both volumes take `&AppPath` natively: `resolve` and `local_windows/paths.rs` collapse into
  `AppPath::to_native` (the Windows `resolve_under` goes, since callers no longer pass relative paths); `reading.rs`
  builds `FileEntry.path` with one `AppPath::join` and `FileEntry.name` from the same escaped name;
  `rename_local_exclusive` gains a `&AppPath` form beside the `&Path` one (the wave-B rule; 10a switches its callers).
  Watch events coming back from the OS go through `from_native_unicode` (10f: `from_native`). Frontend, same land: every
  name sink renders through `shownName` (the listing's Full and Brief name cells, the rename prefill and its "unchanged"
  check, search results, the selection summary, drag images), the backend's menu labels and Brief text widths through
  `shown_name`, and `validateFilename` measures with `nativeNameLength`.
- **Intentions**: the one local crossing per OS, and the listing hot path at one allocation per entry.
- **Landmines**: a non-UTF-8 name on Linux: stat through the raw `OsStr` (fix `fdfa5b58cb`) as today, and the row's
  `path` and `name` become the ESCAPED spelling (Â§ "Names that aren't UTF-8"), `name == path.file_name()`. That's loud
  rule 1's first exception: its own commit, with `reading_parity_test::a_non_utf8_name_lists_lossily` rewritten to say
  the path and name round-trip (the one pin this plan edits on purpose, with the lead's OK), the frontend sinks in the
  same commit (milestone 3 already shipped the helpers), and milestone 1's pins (f)(7)â(8) flipped: the resort keeps
  the selection on the row the person picked, the cursor and `check_rename_validity` address the right file, and the
  rows sort at their escape code point's place (accepted, pinned). The Windows reader's `child_path` stops joining
  lossily (an unpaired surrogate escapes too). `scan_for_conflicts_impl` builds the destination from the source PATH's
  `file_name()`, â never the lossy `item.name`. Watch events from the OS go through `from_native`, which escapes, so
  an event for a non-UTF-8 name patches the row it listed. The two listers stay field-for-field identical. Run the alloc
  ceilings and benches (Â§ "Cost"). â Tests hand `cmdr-local` `tempfile`'s NATIVE `C:\Users\â¦\Temp\.tmpXYZ` where an
  app path belongs, and `paths::native` refuses it as "not an absolute Windows app path". Two entry points, both typed
  `&Path`/`impl Into<PathBuf>` today: the free function `reading::list_directory_core(&Path, â¦)` (59 of the app run's
  65 panics with that message, by name: "the dir lists", "the scratch dir lists") and a `LocalVolume` rooted at or asked
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
  distinct paths. `desktop-e2e-linux`: the display catch-all (Â§ "The escape on the wire and in the frontend": no escape
  code point in the DOM's text, `caf\u{FFFD}.txt` shown, delete of that row removes `caf\xe9.txt` and leaves the
  sibling). That E2E is the first end-to-end proof for #1236; close #1236 when 10a also lands.
- **DONE**: `cmdr-local` has no bridges, `rename_local_exclusive` accepts an app path on Windows, and no
  `to_string_lossy` builds a path in `cmdr-local` (names for display only).
- **Done** (2026-11-19, branch `worktree-agent-ad3a188e5a54f70b7`; the durable design is `crates/cmdr-local/DETAILS.md`
  Â§ "App paths in and out"):
  - What converted: both volumes' bodies take `&AppPath` and cross once per call at `AppPath::to_native`
    (`LocalPosixVolume::os_path`, `LocalWindowsVolume::locate`); the reader builds rows by `AppPath::join`
    (`reading/row.rs`) and stats through the raw OS name; `resolve_under`, `child_app_path`, and the Windows
    `path_grammar` crossing are gone; the Windows anchoring rule is `platform/anchor.rs`, table-tested on every OS.
    Typed forms beside the old: `local_folder_at`, `on_mount_at`, `rename_local_exclusive_at`. The `&Path` entry points
    (`list_directory_core`, `get_single_entry`, `rename_local_exclusive`, the mutation patch, space, the old
    constructors) take a native path, on Windows in either spelling through `from_native_unicode` (8.3 names expanded at
    an old constructor's root). No escape is produced: a name that isn't Unicode text keeps today's lossy row, counted
    in a debug line (10f).
  - Where the code differed from this plan: no typed twin of `list_directory_core` (the crate's surface cap has no
    headroom, and taking a native `C:\â¦` path at the old entry point is what turns its callers' native-tempdir panics
    green); the frontend name sinks, `nativeNameLength`, the Linux DOM catch-all, and every "escaped" cell above ride
    10f, as this section's head says; the alloc ceiling holds at 9 on macOS (`getattrlistbulk`: the typed join's one
    `String` replaces the lossy path's, the name's stays) and Linux's `read_dir` row drops one allocation (ceiling
    pinned at 13, not lowered here); `index_benchmarks` and `transfer_benchmarks` were not run (no M1 run this round).
  - Intended behavior change, on every OS, its own cell seen red first: the conflict scan refuses a source name that
    isn't one plain name (`..`, `/x`, `a/b`) as `InvalidName`, where `Path::join` asked about the destination's parent
    or another path. The refusal is that item's alone (logged; the batch's other items are still checked), and a name
    holding escape code points stays one plain name, checked by today's text join (review fix, 2026-11-19: one such name
    used to fail the whole batch). Review fixes, no expectation changed: every row built from an app path passes
    `rows_can_carry`, so `get_metadata` and a mutation patch answer the listing's lossy text for a name that needs an
    escape (they had put the escape on the wire, a second row beside the listing's), and an old Windows constructor
    reads the disk for 8.3 names only for a root holding a `~`. On Windows only (no install has data): a relative path
    is anchored by app-path names (a `..` refused, where `resolve_under` resolved it lexically), a name holding `:` is
    `InvalidName` before any OS call, and the `&Path` entry points take native `C:\â¦` paths they refused before.
  - What still bridges (24 calls, from 214), each with its milestone: the POSIX fallback for bridge values (`os_path`,
    the root's `os_of`; 11), the old constructors' root (10aâ10d), `notify_mutation`'s bridged parent (10a), the
    `ScanRecorder`'s caller spelling (10a), the listing host's `&Path` (Windows `notify_mutation`; 10c), and the test
    cells that hand in bridge shapes on purpose (`wire_path_shape_test.rs` 6, 10c; the relative and alias cells in
    `local_posix_test.rs` 8 and `local_windows_test.rs` 3, 11). The text budgets: `appTextToPath` keeps 3 (the sidecar
    filter's `OsStr::new(&entry.name)`, a bulk-test and a parity-test row read, 10f), `pathFromBareText` 4 (native mount
    probes and space, outside the conversion).
  - Verified: macOS `cargo nextest run` for `cmdr-local` (154), `cmdr-operations` and `cmdr-restic` (2,752), and the app
    (6,807), all green; clippy `--all-targets -D warnings` on macOS (the four crates) and, through the zig cross-check,
    for Windows and Linux (`cmdr-local`); `pnpm check --fast`, `fuzz-compile`, `module-cycles`, `clippy`. Windows
    builder: `cmdr-local` 30 of 30 on `main` before, 40 of 40 after (8 skipped both times, the task-35 ignores);
    `cmdr-operations` 190 failing of 1,763, from the triage run's 299, none newly red by this branch
    (`docs/notes/windows-port/windows-test-failures.md` Â§ "By milestone"). The 33 task-35 `MountedVolumeInside` cells
    stay task 35's: the guard reads the mount table, which answers "can't say" on Windows.

### 10a. `cmdr-operations`

- **Size**: L (1,910 prod and 4,557 test `Path` lines, 108 test volumes).
- **Scope**: the volume engine and the local engine take `AppPath`; the local engine's fast paths (`local_disk/`, the
  copy strategies, trash, cloud trash, the move sweep, validation) cross at `to_native` per OS call. The in-flight
  ledger keeps native `PathBuf`s. Un-gate the `#![cfg(unix)]` test files whose only POSIX-ism was `LocalPosixVolume`
  (milestone 2's list).
- **Landmines**: "EVERY local write lands via `overwrite::stage_and_land_file`": the landing's paths become app paths,
  the temp's stays the engine's. `DestNameIndex` and `item_identity` compare names, not paths: keep them on `&str`, fed
  from the path's `file_name()` (escaped, lossless; `FileEntry.name` equals it from milestone 9 on, but the path is the
  canonical source), â never from a SHOWN name, or a copy of `caf\xe9.txt` lands as `caf\u{FFFD}.txt`, possibly over a
  real file of that name. Switch `rename_local_exclusive`'s callers to the typed form and delete the `&Path` one; same
  for every wave-B typed-beside pair this crate calls. A destination that can't store a source name's escapes refuses
  that item typed and counts it on the op's summary (the cross-namer rule). Every mutating start already drops a
  trailing `/` and `/.` from its caller's paths (`crates/cmdr-operations/src/wire_paths.rs`, the stopgap for the raw IPC
  `delete_files` of `x/link/` that emptied the link's target); once the starts take `AppPath` from `from_wire`, delete
  that module and its call sites, keeping `trailing_separator_tests.rs` green. Rollback planner and journal rows carry
  app paths into the operation log (pinned). Big enough to split by subagent (transfer, delete and trash, archive edit,
  rename and create, scans), sequential on one branch. Non-UTF-8 safety (milestone 1's pins (f)):
  - The native walks (`scan_walker.rs`, `delete/walker.rs`, the move sweep, `scan_dry_run.rs`, `reversal.rs`) stay
    native inside, and any path they hand to a typed API goes through `from_native_unicode` until 10f (Â§ "Keeping it
    that way"): a child it refuses (`NotUnicode`) goes on as a bridge value (`legacy_unchecked`, lossless), counted, and
    10f turns each crossing into `from_native`, which escapes. â A walk never drops a child it can't type: there's no
    such child, by construction; if a backend's walk does skip one, the skip is counted and reported on the op, never
    silent, because a copy that reports success while missing a file is the data loss a person can't see.
  - `walk_cached_entries` (the listing-oracle short-circuit) builds each child from `entry.path` via `from_normalized`,
    which carries the escape after milestone 9; that fixes today's bug where a pane open on the folder made the scan
    record a lossy, nonexistent path. Its `is_left_out` check takes the path's `file_name()`, not `entry.name`.
  - The move sweep's ledger stays the safety net: it deletes only what landed, so a child the copy missed stays.
  - Windows aliases (Â§ "One file, several app paths"): `validate_destination_not_inside_source` canonicalizes both
    sides natively after `to_native`; the destination conflict scan and the landing's collision read the stored name, so
    a source named `LONGFI~1.TXT` meets `Long File Name.txt` as a conflict on that name (Â§ "The native boundary", 8.3).
  - The Recycle Bin, once Windows has one (today `move_to_trash_sync` refuses `TrashNotSupported` there), goes through
    `to_native_plain`, so a name that can't survive the plain form is refused per item, never recycled as its neighbor.
  - **A remote name holding `:` or `\` copies to Windows.** The engines join listed names with `std::path` through
    `ChildName` (`merge.rs`, `rename_merge.rs`, `landing.rs`, `copy_concurrent_source.rs`), and on Windows that join
    splits `a\b` into a subfolder and reads `12:00 notes` as a stream, so today such a name is the destination refusing
    it, asked per item (Skip / Skip all / Stop), through `DestinationNames::native_join_refuses`
    (`cmdr_fs::volume::only_native_join_refuses`), â never the whole transfer ending. This milestone switches those
    joins to `AppPath::join` with a typed `AppName`, so the name lands per the escape rules (Â§ "The native boundary": a
    remote or in-memory destination keeps it as named; a local NTFS one refuses it typed, per item, through
    `to_native`), then deletes `native_join_refuses`, its `DestinationNames` arm, and their cells
    (`refused_class_tests.rs`' `*_native_join_refuses`), keeping the per-item outcome for NTFS. `rename_merge.rs` (a
    same-volume move) still ends the move on such a name today; it switches here too.
- **Test plan**: Windows: the 289 failures' path clusters (review round 4, from the builder's log: 68 name the
  finalize's native `C:\â¦` refusal, 16 more a native tempdir handed to `LocalVolume`, about 45 local-engine flows
  whose panic carries no cause but whose siblings show the finalize, 37 a `\` joined into an in-memory or remote app
  path, mostly `transfer::volume`, and 25 `archive_edit::remote` cells over a remote fixture), plus a Windows-only cell
  in a tempdir on the system drive (the builder's `TEMP` is on `C:`, where 8.3 is on; the cell skips, saying so, when
  `GetShortPathNameW` returns no short name): a copy of a file named `LONGFI~1.TXT` into a folder holding
  `Long File Name.txt` reports a conflict and leaves its bytes alone. Not in scope, about 60 that stay red here: the
  `MountedVolumeInside` refusals (25, task 35's mount table and file id); the in-flight-temps test ledger that can't be
  emptied while a connection holds it (25, "start the test ledger empty: Access is denied", #1226's family); node
  identity answering `Unverifiable` (about 8: `ledger::tests`, `reversal::tests`'s drift cells, `source_binding::tests`,
  task 35's `file_identity`); `delete::trash`'s missing-path cell (no Recycle Bin yet); and `archive_edit::fresh_zip`'s
  Unix mode bits. macOS/Linux: full suite, `transfer_benchmarks` before/after.
- **DONE**: bridge count zero in `cmdr-operations`; a local copy and move land on the builder's NTFS.
- **First pass done** (2026-11-21, branch `worktree-agent-ae2be2908d00b0d4c`, PR #1313; the lead on the tests and
  `transfer/volume/`, helpers on the delete, rename, create, and scan-preview engines, `archive_edit/`, the staged
  writes, and the top-level files with the rollback executor). A clean partial, not the whole milestone. Bridge calls
  981 â 478, `appTextToPath` 28 â 26, `pathFromBareText` 236 â 118; no other crate's budget rose. What it is:
  - **Cross once per value**, where a body crossed once per call: a struct holds its paths' app spelling made once
    (`DisplacedDestination::new`, `StagedWrite`'s `Temp` and `Spelled`, with `target_app()`), a walk step crosses its
    folder once, and the delete, rename, create, and remote-bulk engines cross each row once. Every volume call gets the
    same `legacy_unchecked` value as before, made earlier, so these are byte-exact by construction.
  - **Typed test doubles and fixtures**: canonical literals are `app_path!`, runtime canonical text
    `AppPath::from_normalized` (it panics on a non-canonical one, so no cell silently tests another path); doubles that
    fold or record do it on the typed path. Doubles and cells handed RELATIVE or `..` text keep `std::path` on purpose:
    they pin what the engine does with bridge text, and the typed `join` refuses a relative folder.
  - **The failure seam**: `PathedVolumeError` and `AtPath` take any `FailedAt` path; an app path crosses back through
    one named impl (`transfer/volume/DETAILS.md` Â§ "Naming the item that failed").
- **Why it stops here**: the engine's shared vocabulary (`PathedVolumeError`, the created ledger, probes,
  `in_flight_temps`, `StagingTemp`, the journal and listing-host seams) holds `std::path`, and its text exits
  (`display()`, `to_string_lossy()`) can't be reproduced from an app path's text for a Linux name that isn't UTF-8 while
  `ESCAPES_LIVE` is false. Typing those types is the next pass; until then every further conversion only moves a
  crossing. So the plan's typed joins (`AppPath::join` with an `AppName` in `merge.rs`, `rename_merge.rs`, `landing.rs`,
  `copy_concurrent_source.rs`), the deletion of `native_join_refuses`, `walk_cached_entries` from `from_normalized`, the
  `rename_local_exclusive` switch (bulk rename's `BulkRenameRow` is public, built by the app and `cmdr-multi-rename`),
  `wire_paths`' deletion (the starts take `PathBuf` from the app until 10c/10d), the `ScanRecorder` callbacks, the typed
  `RollbackUnit` built through the escape mark's `StoredPathReading` (`InverseAct` already carries the planner's app
  paths), and the un-gating of milestone 2's `#![cfg(unix)]` files are all still 10a's.
- **Grown entries** (approved by the lead, 2026-11-21: a text exit or a native crossing, through one named helper): one,
  `transfer/volume/transfer_error.rs` 0 â 1 (`impl FailedAt for AppPath`, the failure's text exit; the second pass
  replaced it, see below). Two numbers were set by hand to their files' real counts after a run ratcheted them before
  rustfmt split a line (`conflict.rs` 6, `interrupted_landing.rs` 3, both at or below `main`'s).
- **Intended behavior changes** (each pinned, seen red first, for input no real operation produces): the batch scan
  preview keeps a result per source that differs only in a bare scheme root (`sftp:`, `sftp://`), where the second
  overwrote the first; a fresh-compress plan fails as a read error on its folder for a listed child named `a/b`, `..`,
  or empty (it packed a different subtree or re-listed the parent), and a selection spelled `/src//sub` lists its
  children as `/src/sub/d` (the same files).
- **Fixed on the way**: the two stale-temp reaps (`cleanup.rs::reap_stale_transfer_temps` and the remote zip edit's
  `reap_remote_temps`) deleted `dir.join(name)` for a listed "temp" that wasn't one plain name, so a hostile server's
  `<temp>/../../victim.txt` aimed a delete outside the folder; such a name is never reaped now (both cells red first).
  Filed: #1316 (a Rename-on-conflict or ` (recovered)` name spells a non-UTF-8 Linux name lossily).
- **Byte-exact evidence** (throwaway differential harnesses on macOS, old code against new): the scan preview's app-path
  result keys 24,048,000 cases, differing only in the bare-scheme-root class above; the compress plan's typed walk
  6,191,797 (top joins, odd shapes, deeper levels, stem and extension splits), differing only in `//` and `/./` tops and
  the non-plain-name refusals; the rollback restore's typed `parent` 224,876, differing only for text no journal row
  holds (a `//` lead with three or more names, #1309's class; relative `C:/` text; a scheme item directly under its root
  answering `sftp://h/` for `sftp://h`, which `RemoteRoot` reads as the same server root), and its root fence 12,145, 0;
  the failure seam's `failed_at` 2,000,000 generated byte paths (non-UTF-8 and escape-range text included), 0 in bytes
  and in `display()`. Everything else is the same `legacy_unchecked` value made earlier. Suites on macOS:
  `cmdr-operations` + `cmdr-operation-log` 2,789, `cmdr-multi-rename`, `cmdr-listing-wasm` 461, the app 6,807, all
  green.
- **Windows**: `pnpm check clippy-windows` and `clippy-linux` green; the CI Windows lane (compile only) green on the PR.
  No builder run today (no `ssh mami`), so the clusters this pass touches are expected green by their causes, not run.
  `transfer_benchmarks` not run (no M1 run this round); the pass adds at most one owned path per moved item.
- **Second pass done** (2026-11-21, branch `worktree-agent-a34bb84ac179fb9f8`; one agent, no helpers). A small, clean
  partial. Bridge calls in `cmdr-operations` 478 â 471 (workspace 1,358 â 1,351 after the meter fix below); no other
  crate's budget rose. What it is:
  - **The meter counts calls** (#1317): `app-path-std-bridge` counted LINES holding a bridge call, so two calls on one
    line counted one and rustfmt splitting it pushed the file over budget. It counts each call now, which raised five
    files outside this crate to their true counts (workspace 1,350 â 1,358, +8; the issue guessed about 12 from a line
    count): `volume/manager.rs` 9 â 13, `manager/archive_routing.rs` 8 â 9, `sftp_transfer_semantics_test.rs` 16 â
    17, `cmdr-fs` `volume/conformance.rs` 30 â 31, `conformance/forced_rename.rs` 4 â 5. The two tripwires stay per
    line.
  - **A failure's path is an app path**: `PathedVolumeError.path` is an `AppPathBuf`; a `std::path` value crosses in
    through `impl FailedAt for Path`, and every text exit goes through `failed_native` (`failed_path_text` for the
    error, `LogPrivate` for logs, `refused_item.rs`'s `Path ==`). Design: `transfer/volume/DETAILS.md` Â§ "Naming the
    item that failed".
  - **Cross once per value** where that lets a body: the conflict resolver and its conditional reduction, the move's
    source sweep (`kind_of`, `file_is_unchanged`, the batch deletes), a rename-merge child, and the in-flight ledger's
    root (`is_recorded_on` takes `Volume::root()` typed).
- **Grown entries** (the approved rule): `transfer/volume/transfer_error.rs` 1 â 2 (the failure's native crossing in
  and its one text exit) and `in_flight_temps.rs` 0 â 1 (the ledger's root crossing). Both go in 10f.
- **Byte-exact evidence**: the failure seam by a throwaway differential on macOS (2,000,000 generated byte paths,
  non-UTF-8 and escape-range text included: the old `PathBuf`'s `display()` and bytes against `failed_path_text` and
  `failed_native`, and an app-path failure's old and new text), 0 differ. Everything else is the same `legacy_unchecked`
  value made once, a borrow for UTF-8, so no new allocation on the copy or delete paths. Suites on macOS:
  `cmdr-operations` 2,614 and `cmdr-operation-log`, green.
- **What the pass found, for the third**:
  - The vocabulary pass 1 named (the created ledger, probes, `in_flight_temps`, `StagingTemp`) holds NATIVE paths by
    design for the local engine (`StagingTemp`, `CopyTransaction`, the in-flight records), and the volume engine's
    copies of them (`CreatedPaths`, `WrittenFile`) are fed and read by `std::path` bodies, so typing one alone only
    moves a crossing to its readers (a journal row, a log, a blocker-clearing delete). The count falls only when a
    WALKER is typed end to end: `merge_level` with `where_it_lands` and `Landing` returning app paths joined with
    `AppPath::join` and an `AppName`, and with them `RefusedChild`, `LeafPool::submit`, `copy_leaf`, `CreatedPaths`, and
    the folder-date notes. That is the typed-join item (`merge.rs`, `landing.rs`, `rename_merge.rs`,
    `copy_concurrent_source.rs`, then `native_join_refuses`' deletion); it's one serial cascade, not splittable by
    helper.
  - `walk_cached_entries` must NOT read `entry.path` through `from_normalized` yet: a pane's row is still lossy for a
    non-UTF-8 Linux name (`non_utf8_name_tests.rs` pins it), so it fixes nothing today, and a row holding a LITERAL
    escape-range code point (a real macOS name) would read as an escape and `to_native` to a different file (#1310's
    class). It flips with the lossless row (10f).
  - The `ScanRecorder` callbacks span `cmdr-fs`'s `ScanBoundary`, every backend's walk, `TreeRecorder` (its `PathTree`
    keys `std::path` sources), `NameTally` (its examples are `to_string_lossy` text), and the test `ScanRecording`
    across four crates; typing them alone nets about zero (each backend's `for_boundary` is one counted helper), so they
    go with the transfer tree's sources.
  - The typed `RollbackUnit` needs each volume's `Namer` where `cmdr-operation-log` reads the rows (no volume is in hand
    there) and a decision for stored text `from_literal_store` refuses (relative, `..`). David's call (2026-11-21): such
    a row is skipped with a new per-item reason, no migration; the namer comes as a lookup beside the volume lookup.
    Built in the fourth pass below.
- **Third pass done** (2026-11-21, branch `worktree-agent-a4e240410649dae62`, PR #1321; one agent, no helpers). A clean
  partial: the merge walk's DESTINATION side is typed, the leaf bodies past it aren't. Bridge calls in `cmdr-operations`
  471 â 462 (workspace 1,351 â 1,342), `appTextToPath` + `pathFromBareText` 610 â 608; no entry grew. What it is:
  - **The typed join**: `AppName::from_listed_text` (`cmdr-fs`) reads a row's name as app text with escapes not live (a
    literal escape-range code point spelled as the bridge spelled it inside a joined `Path`), and
    `landing.rs::ListedName` is the one way a listed name reaches a destination path, joined by `ListedFolder`.
    `where_it_lands` takes the folder as `&AppPath` and answers app paths; `landing_at` is the top-level split both
    drivers share. `DestNameIndex::lookup` takes the listed text.
  - **The walk holds its destination typed**: `merge_level` from `copy_directory_streaming` down (`merge.rs` 3 â 0),
    the folder-date notes (`FolderMetadata.dest`), the extract quarantine, and `rename_merge.rs`'s child renames. The
    bodies that still take `std::path` cross through `landing.rs`'s `on_volume`, `std_of`, and `into_std` (the join's
    own buffer when the spellings are one), so no new per-file allocation.
- **Byte-exact evidence** (throwaway differential on macOS, old code against new): 2,497,624 (folder, listed name) cases
  over POSIX, scheme, and MTP folders with non-UTF-8 bytes, NFC/NFD, literal byte- and unit-escape code points, `\`,
  `:`, `.`, `..`, `/`, and NUL: the typed join's text and its `std_of` bytes equal `legacy_unchecked(dir.join(name))`,
  and the 796,288 refusals match, 0 differ. The top-level split over 400,000 generated destinations: 0 differ for
  canonical text (167,908 cases); the rest fold `//`, `/./`, and a leading `//` to the same place, text no driver hands
  in. Everything past the join is the same value made once. Suites on macOS: `cmdr-operations` + `cmdr-operation-log`
  2,789 green.
- **What's left of the cascade**, and why `native_join_refuses` stays: the leaf bodies (`copy_leaf`,
  `LeafPool::submit`'s rows, `RefusedChild`/`RefusedLeaf`, `stream_pipe_file` and `land_link`, the staged write's
  `Spelled`, the resolver's `ResolvedConflict.write_path`, `CreatedPaths`) take `std::path`, whose Windows
  `parent`/`file_name` split a POSIX server's `a\b`. So `ListedName::new` still refuses `\` and `:` on Windows (through
  `ChildName`), and the per-item `native_join_refuses` question stays until those bodies take app paths; typing them
  alone moves crossings to their readers (the journal, the logs, the ledgers: pass two's finding), so the count falls
  there only with 10f's text exits.
- **Fourth pass done** (2026-11-21, branch `worktree-agent-a4e2fe2e5f47733a1`; one agent, no helpers): the typed
  `RollbackUnit`. `cmdr-operation-log` reads every rollback row through the escape mark's `StoredPathReading`
  (`store/rollback_unit.rs`'s `UnitReader`), and the planner and the executor act on those app paths: the planner's one
  crossing (`target.rs`' `app_path_of`) is gone. Bridge calls 1,342 â 1,340, `pathFromBareText` 608 â 607; no entry
  grew. The design and its evidence: `crates/cmdr-operation-log/DETAILS.md` Â§ "The escape mark". In short:
  - **A row the typed reading refuses** (a `..` name, a NUL) is `UnitReading::Refused`, recorded as
    `SkipReason::UnreadablePath` and never acted on. No migration, no stored row rewritten.
  - **The namer** is `VolumeLookup::namer` (default `BackendKind::namer`, new in `cmdr-fs`), asked only for literal
    escape-range text; unknown is `Failed`, never a guess.
  - **The books** (inverse rows, skip names, progress, summary, sweep depth) spell the journal's text back
    (`StoredPath`), split by `std::path` as every row was made.
- **Byte-exact evidence**: 1,000,000 generated rows through a real store, old `PathBuf` reading against the typed one:
  every canonical path (908,891) byte-exact in act path, inverse-row split, skip name, depth, in-place test, and root
  rebase under the host's namer; non-canonical text (no journal row holds it) names the same place respelled; `..` and
  NUL rows refused. Under SMB's units namer only literal escape-range text differs. Suites on macOS: `cmdr-operations` +
  `cmdr-operation-log` 2,794 and the app's 43 rollback and operation-log cells, green.
- **Fixed on the way** (a Mami run's two cells, red on Windows from birth, `815b6a16b7`): both stale-temp reaps
  (`cleanup.rs::reap_stale_transfer_temps`, the remote zip edit's `reap_remote_temps`) joined a listed temp onto a
  remote folder with `Path::join`, a `\` on Windows, so a real leftover was never reaped. They join through
  `landing.rs::ListedName` now (re-exported for `archive_edit`), keeping the one-plain-name refusal; macOS and Linux
  bytes unchanged (pass three's join evidence). `remote_tests`' cell may stay red on Windows for its `/device` POSIX
  fixture, which its siblings share. Bridge calls 1,340 â 1,339.
- **Windows, open**: a stored row with `\` in a drive path is refused (`Backslash`); whether today's Windows journal
  writes `\` (its `local_split` is `to_string_lossy` of a native `PathBuf`) wasn't checked on a builder (no Mami run).
- **Fifth pass done** (2026-11-22, branch `worktree-agent-adc8491d52ad6eb0e`, PR #1378): the journal's ends. Reproduced
  #1351 on Mami first. Two causes, both fixed: (1) `local_split` stored native `C:\Users\x`, which the dir interning
  (split by `/`) kept as ONE name under `/`, read back as `/C:\Users\x`; `journal::journal_split` now stores app text (a
  native end through `from_native_unicode`, the rest as before, and on Windows a volume path's `std::path` `\` as `/`).
  (2) The local engine journaled every row under `root`, the system drive alone on Windows, so a `C:` â `D:` move's
  undo failed per item; `OpScope::local_ids_for` names a volume that reaches each end. macOS and Linux rows keep their
  bytes (`journal_split_tests.rs`, plus a 2,000,000-path differential). Un-gated: `move_journal_tests`, the bulk-rename
  undo cell, and #1367's five undo cells; four `journal_row_shape_tests` left the known-failures list. On Mami's desktop
  (a debug build, driven over MCP): a `C:` â `D:` move of a file and a folder and its undo, a bulk rename and its
  undo, and a compress onto `D:` and its undo all restore. `OperationVolumes::find_by_root` takes the app path. Bridge
  calls in this pass 1,068 â 1,065; `transfer_benchmarks`' journaled copy unchanged (p = 0.64).
- **Left after the fifth pass**: the engine's starts and the rest of `OperationVolumes` (`resolve`,
  `routes_over_its_parent`, `path_is_inside_archive`, `path_crosses_archive_boundary`, `forget_archive_index`) still
  take `PathBuf`; typing the seam alone adds a crossing at each of its ~10 engine callers, so it goes with the starts.
  Then the backend suites, `suggested_ops/bridge/`, and the WebDAV/NFS `scratch_dir` twins (10d), and the `ListingHost`
  std-path seam that #1373 left (its oracle callers here hold `PathBuf`s).
- **Sixth pass done** (2026-11-22, branch `worktree-agent-acb193f2529f392cf`): the starts and the seam, with 10d's first
  item. One agent.
  - **The starts take app paths**: `move_files_start`, `delete_files_start`, `trash_files_start`, `start_volume_copy`,
    `start_volume_move`, `start_rename_by_move`, `start_volume_compress`, `rename_managed`, and `resolve_source_volume`.
    Each asks its route gates with the typed values, then crosses once into the bodies through `wire_paths::engine_path`
    (the bridge plus the trailing-`/` trim). A seam caller holding a value a body made (a ledger row, a create's parent,
    an anchored destination) crosses back through `wire_paths::seam_path`, the module's other named helper: 14
    production call sites (and three test cells), the crossings that turn typed as those bodies do.
  - **`OperationVolumes` is typed** (`resolve`, `routes_over_its_parent`, `path_is_inside_archive`,
    `path_crosses_archive_boundary`, `forget_archive_index`). Gone: the app's `engine_path`,
    `ResolvedVolume::passthrough`, and the registry's `engine_archive_volume_id` (the engine names an edited zip as the
    OS spells it, `archive_edit/engine.rs::archive_app_path`).
  - **The app's edge**: `wire_path::start_path` / `start_paths` / `start_destination`; the suggested-ops decorator keys
    by app path (`op_key`). `BulkRenameRow` stays `PathBuf` (the app and `cmdr-multi-rename` build it), so
    `start_renames` crosses its rows through `seam_path`; as do the scan preview and the home-folder pre-check, which
    keep `std::path` inputs.
  - **Behavior changes** (input no pane or row sends): a write start's relative or empty path refuses at the edge as the
    generic I/O failure; `//` and `/./` fold; `rename_file` refuses a `..` at the edge (a new name of `.` or `..` keeps
    `NameNotAllowed { DotName }`); the same-`root` move's destination refuses a `..` left after its resolve; on Windows
    a volume-relative POSIX source holding a literal escape-range code point refuses at the edge.
  - **Suites**: the engine's and the app's cells hand the starts typed paths (`cmdr_operations::testing::app_path_of`,
    native or app text; `app_path!` for literals). A volume-relative path (a share suite's `cmdr-test-â¦/a.txt`) has no
    app path without its volume's root, so `app_path_of` passes it on as a bridge value through `seam_path`, its bytes
    as held; cells pinning a shape no typed door makes (a trailing `/`, a `..`, a relative source) say so with a bridge
    value. The backend suites' concrete cells moved to `app_path_of`; their shared `*_test_support.rs` helpers stay on
    the bridge, as their callers hand them volume-relative paths too.
- **Left after the sixth pass**: the suites' support helpers and the WebDAV/NFS `fixture()` (each hands out the
  `scratch_dir` twin's `PathBuf` to every cell of its backend, so the twins go with typing those helpers); the
  `ListingHost` seam (its `directory_changed` callers include `cmdr-local`'s watcher, another effort's crate this
  round); `BulkRenameRow`.
- **The sixth pass's Windows cells** (2026-11-23, branch `worktree-agent-a21960add7ac0dc90`, which carries the sixth
  pass to `main`): its 25 newly failing Windows cells, each read for a cause.
  - **Three real Windows bugs**: a local write reported each finished source re-joined (`C:/x/dir\old.iso`), so the
    suggested-ops review never marked a deleted file's op done (`scan_source_tracker::top_level_source_path` now slices
    the file's own path). A rename-as-move into a zip on a POSIX volume went in unrefused, as `start_rename_by_move`
    built its targets with `Path::join` (`/repo/a.zip\top.txt`, one name the route gate couldn't place); it joins typed
    now (`ListedFolder`). And the local fast path's route guard (`write_ops.rs::reject_if_routed_over_a_parent`) SKIPPED
    a path with no native spelling, so a mangled destination (the box's native `C:\â¦\a.zip/x` anchored as one name
    under `C:/`) crossed no route check; it refuses now, fail closed.
  - **The demo** reads its write paths POSIX on every host (`invoke::demo_path`), as its listing commands did.
  - **Cells**: the suggested-ops bridge fixtures store app text (as the drive index hands the agent a path), and the
    engine's already-gone and queued-sources cells expect a source reported back as sent (`C:/â¦`).
  - **Left**: the four items above. `seam_path` still crosses at 14 production sites the meter doesn't count (it's a
    named helper over `legacy_unchecked`, counted once in `wire_paths.rs`).
- **Seventh pass done** (2026-11-23, branch `worktree-agent-a4b43c8f78683cd81`; one agent, no helpers): the walkers read
  their children typed, and every transfer's destination anchors typed. 10f's hazards 76 â 62 (the list's 11 walker
  and destination sites, `resolve_dest_path`, and three `seam_path` crossings gone, the anchor's `seam_path` fallback
  new); bridge calls 917 â 908 (`cmdr-operations` 459 â 450), `appTextToPath` 42 â 34, `pathFromBareText` 272 â
  269; no entry grew.
  - **A listed child's path is read typed**: `AppPath::from_listed_row` (`cmdr-fs`) is the walk's door for a row's own
    `FileEntry.path` (an MTP twin's path isn't its folder plus its name), the bridge's value byte for byte for canonical
    text while escapes aren't live, `from_normalized`'s once they are. Through
    `transfer/volume/listed_name.rs::listed_path`: the merge walk (its source level typed too), the rename-merge, the
    move's sweep and `stamp_source`, `remove_tree`, the volume delete's scan, the scan oracle, and `hidden_links`. A row
    holding a `..` name (a hostile `../x`) is refused as that child, never walked (an absolute row outside the folder
    still passes, #1444) (an archive's relative inner row walks as the bridge passed it): the sweep and `remove_tree`
    keep it, the delete scan and the scan stop. The bodies past the walk still take `std::path`, so they get the read's
    own buffer (`into_std`).
  - **`resolve_dest_path` anchors typed** (`anchor_at_root`, answering an `AppPathBuf`), the destination's literal
    escape-range code points spelled as the bridge spelled them; `resolve_destination`, the compress start, the
    rename-as-move, and the conflict scan anchor on the drive first and ask the route question with the typed value. A
    local volume's destination the OS spells (`C:\x\dst`, an agent's or a suite's) reads as machine-handed outside text
    (`from_external_text`) and anchors again, as the `std::path` anchor took it; a root or text no typed reading answers
    (a suite's in-memory volume rooted at a native `C:\x`) keeps the `std::path` anchor through `seam_path`, a `..` and
    another volume's path excepted. The rename-merge's case-folded sibling joins through `ListedName` (it was a raw
    `Path::join` of a listed name).
  - **Behavior changes** (input no pane or row sends): a destination holding a `..` or naming another volume refuses as
    `DestinationNotFound` (the app resolves a box's `..` first, `commands/wire_path.rs::transfer_destination`); the
    conflict scan anchors before its route question, as the start does (#1369's shape); a trailing-`/` trim keeps the
    kept text as text, so `/a//b/` stays `/a//b` (it was `/a/b`, the same place) and on Windows `/x/link/` stays
    `/x/link` (it was `/x\link`, one name no volume holds).
  - **Windows cells** (by cause): `wire_paths`' two, `failure_side_tests`' five (their fixture joined `/t0` and `nested`
    with `Path::join`), and the separator-shaped siblings: `unique_name.rs::sibling_in` joins a recovered or ` (N)` name
    with its folder's own separator (`/dest\notes (recovered).txt` on Windows was a name no remote volume holds), and
    `cmdr-listing-wasm`'s `folder_sizes.rs` joins its POSIX rows with `/`. Seen on the hosted Windows lane (run
    45228362992): 0 newly failing; the known-failures list's `cmdr-operations` entries 45 â 36 and
    `cmdr-listing-wasm`'s 7 â 5. Its first runs caught three Windows-only shapes the typed doors had refused (a native
    `C:\x\dst` destination, a Windows archive's native rows, a suite's `/`-rooted local volume); each reads as the
    `std::path` form read it now.
  - **Left in 10a** (sizes are guesses): the leaf bodies past the walk (`copy_leaf`, `LeafPool` rows, `RefusedChild` /
    `RefusedLeaf`, `stream_pipe_file`, `land_link`, the staged write's `Spelled`, `ResolvedConflict.write_path`,
    `CreatedPaths`), then `native_join_refuses`' deletion (L, one cascade); the drivers' sources and destination
    (`copy_volumes_with_progress` / `move_volumes_with_progress` and ~110 test call sites, L); the create,
    rename-validity, and conflict-preflight-sources entry points, `leaf_validation` / `target_names` / `transfer_leaves`
    (M); `scan_walker`'s cached walk (with 10f's lossless row); the remaining 11 `seam_path` crossings.

- **Eighth pass, milestone 1** (2026-11-24, branch `worktree-agent-aa4259d35808f95c3`; one agent, no helpers): the leaf
  bodies past the walk take app paths, and `native_join_refuses` is gone. Bridge calls 907 â 893 (`cmdr-operations`
  450 â 436), `appTextToPath` 34 â 33 (`journal_landed.rs:158`, off the list), `pathFromBareText` 269 â 269; no
  entry grew. What it is:
  - **Typed end to end, walk to journal**: `copy_leaf`, `copy_link_leaf`, the leaf rows (`LeafRow`, `RefusedLeaf`,
    `RefusedChild`, `RefusedFolder`), `stream_pipe_file` and `land_link` (their `FileRun` holds both spellings, the app
    one for every volume call, so `stream_pipe*.rs`, `server_side_copy.rs`, and `move_file.rs` hold no bridge call),
    `interrupted_landing::settle_unanswered_create`, the staged write (`StagedWrite::begin` takes an
    `AnnouncedOnVolume`; `staged_paths.rs::Temp::mint` names the temp by the app path's split and join, registered under
    its own name through the new `StagingTemp::adopt_named`), `CreatedPaths` and the drivers' op-wide ledgers
    (`WrittenFile<AppPathBuf>`, `Vec<AppPathBuf>`), the volume rollback (`cleanup.rs`), the move sweep's `SourceLedger`
    and skipped set (keyed by app path: a Windows `PathBuf` key compares `a\b` equal to `a/b`), `LandedReadBack`, and
    the journal's volume-transfer and created-dir rows (`journal_split.rs::journal_split_app`, split out of
    `journal.rs`).
  - **`native_join_refuses` is deleted** (with `only_native_join_refuses`, `DestinationNames::join_as`, and their
    cells): `ListedName::new` refuses only what's no plain name on any host, so a POSIX server's `a\b` or `12:00 notes`
    lands whole on a remote destination, and a local NTFS one refuses it by its rules, per item. The cells:
    `refused_class_tests.rs::run_remote_names_case` (both ways, all three drivers, `a\b.txt` beside a real `a/b.txt`).
    â Its `\` names and `listed_row_tests.rs`' backslash cell still run on macOS and Linux only, for the TEST DOUBLE:
    `InMemoryVolume` keys its store by `PathBuf` (`in_memory.rs`, on the 10f list), so on Windows it lists `a\b.txt` as
    `b.txt` in `a` and can't hold `foo\..` or both `a\b.txt` and `a/b.txt` (the lane's first run, 45467575511, read
    exactly that). Typing its store is what re-enables them; the engine's side is pinned on every host by the typed temp
    cell (`staged_write_tests.rs`) and the journal's split under both flavors (`journal_split_tests.rs`).
  - **Windows lane** (hosted, run 45480062626): 0 newly failing; 13 known failures pass and left the list (eight
    `swap_outage_tests`, five `archive_edit::remote::remote_outage_tests`, which named their temp through `std::path` on
    a POSIX fixture), with the three deleted native-join cells: `cmdr-operations` entries 36 â 20.
  - **A name a drive folder can't hold is asked about per item** (review of #1451): the typed join refuses `\` under a
    drive or UNC folder, and that refusal used to come back as the LEVEL's failure (a POSIX server's `a\b.txt` into
    `C:\dest` failed the walk, or on a Skip dropped the whole folder); `merge_level` now refuses that child as a
    `RefusedChild` before `where_it_lands`. Pinned on every host by `refused_class_tests.rs`'
    `a_backslash_name_a_drive_folder_cant_hold_is_asked_about_per_item` (a drive-spelled in-memory destination, seen red
    first).
  - **A journal rebase between canonical roots only**: a volume-relative root (a share suite's `cmdr-test-â¦/tree`, a
    local tree's `tree`) rebases its leaves by `std::path` as before; the first run's typed-only rebase wrote such a
    leaf's DEST as its source, and the SMB and S3 fixtures' undo cells caught it (`UnverifiablePrecondition`).
  - **Byte-exact on macOS and Linux** by construction, no differential harness this pass: a typed temp is minted only
    for a canonical final path holding no escape-range code point, whose split and join there equal `Path`'s (anything
    else mints through `std::path` as before, so a non-UTF-8 Linux name keeps its lossy temp); a typed journal end
    splits through `journal_split(engine_path(...))` off Windows (`journal_split_tests.rs` pins both flavors); every
    other volume call gets the app path the bridge made before, made once. Suites on macOS: `cmdr-operations` +
    `cmdr-fs` 3,880 green.
  - **New crossings, to go with the drivers' pass**: `journal.rs::record_volume_transfer_source` reads its two driver
    roots through `seam_path` (2), and the top-level `copy_single_path`, `move_file_across_volumes`, the copy drivers'
    file-source ledger push, `link_carry`'s probe, and the two archive-edit announces cross the driver's `std::path`
    through `listed_name::on_volume` (each replaced a bridge call further in).
- **Left in 10a after the eighth pass**: the drivers' sources and destination (a TOP-LEVEL name still splits through the
  driver's `std::path`, `landing.rs::landing_at` with `ListedName::native`'s host refusal, so on Windows a top-level
  remote source named `a\b.txt` lands as `b.txt`, silently: #1457, which that pass closes); the resolver's
  `ResolvedConflict.write_path` and `naming.rs` (a `Rename` pick on a `\`-holding name splits on Windows, crossed back
  through `on_volume`); `DisplacedDestination`'s aside mint (`std::path`); then milestone 2's list (the create,
  rename-validity, and preflight-sources entry points, `leaf_validation`, `target_names`, `transfer_leaves`).

- **Ninth pass, milestone 1** (2026-11-25, branch `worktree-agent-af05b2518a5689a0f`, PR #1465; one agent, no helpers):
  the drivers' sources and destination are app paths end to end, which fixes #1457. Bridge calls 893 â 880
  (`cmdr-operations` 436 â 423), `appTextToPath` 33 â 33, `pathFromBareText` 268 â 268; no entry grew. â The
  meter counts direct bridge calls only, so it reads lower than the crossings: in `cmdr-operations`' production code the
  lines calling a named wrapper went `std_of(` 31 â 73, `on_volume(` 34 â 39, `seam_path(` 18 â 17, `engine_path(`
  14 â 16, `into_std(` 8 â 5 (mostly a typed driver handing a value to a seam still keyed by `std::path`: the tree,
  the probe, the sink, the resolver). Counting through wrappers is #1469. What it is:
  - **Typed from the start to the bodies**: `copy_between_volumes` / `move_between_volumes` take the routed start's
    `AppPathBuf`s and cross once (`wire_paths::engine_path`) only for the native shortcut, the same-volume engine, and
    the summary; the source binding keeps typed sources by position (`retain_bound_app_sources_on`).
    `copy_volumes_with_progress` / `move_volumes_with_progress`, the async driver (its `TransferContext` generic over
    the path spelling, `NameAtDest`, `ConflictDecision`, `build_pre_skip_set`), `landing_at` (`ListedName::app`),
    `copy_single_path`, `copy_directory_streaming`, the sequential extract, the preflight's hints, the concurrent task,
    `HeldSource`, `LandedReadBack`, and `journal::record_volume_transfer_source` all take app paths.
  - **One top-level split**: `transfer/volume/listed_name.rs::top_level` (the chosen name, else `own_name`: the app
    path's last name, and at a ROOT the name `std::path` read there, so `sftp://u@h:22/` still lands in `/dst/u@h:22`
    and on macOS and Linux `C:/` in `/dst/C:`, byte for byte `main`'s; a review of #1465 caught the first draft merging
    a scheme root into the destination) feeds the serial loop, the concurrent `prepare_source`, the copy's pre-skip
    identity check, the pre-known-conflict match, and the progress labels. A name the destination folder can't hold
    (`TopLevel::Refused`: a drive folder and a `\`, or a chosen name that isn't text) is asked about per item
    (`TransferContext::dest_name_refused`, `CopyTask::dest_name_refused`), where it failed the transfer.
  - **The journal's ends**: the two `seam_path` crossings `record_volume_transfer_source` read the roots through are
    gone; a file source's row splits through `journal_split_app`, which now takes the typed split only for a CANONICAL
    path (a Windows suite's native-spelled bridge root keeps its `journal_split` row). Pinned for every root shape (`/`,
    a drive, a scheme root, a volume-relative root, a top-level `a\b.txt`, a source that is its volume's root, a scheme,
    drive, or UNC root as the source, `/` or a scheme root as the destination, a selected link), file AND directory
    rows, source AND destination, copy and move: `journal_root_shape_tests.rs`.
  - **Byte-exact on macOS and Linux** by construction: a typed path's split and join equal `Path`'s there for every
    canonical path; progress labels and the name-rules check read the app name as the native text it spells (`label_of`,
    `native_text`), so a non-UTF-8 Linux name reads and checks as it did; the dest-inside-source guard still compares by
    `Path`. Suites on macOS: `cmdr-operations` + `cmdr-fs` 3,885 green.
  - **Crossings that moved, not new**: the resolver still answers a `std::path` write path (`ResolvedConflict`), read
    back through `on_volume` in `top_level_conflict.rs`, `copy_concurrent_source.rs`, and `move_same.rs` (it used to
    cross inside `copy_single_path`); the same-volume engine and the archive move-out cross their `std::path` sources
    once into the typed driver (`on_volume`, replacing the closures' own bridge calls).
- **Tenth pass** (2026-11-25, branch `app-path-pass-10`, PR #1486; two leads in turn, plus helpers `inmem-typed` on the
  in-memory store and three on the Windows joins below, each in its own worktree, cherry-picked): the test double, the
  resolver, the aside, and a new name's join take app paths. Bridge calls 880 â 816 (`cmdr-operations` 423 â 391,
  `cmdr-fs` 205 â 173), `appTextToPath` 33 â 33, `pathFromBareText` 268 â 261; no entry grew. Wrapper lines in
  `cmdr-operations` production code (uncounted, #1469; counted as non-comment lines holding the call text, in files
  outside `*_test(s).rs`, `tests/`, `testing/`, and `test_support`, definitions included, so the base reads higher than
  the ninth pass's figures): `std_of(` 74 â 78, `on_volume(` 46 â 44, `seam_path(` 19 â 20, `engine_path(` 17 â
  18, `into_std(` 6 â 6. Hazards 62 â 62 (nothing on the list; the resolver's read-backs were wrapper crossings, and
  the joins below were `Path::join` on a body's folder, which the list's regex doesn't see). What it is:
  - **`InMemoryVolume` keys its store by `AppPathBuf`** (`cmdr-fs` `in_memory/keys.rs`, `scan.rs`), its arithmetic the
    app path's, so on Windows `/a\b.txt` is one file in `/` beside `/a/b.txt` and `foo\..` a name; listing text,
    `FileEntry.path`, and error text stay byte for byte (lossy decode of an escape, as `display()`); a drive path keys
    as `/C:/x` off Windows, as `Path` read it. Re-enabled on Windows: `refused_class_tests.rs`' `\` names,
    `listed_row_tests.rs::a_backslash_name_copies_and_moves_as_one_name`, and `top_level_name_tests.rs`' `\` cells
    (#1457's Windows proof).
  - **The resolver takes and answers app paths** (`ResolvedConflict::write_path`), and the volume namer walks
    `unique_name::AppNameCandidates`, so a Rename of `a\b.txt` picks `a\b (1).txt` beside it (#1470); the op's
    `ClaimedNames` ledger keys by the `std::path` spelling (a reservation only lands on a local disk).
  - **`DisplacedDestination` mints its aside typed** (`staged_paths.rs::typed_aside`), beside the original; a bridge
    value or an escape-range name keeps the `std::path` mint.
  - **â The typed double exposed `Path::join` on a remote folder.** Once the store keyed by app path, 61 Windows cells
    failed: on Windows `Path::new("/dest").join(name)` is `/dest\name`, ONE name in `/` on the double as on every POSIX
    server (where the `PathBuf` store had split it). Three kinds: the double's own `&Path` test helpers, which now read
    `\` as the `std::path` separator it is (`in_memory.rs::app_of`; a `\` name reaches the store only through a typed
    `Volume` call); cells joining with `std::path` before a `Volume` call, now typed or formatted; and production joins,
    real Windows bugs on SFTP, MTP, and the like. A NEW name joins typed through `wire_paths::seam_child`
    (`ListedFolder`; a refusal stays that item's answer): `create.rs::new_entry_path`, `look_alike::spelled_new_path`,
    `rename::rename_target`, and both ends of `rename/validity.rs`. A name already plain joins by text through
    `unique_name::sibling_in`: the compress landing's look-alike, the bulk rename's hop temp, the respell compare.
- **Left in 10a after the tenth pass**: the create, rename-validity, and preflight-sources entry points,
  `leaf_validation`, `target_names`, `transfer_leaves`; the same-volume engine's `std::path` body (`move_same.rs`,
  `rename_merge.rs`, `top_level_move_hints`; `move_same.rs`' `dest_path.join(name)` sameness compare misreads a `\` leaf
  on Windows, so a remote move to a chosen `a\b.txt` skips as "already in place"); `create.rs::refuse_routed_create`'s
  `Path::new(parent).join(name)` before validation; any other `Path::join` onto a body's folder (no check sees one; a
  sweep of production `.join(` would); those three are #1491, each a refusal or a misreport only; the op's
  `ClaimedNames` ledger and the staged writes' placeholder release (`std::path` keys); `scan_walker`'s cached walk (with
  10f's lossless row); #1316 (a Rename or ` (recovered)` name of a non-UTF-8 Linux name stays lossy, byte for byte as
  before).
- **Eleventh pass, milestone 1** (2026-11-25, branch `worktree-agent-a87892783e590c297`; helpers `join-check` (the
  check) and `split-sweep` (the sweep below), each in its own worktree, cherry-picked): #1491 fixed, the sweep's joins
  typed, and a check that keeps them so. Bridge calls 816 → 813 (`cmdr-operations` 391 → 388), `appTextToPath` 33 → 32,
  `pathFromBareText` 261 → 255; wrapper calls (new budget, #1469) 173 → 188, so every crossing 989 → 1,001 (the sweep's
  typed splits hand their answers to `std::path` bodies through a wrapper: more crossings, each now named and counted);
  `std::path` joins in the write engine (new budget) 80 → 71. Hazards 62 → 56. No pre-existing entry grew. What it is:
  - **The sweep**: every production `.join(` / `with_file_name(` and `file_name()` / `parent()` split in
    `cmdr-operations` and the app's `operations_host/`, read one by one (about 300 sites; about 150 native: the local
    engine, tempdirs, the in-flight ledger, fixtures, thread joins). 14 real Windows bugs, each fixed with a cell that
    runs on every host (red only on Windows, as each split equals `Path`'s on macOS and Linux), all where `std::path`
    read a POSIX volume's `a\b.txt` as `b.txt` in `a`:
    - a same-volume move to a chosen `a\b.txt` skipped as already in place, reported Done (`item_identity`, #1491; the
      copy's pre-skip identity too);
    - a rename-as-move to an object store refused a target name holding `\` (`TargetNames`);
    - a named transfer onto a server: false `SameName` / `OntoASource`, a missed `AlreadyInDestination`
      (`leaf_validation`);
    - the conflict preflight's pane stats and self-collision filter took another entry's answers;
    - `refuse_routed_create` joined before reading the name as one (#1491);
    - a remote zip's duplicate pre-check joined `/share/a.zip\dir/x.txt`, which no route serves
      (`archive_inner_exists`);
    - a rename, new file, or new folder named `a\b.txt` was respelled into folder `a` or failed (`look_alike.rs`'s
      splits undid the tenth pass's typed join; `folder_and_name`);
    - a bulk rename refused `x.txt` → `sub\x.txt` as changing folders, moved rows into `sub`, put its hop temp there,
      and let a share that refuses `\` take one (`rename/bulk.rs`, `by_move.rs`, `refused.rs`);
    - a same-volume move listed `/src/a` for `a\b.txt`'s hint and failed or took another entry's kind
      (`preflight.rs::top_level_move_hints`);
    - a failed landing's ` (recovered)` rescue went into folder `a` or stayed on a temp name (`recovered_name.rs`);
    - a long `a\b.txt` the server refused read "not allowed" for "name too long" (`transfer_error.rs`);
    - a compress to `sub\a.zip` replaced a look-alike in `sub` (`archive_edit/compress.rs`), and the remote edit's temp
      reap swept `sub` (`archive_edit/remote.rs`);
    - a transfer or delete preview took `sub/a.jpg`'s size and kind for `sub\a.jpg` (`scan_preview/volume.rs`).
  - **Typed now**: `is_the_same_item` / `is_the_same_volume_path` (`&AppPath`), `TargetNames` (`AppPathBuf` keys, names
    checked by `AppName::from_listed_text`), `TransferLeaves::chosen_name` (the volume engine's leaf as app text;
    `chosen_for` stays for the local engines), `listed_name::top_level` (`Option<&str>`), the preflight-sources entry
    point (`scan_volume_for_conflicts_within` takes `Option<Vec<AppPathBuf>>`, read at the app's and the demo's edge; a
    refused set falls back to the name-only check), and `leaf_validation` on a name-only route (it reads the path's own
    text as its canonical `AppPath`, no crossing).
  - **The check** (`scripts/check/checks/desktop-rust-app-path-std-join.go`, inside `app-path-std-bridge`): two new
    per-file ratchets. `wrapperCalls` counts each call through `std_of`, `into_std`, `on_volume`, `seam_path`, and
    `engine_path` in `cmdr-operations` and `cmdr-fs` (#1469), so the meter reads every crossing (1,001: 813 direct + 188
    through a wrapper; seeded at this branch's true count). `stdJoin` counts a production `Path::join` /
    `with_file_name` in `cmdr-operations` and `operations_host/` (a typed join is told apart by the `Result` it
    returns); a native one opts out with `// allowed-std-join: <reason>`.
  - **Behavior changes**: a rename-as-move's target name holding a NUL refuses at the start (no volume stores one); a
    preflight whose sources the edge refuses runs name-only; on Windows, the preflight's local self-collision asks the
    OS only for typed ends with a native spelling (else the conflict stays).
  - **Byte-exact on macOS and Linux** by construction: the typed splits and joins equal `Path`'s for canonical UTF-8
    text, and every typed reading falls back to the `std::path` one (the lenient reader splits by the host's rules) for
    text that isn't UTF-8, holds an escape-range code point, or isn't canonical, so a native Windows `C:\x\a.txt` still
    splits at `\`. Suites on macOS: `cmdr-operations` 2,699, `cmdr-listing-wasm` 146, green.
  - **Found, not fixed** (each a refusal, a label, or a design call): `in_flight_sweep.rs::keep_beside` names a volume's
    rescue with the `std::path` `recovered_sibling` (the `recovered_name.rs` bug, on the startup sweep); a remote
    `x\a.txt` copied into a zip lands as entry `a.txt` (`archive_edit/copy_into.rs:282`, `fresh_plan.rs:333`: how a zip
    names a `\` entry is David's call); `validation::first_shared_name` refuses remote `x\a.txt` and `y\a.txt` as
    sharing a name, and `name_rules::ends_in_dot_name` reads `foo\..` as `..` (refusals); progress labels and summary
    names in `rename/bulk.rs`, `delete/volume_walker.rs`, `delete/already_gone.rs`, `summary.rs`, `types/leftovers.rs`,
    `left_in_source.rs`; `transfer_tree/model.rs`' `components()` attribution; `rollback.rs:275` and
    `fresh_compress.rs:252` refresh a pane by the `std::path` parent.
- **Left in 10a after the eleventh pass**: the create and rename-validity entry points (`create_*_managed` and
  `check_rename_validity_impl` still take the parent as text, so `create.rs` holds 8 tripwire sites and `validity.rs`
  2); the same-volume engine's `std::path` body (`move_same.rs`, `rename_merge.rs`); `ClaimedNames` and the placeholder
  release (`std::path` keys: on Windows `a\b (1).txt` and `a/b (1).txt` hash as one key, so a claim can shadow the
  other; a leftover placeholder at worst); `scan_walker`'s cached walk (10f's lossless row); #1316.

### 10b. `cmdr-index`

- **Size**: M.
- **Scope**: the local scanner, reconciler, and watcher ingest native paths through `from_native_unicode` at the walk
  and event edges (a `NotUnicode` child keeps today's lossy row, counted; 10f swaps in `from_native`, which escapes) and
  hold app paths after; `store/paths.rs` resolves `&AppPath` by names; the exclusion policy's `/`-prefixed rules become
  app-path rules; media and importance stores keep their `TEXT` columns.
- **Landmines**: the drive index's key decision stands (lowercase, Unicode form untouched). The cold-drive fixture roots
  a tempdir; on Windows that's a drive app path, so the index root is that path, not `/`. Some of that may need task
  34's drive volumes: if so, say which tests and stop there. The walker's `RawDirEntry.path` is raw today and the index
  stats every child through it; at the edge, `from_native` escapes a non-UTF-8 name, so the row's `name` changes from
  the lossy spelling to the escaped one (a cache: the reconciler sees one remove and one add, then converges; pin that
  it converges and doesn't churn every pass, which a lossy name compared against a raw disk name would). Same for media
  and importance `path` columns (Â§ "Persisted formats").
- **Test plan**: Windows, #1232's clusters as review round 4 counted them from the builder's log (87 failures and 47
  timeouts, all fixtures whose tree is a native tempdir in the CWD, `D:\src\cmdr\crates\cmdr-index\cmdr_test_â¦`, read
  against the index's `/`-rooted path space, so rows land as `/D:\src\â¦` or not at all): `reconcile::*` (41),
  `lifecycle::phases::tests` (19), `lifecycle::cover` (8 plus the 46 cold-drive timeouts, as far as task 34 allows),
  `scanner::convergence_tests` (6), `scanner::policy_tests` (4), `watch::branches` (4) and
  `watch::event_loop::tests::replay_chunks` (3, with task 12's smoke test), `partial_agg` (1), the concurrency stress
  cell (1), and one reconciler timeout. The fixtures switch to milestone 3's `tempdir_in` helper. Not here: 20 that are
  #1226's family (`importance::store::legacy_split` is 17, not 3, plus `handle::tests::building_splits_â¦` and two
  database deletes while a connection holds the file); `store::tests::path_resolution` (3), which aren't paths at all
  (the `*_off_macos` cells expect Linux's binary compare, and Windows now folds case: a cfg fix, #1232); and the
  cold-drive cell that renames a mount point with handles open (Windows refuses the rename). macOS/Linux: suite plus
  `index_benchmarks` before/after.
- **DONE**: bridge count zero in `cmdr-index` and `index-query`; #1232 updated with what's left.
- **Done, first step** (2026-11-21, branch `worktree-agent-af451005373bb3ef6`; the lead on the roots and the network
  scanner, one helper on MTP's resolved object, the test doubles, and the media fetch): the index takes each volume's
  root, and walks a share or a phone, by its typed app path. Bridge calls in `cmdr-index` 29 â 0 (`index-query` had
  none), `appTextToPath` 6 â 3, `pathFromBareText` 48 â 42; workspace bridges 1,882 â 1,853. No budget rose. The
  durable design: `crates/cmdr-index/src/indexing/paths/DETAILS.md` Â§ "App paths at the edges". What converted:
  - **Roots**: `StartRequest` and `IndexManager` hold the `Volume`'s root (`AppPathBuf`), crossing once each way in
    `paths/app_text.rs`: `stored_text` for every stored or compared root text, `native_root` for the local pipeline
    (`None` for `mtp://` and `adb://`). The boot root parses `mount_layout::boot_volume_root()` (`C:/` on Windows), and
    the cover bootstrap's `root` now roots there too instead of a bare `/`.
  - **The network scanner** (fresh, reconcile, cover): its queue and `ScanContext` are app paths; a row's path is its
    own canonical text (`app_path_of`, no extra copy), a reconcile child the typed join (`child_of`); a folder path is
    built only for a folder row. `resolve_scan_root` takes index text; the cover chain joins `/` text (`push_name`).
  - **MTP's resolved object** (`ResolvedMtpObject.path`, in `cmdr-mtp` and the index's host seam) is an `AppPathBuf`,
    and the MTP watch does typed `parent`/`file_name`; the media fetch builds its path through `from_external_text`.
- **Byte-exact on macOS** (throwaway differential harnesses, old bridge answers against the new, generated input; in the
  lead's scratchpad): root text 2,000,000, 0 differ; native root 2,000,000, byte-identical for every canonical root,
  refusing only bridge-only shapes (a trailing `/`, `//`, `.`, `..`) and `//`-led three-name text (#1309's UNC reading),
  and `None` for scheme roots; row paths 1,621,506, the same text for every canonical row except a name holding a
  literal escape-range code point (#1310's class: the bridge re-escaped it); reconcile children 886,428, every
  difference holding an escape-range code point, and a name that isn't one plain name now `None`; the bootstrap join
  1,536,466, 0 differ. MTP watch 3,008,374 (parent text, row name, `DirsUpdated` URL), 0 outside the literal-escape
  class (1,874,102 of them: the bridge wrote U+FFFD, the typed path keeps the row's text); media fetch 1,034,928 joined
  texts, 0 differ (refused now: empty, relative, `..`, NUL text, a per-file skip; `//`, `/./`, a trailing `/` fold). The
  suites: `cmdr-index` 2,013 and `cmdr-mtp` 206, green.
- **Intended behavior changes** (input no backend lists): a listed folder whose name isn't one plain name, or whose row
  text names no place, is indexed as a row and not descended (the reconcile walk asked the volume to list `dir/..`); a
  phone's start no longer asks `statvfs` about `mtp://â¦` (it answered `None` with a warning).
- **The review round's fixes** (2026-11-21): the native root crosses through its canonical twin, so a bridge-only root
  shape (a trailing `/`, `//`, `.`) opens where the OS opens it instead of sending a local drive to the SMB gate (only a
  `..` refuses); `app_path_of` folds separators but never respells escapes (non-canonical escape text was read as `xÃ©`,
  a look-alike whose children a walk filed under the literal row; now `None`, not descended); and a cover root resolves
  its row by its own text, not `stored_text`, which read a literal escape-range code point as U+FFFD.
- **Left for the next step** (the Windows clusters, #1232): the LOCAL pipeline (the guarded walker, the reconciler, the
  watcher, the phase machine) stays native inside, and its native â stored-text edge is about 100 `to_string_lossy`
  sites that spell `D:\x` on Windows where the index wants `D:/x`; that edge (`from_native_unicode`, a `NotUnicode`
  child keeping today's lossy row, counted) and the fixtures' switch to `tempdir_in` are what turn the 133 cells green.
  Also: `CoveredEntry.path` (a `pub` `PathBuf` `cmdr-search` reads) still carries a share row's text as a `std::path`,
  one `appTextToPath` site in `cover_scan.rs`; the SMB watcher's `&Path` seam (`apply_smb_change`) waits on the app's
  listing host (10c), and the index reads its text through `app_path_of` meanwhile.
- **Windows**: `pnpm check clippy-windows` green on the merged branch; no builder run (no Mami today). Expected green:
  nothing yet among the 133 local-pipeline cells, which wait on the edge above.
- **Done, second step** (2026-11-21, PR #1318, branch `worktree-agent-a9d98875cd8e03dac`): the local pipeline's way up
  from the OS. Durable design: `crates/cmdr-index/src/indexing/paths/DETAILS.md` Â§ "App paths at the edges".
  - **The edge**: `paths/app_text.rs`'s `native_text` (a path) and `native_name` (a walked name) replace about 100
    `to_string_lossy` sites in `scanner/`, `reconcile/`, `watch/`, `lifecycle/`, `read/expected_totals.rs`, and their
    tests; through `from_native_unicode`, a name the app path can't hold yet keeps its lossy row and is counted
    (`names_kept_lossy`), never dropped. On Windows a plain or verbatim drive path takes a separator swap
    (`plain_drive_text`) instead of the full crossing.
  - **Rebuilt text**: `path_prefix::rooted` (with `reconstruct_path`, `capped_prefix`, `deepest_common_ancestor`),
    `compute_parent_path`, `self_and_ancestors`, `join_volume_relative`, and the cover bootstrap root a drive's text at
    `D:/` and keep a UNC `//` lead, gated on `PathFlavor::HOST`.
  - **The root's native form**: `native_root` is plain first (`to_native_plain`), verbatim only past what plain can
    spell; a verbatim root matched no host mount answer.
  - **Fixtures**: the phase machine's and cold drive's volumes root at the tree's app spelling; POSIX-literal fixtures
    (convergence, `partial_agg`, the removable stops, `local_external`) became host-absolute; the three
    `store::tests::path_resolution` `*_off_macos` cells are Linux's, with a Windows twin.
- **Byte-exact on macOS** (throwaway harnesses in the agent's scratchpad, against `to_string_lossy`): 2,000,000
  generated paths (NFC and NFD, `\`, `:`, invalid bytes, literal escape-range code points, `//`, a trailing `/`) and
  1,713,933 names, 0 differ; 3,011,023 real paths under a home folder and Dropbox (539 non-ASCII, 329 NFD), 0 differ,
  every one borrowed. Every `rooted` / drive / UNC change is behind `PathFlavor::HOST == Windows`. The separator swap
  against `from_native_windows`: 1,865,802 generated Windows texts, the 319,536 it answers all equal. The suite:
  `cmdr-index` 2,021, green.
- **Allocation**: on macOS and Linux none added; the hot walk loop's path text is borrowed exactly as `to_string_lossy`
  borrowed it, and its name is the one `String` it always was. On Windows a `\`-spelled child pays one `String` for its
  respelling (the walker joins with `\`): about 127 ns a path against 1,046 ns for the full crossing (macOS proxy over
  1,000,000 real paths spelled `D:\â¦`).
- **Windows** (the builder, `cmdr-index`, 1,932 tests): `main` 192 red â 31, none newly red; the 31 are #1226 (19), a
  rename of a folder with open handles (5), no `nlink` from `std` (4, task 35), and POSIX fixtures on the root volume
  (3, task 34). `docs/notes/windows-port/windows-test-failures.md` Â§ "By milestone" and the TSV's `rerun` column.
- **Left for 10c**: `CoveredEntry.path` and the SMB watcher's `&Path` seam, as the first step said; `#1310` unchanged
  (no escape spelled anywhere new).

### 10e. `cmdr-multi-rename` and `cmdr-listing-wasm`

- **Size**: S. Moved out of 8 in review round 2: both depend on `cmdr-operations` (the wasm crate also on `cmdr-viewer`,
  `cmdr-multi-rename`, and `cmdr-operation-log`), so they convert after 10a.
- **Scope**: as 8: bodies drop their bridges; `cmdr-multi-rename`'s `names_file.rs` crosses at `to_native` for the names
  file it owns. `cmdr-listing-wasm` builds for `wasm32` (no `to_native` there at all: it has no OS).
- **Test plan**: Windows: `cmdr-listing-wasm`'s 6 failures and 1 timeout (`cmdr-multi-rename` had none on the builder;
  review round 4). macOS/Linux: both suites, plus the wasm build.
- **DONE**: bridge counts zero in both.
- **Done** (2026-11-21, branch `worktree-agent-abb6a765db0075d73`, PR #1323; one agent, no helpers). Bridge calls in
  `cmdr-listing-wasm` 15 â 1 (workspace 1,342 â 1,328), its `appTextToPath` 10 â 8 and `pathFromBareText` 14 â
  10; `cmdr-multi-rename` held none. No entry grew anywhere. Durable design: `crates/cmdr-listing-wasm/DETAILS.md` Â§
  "App paths". What converted:
  - **`cmdr-listing-wasm`**: a path arg or a `FileEntry.path` becomes an app path through `invoke.rs`'s `demo_path`
    (POSIX on every host: the demo's volume is, and Windows' `from_wire` reads only drives), and every volume call takes
    it typed: the path questions, About, the archive notice, the sweep, the claim, the staged drop (joined by the app
    path's join, where `Path::join` put a `\` on Windows), and the box's write access (`anchor_at_root`). It has no OS,
    so nothing crosses at `to_native`; the in-memory store's own `std::path` helpers take the app text (`on_store`).
  - **`cmdr-fs`**, for the demo: the typed archive split (`archive_boundary_candidate_at`, moved from `cmdr-archive`'s
    `pub(crate)` beside the `std::path` split, so `cmdr-archive`'s surface cap is untouched), `root_echo_at`, and
    `InMemoryVolume::canonical_app_path` (go-to's link walk, read back typed).
  - **`cmdr-multi-rename`**: nothing to convert. Its folder `dir` is a `std::path` value feeding only `std::path` seams
    (`BulkRenameRow`, the write engine's `SourceFingerprint::capture_*`, `rename_place`), and `names_file.rs` writes to
    the host's native `results_path`, so it holds no app path to cross. Typing `dir` alone would add a crossing per row.
- **What still bridges, and who takes it**: `operations_host/volumes.rs`' `find_by_root` (1): `OperationVolumes`'s seam
  takes a `&Path` its one caller (`fresh_compress.rs`) makes with `as_std_legacy`, so typing the seam (10a) drops both.
  `BulkRenameRow` staying native forces no bridge call (see above); multi-rename's `dir` types with it in 10c/10d. The
  crate's other `std::path` arithmetic (`folder_sizes.rs`, `search.rs`, `backend_listings.rs`) runs over values the
  engines' host seams hand in (`ListingIndex`, `OperationIndex`, `GoToHost`, `open_listing_paths`, `directory_changed`),
  and types with them (10a, 10c).
- **Intended behavior changes** (input no pane spells): a path question about relative text or a path holding a `..`
  name finds nothing (`path_exists`, `stat_paths_kinds`, About, the archive notice), where the bridge rooted it at `/`
  or climbed by text; the box's write access for such text is `unknown`; the archive notice names a sloppily spelled
  archive canonically (`/a.zip` for `//a.zip`, the same file). Pinned:
  `the_path_questions_find_nothing_at_text_that_names_no_app_path` (red on `main`).
- **Byte-exact evidence** (a throwaway differential on macOS, old bridge code against new over an in-memory fixture with
  `.zip`, `.tar.gz`, `\`, `:`, NFC and NFD names): 185,644 generated texts (one to four names from 13, each with a `/`,
  `//`, or no lead and an optional trailing `/`), ten answers each (exists, stat, extension, archive notice, write
  access, root echo, claim, the drop's relative check and staged text, go-to's found kind) plus the sweep's held rows: 0
  differ for canonical text; every difference is a `..` name (58,872), relative or empty text (12,657), or the archive
  notice's spelling of non-canonical text (12,262). Suites on macOS: `cmdr-listing-wasm` 144, `cmdr-multi-rename` 143,
  the moved split and the typed echo in `cmdr-fs`, `cmdr-archive`'s boundary tests, all green.
- **Windows**: `pnpm check clippy-windows` green; no builder run (no Mami today). Expected green by cause:
  `imports_test::a_dropped_folder_lands_staged_with_its_structure_and_bytes` (the staged join). The `folder_sizes_test`
  (2 and the timeout) and `search_test` (2) cells stay red until their seams type (above); the viewer cell's cause is
  unread.

### 10c. The app, part one: file system and listings

- **Size**: M.
- **Scope**: `apps/desktop/src-tauri/src/` `file_system/`, `listing_host/`, `volumes/`, `commands/`, `go_to_path/`,
  `favorites/`, `recents/`, `file_viewer/`, `native_drag/`, `about/`, `network/`. IPC arguments become app paths at the
  command edge through `from_wire`; OS integrations cross at `to_native`.
- **Landmines**: the resolver's un-hinted fallback (`resolve_path_to_volume` walks the mount table to `/`) is what D1
  and `remote_paths.rs` defend against; keep it answering exactly as today on macOS/Linux. Two-layer timeouts on path
  resolution stay. `root_volume`'s Windows answer is task 34's, not this plan's. `from_wire` at the command edge is loud
  rule 1's second exception: a trailing `/` is dropped and a `..` name is refused (Â§ "Normalization"), so a command
  that took `/x/link/` or `/x/link/../y` answers differently. Land that as its own commit after the mechanical
  conversion, with milestone 1's pins (g) flipped deliberately and the lead's OK; until then a command that needs the
  old answer keeps `legacy_unchecked`. In a second commit of the same exception: `resolve_typed` (Â§ "Typed and pasted
  `..`") in `go_to_path/`, used by go-to-path, the CLI's path arguments, and deep links; the
  `Volume::parent_for_dot_dot` trait method with its lexical default and its two overrides (`cmdr-local`'s Unix volume,
  `cmdr-sftp`; both crates are done by then, so this is a small, owned edit there); go-to-path's `CLAUDE.md` "never
  `canonicalize`" rule rewritten to the new one. The UNC root fold (`same_root`) lands with the resolver's volume pick.
- **Test plan**: Windows: the app failures in these directories (about 72 of the 188 in `cmdr`, review round 4: 11
  native-tempdir panics in `file_system/watcher*` and `network/smb_mount_gate`, the rest `std::path` on app paths, most
  in `network/`, `go_to_path/`, `about/`, and `commands/`; `about/`'s 13 carry no path in the panic and are the least
  certain; milestone 1's baseline lists them), and the `#![cfg(unix)]` files here gated only for #1225 un-gate. Not
  here: the root volume's lookups (`resolve_location`, `a_home_relative_path_resolves_to_root`), task 34's. macOS/Linux:
  suite and `desktop-e2e-linux` (it touches listings); pins (g) and (h) per entry point, an IPC delete of `/x/link/`
  removes the link and leaves the target's contents, and Â§ "Typed and pasted `..`"'s cells.
- **DONE**: bridge count zero in these directories.
- **First step done** (2026-11-21, PR #1322, branch `worktree-agent-a4ca45328a62809bc`; the lead on the volume manager,
  `listing_host/`, and `commands/`, one helper on `network/`, `about/`, `native_drag/`, `file_viewer/`, and
  `favorites/`, one on the rest of `file_system/` and `volumes/`). The mechanical conversion only, a clean partial.
  Bridge calls in the 11 directories 319 â 103 (the call-counting meter; `file_system/` 187 â 60, `network/` 51 â
  12, `listing_host/` 23 â 1, `commands/` 19 â 13, `file_viewer/` 13 â 4, `native_drag/` 10 â 5, `about/` 10 â
  4, `volumes/` 5 â 3, `favorites/` 1 â 1), `appTextToPath` 12 â 11, `pathFromBareText` 179 â 151; workspace
  bridges 1,351 â 1,135. No other directory's total rose, and no per-file entry grew. What converted:
  - **The volume registry's mount roots** (`file_system/volume/manager/roots.rs`, `manager.rs`, `root_move.rs`): the
    root set, rank, identity-conflict check, promotion, and every `Promoted { new_root }` are `AppPathBuf`;
    `roots::same_root` is the one comparison (`AppPath`'s `==`, `Path`'s `==` by construction; the rank's tie is
    `AppPath`'s `Ord`, `PathBuf`'s order by construction, a `..` included: the review's fix, after a name-text tie had
    put `/Volumes/-a` before `/Volumes/..` and a canonical-twin arm had equated Windows `\\?\C:\x` with `C:\x`). The
    OS-event entry points (`remove_root`, `move_root`, `find_by_root`) keep their native `&Path` and cross once at
    `roots::mount_root_from_os` (`from_native_unicode`, a non-Unicode name staying a lossless bridge value);
    `find_by_app_root`, `mark_root_stale`, and `would_keep_incumbent` take a root in hand. Durable design:
    `apps/desktop/src-tauri/src/file_system/volume/DETAILS.md` Â§ "A volume ID owns a set of mount roots".
  - **Test doubles and fixtures** in these directories: `app_path!` / `from_normalized` for canonical text, and
    `cmdr_fs::testing::AppTestDir` (re-exported from `crate::test_support`) for a scratch tree, so a cell hands a volume
    `C:/â¦` on Windows where the native tempdir was refused. Cells handed relative text keep `std::path` on purpose.
  - **Cross once per value** where a body crossed once per call, the same `legacy_unchecked` value made earlier, so
    byte-exact by construction: `tags_io.rs` (each wire path once, at grouping), the SMB encryption gate's native root,
    the macOS-mount handback's share root, the viewer's pull (`as_volume_calls_spell_it`), the drag-out file stream, the
    About card's xattr read, and the snapshot archive route's view root.
  - **The cells**: git and restic portal cells cross their tempdir once through `from_native_unicode` (the restic read
    gate matches a typed path's `names()`); the in-memory, SMB scan and index, MTP oracle, tags, network wiring, About,
    drag-out, and viewer cells are typed; the SMB gate's two tempdir cells use `AppTestDir` (their Windows tempdir
    panics, expected green, not run on the builder).
- **Byte-exact on macOS** (throwaway differential harnesses, old `Path` answers against the new; the lead's scratchpad):
  root equality 2,000,000 generated pairs and the OS-event equality 2,000,000, 0 differ; the rank's tie-break over
  103,583 equal-length pairs and the nested-mount log level over 2,000,000, every local difference holding a `..` name
  (no mount root does), the rest mixed root kinds or `//` leads (#1309) one id never holds. The one other candidate for
  typed arithmetic, `tag_store_for`'s longest-root fallback, measured 180,617 of 3,617,300 differing (every one a row
  holding `..` or a `//` lead), so it keeps `Path`'s logic and its two bridges. Suites on macOS: the app 6,807, green.
- **What stays, and where it goes**:
  - **The IPC edges** switched to `from_wire` in the first behavior-change commit (below). `go_to_path::resolve_with`'s
    `std::path` arithmetic (#1314 item 1) went typed with `resolve_typed` and `Volume::parent_for_dot_dot` in the second
    behavior-change step (below).
  - **The routing seam**: `VolumeManager::resolve(volume_id, &Path)` and its `ResolvedVolume.path` built by
    `legacy_unchecked` (archive, git, restic routes) went typed (below, "The routing seam went typed").
    `mount_id_for_path` keeps `Path`'s `starts_with` on its caller's text (a `..` included) until its callers (index
    routing, 10d's `inspect` tools) pass app paths.
  - **The text exits** (`commands/importance.rs`, `media_index/`, `volume_copy`'s root echo:
    `as_std_legacy() .to_string_lossy()`): an app path's own text differs for a Linux name that isn't UTF-8 while
    `ESCAPES_LIVE` is false, so they wait for 10f's type-level gate, as in 10a.
  - **Still 10c's, not started**: the listing host's `&Path` seams from MTP and SMB (`DirectoryChange` parent paths
    through `cmdr-listing`'s `notify_directory_changed` and the index's `apply_smb_change`), `cmdr-listing`'s 16
    crossings, `CoveredEntry.path`, and `listing.rs`'s `listings().location()` (a `PathBuf` from `cmdr-listing`).
  - **Kept on purpose**: native crossings that hand a root on as OS text (`smb_direct_switch`, `smb_connect_directly`,
    `smb_pane_upgrade`, the gate's `rerooted`), wire text turned into app paths in `about/through.rs` and
    `native_drag/fulfillment.rs` (a direct wrap changes a name holding an escape-range code point, #1310, and the folder
    walk's typed join answers differently on `/a/./b`), `live_server_edit::check`'s `Path` comparisons over
    `RemoteRoot`'s bridged values (`RemoteRoot` goes typed first), the eject and `drive_release` mount-path text exits
    (10f), and cells handed relative or empty text (git `.git/â¦`, in-memory `""`, share-relative media names, the
    relative SFTP cells, a drag-out `..`, a favorite stored with a trailing `/`).
- **First behavior-change commit done** (2026-11-21, approved by David the same day; loud rule 1's second exception, its
  `from_wire` half). One door, `apps/desktop/src-tauri/src/commands/wire_path.rs`; the durable design is
  `apps/desktop/src-tauri/src/commands/DETAILS.md` Â§ "Sent paths".
  - **Flipped**: `delete_files`, `trash_files`, and `move_files`' sources (a `..` refuses the start with the generic
    `IoError`, logged through `log_error!`; otherwise the text goes on as sent, since the starts drop a trailing `/`
    themselves and still take `PathBuf`s), `stat_paths_kinds` (a trailing `/` stats the link; a `..` is unknown),
    `share_link` (`VolumeError::IoError`), `suggested_ops`' destination check (a `..` is `Unknown`, warned, no bug
    report: a stored agent proposal can hold one until 10d's tools resolve it), and the destination probes
    (`path_exists`, `destination_exists`, `destination_write_access`), which anchor through the typed `anchor_at_root`
    (#1314 item 2: `path_checks`' `root_anchored` is gone). An escape-range code point stays literal
    (`from_literal_store`) while `ESCAPES_LIVE` is false, so a row's path crosses byte for byte.
  - **Pin (g) flipped**: `commands/file_system/stat.rs`'s `a_trailing_slash_names_the_link_and_a_dot_dot_is_refused`
    (was `â¦_are_the_os_answers_today`: `x/link/` read as a folder, `x/link/../y` as `real/y`). New cells, red first:
    `write_ops_wire_path_test.rs` (an IPC delete of `x/link/` removes the link and keeps `real/inner/kept.txt`; deletes
    of `x/link/../y` and `/Volumes/share/../etc` refuse and delete nothing; a move with a `..` source refuses; a
    same-root move lands as before), `wire_path_test.rs` (a row's path unchanged, NFD, an archive-inner path, scheme
    paths, an escape-range code point; the typed anchor on `/`, a share, and `C:/`), and `write_ops_destination_test.rs`
    (the Windows Move dialog's `/Users/Admin/w35/dst` on a `C:/` boot volume lands on `C:/Users/Admin/w35/dst`, run on
    every host; a POSIX root's destination is as sent).
  - **The Windows same-drive move** (found on the builder): the Move dialog hands `move_files` a volume-relative
    `/Users/â¦`, which nothing anchored on the boot volume's `C:/`, so the engine's `from_wire` refused it.
    `move_files` now anchors it the way a copy does, through `cmdr_operations::resolve_dest_path` (the typed anchor
    would need a bridge back to the start's `PathBuf` in a file with no budget, and would refuse a typed `..` the copy
    accepts), and so does `get_name_rules` for the boot volume. The other same-root commands (delete, trash, rename,
    create, compress) take row or pane paths, never the dialog's box, so they had no gap.
  - **A `..` in a person-typed destination**: resolved before the anchor since the second step ("Transfer box `..`
    done", below). The listing start, the create commands, rename, the pre-checks, the scan preview, and the volume
    transfers (whose engine starts already drop a trailing `/`) take the text as sent: their senders are rows, pane
    paths, or typed text, and a listing of `x/link/` lists the target's rows under `x/link` either way. The
    `cmdr-local`, delete-engine, and MCP pins (g) stay: they pin the volume's and the engine's own answers, or belong to
    10d (the go-to pin flipped in the second step).
  - **Review round** (2026-11-21): the single trash (`move_to_trash`) reads its path at the edge too, and its engine
    body (`trash_single_journaled`) drops a trailing `/` like every other start (the OS trashed the link either way; its
    `lstat` journaled the target's subtree). The MCP `nav_to_path` refuses a `..` typed, so no pane stands on one (rows
    under it would refuse every delete as a bug). Cells:
    `a_single_trash_through_a_dot_dot_is_refused_and_trashes_nothing`,
    `a_single_trash_of_a_folder_link_with_a_trailing_separator_trashes_the_link`,
    `a_dot_dot_name_is_refused_and_a_dotted_name_is_not`.
  - Bridge calls in `commands/`: 13 â 9 (`path_checks` 2 â 1, `volume_copy` 5 â 4, `share_link` and
    `suggested_ops` gone; `wire_path.rs` holds none), and `pathFromBareText` 3 down (`path_checks`, `write_ops`,
    `share_link`).
- **Windows**: `pnpm check clippy-windows` and `clippy-linux` green. The builder, the app's cells in these 11
  directories (1,236 tests): `main` 44 red â the branch 43, none newly red; of the baseline's 70 here, 27 pass and 43
  stay red, waiting on the routing seam, the two behavior-change commits, and the watcher fixtures
  (`docs/notes/windows-port/windows-test-failures.md` Â§ "By milestone", the TSV's `rerun` column).
- **#1311 done**: drops and app-path picks cross one native-to-app door, `commands/native_paths.rs`'s
  `app_paths_from_native` (`from_native_unicode`, a typed `Refused` per path), behind the frontend's
  `tauri-commands/native-paths.ts` (`onAppPathDragDrop`, `pickOneAsAppPath`, `chooseSaveTarget`); the pane drop, the Ask
  Cmdr drop, the media index's chosen folder, and the viewer's Save as take app paths. The adb binary, the SFTP key
  file, and the macOS `.app` picker stay native (only the OS reads them). `no-restricted-imports` names the webview's
  `getCurrentWebview` and `plugin-dialog`'s `open` / `save`. No bridge call added.
- **Second behavior-change step done** (2026-11-21, David's OK the same day; branch `worktree-agent-a6e246efe235deaa9`):
  Â§ "Typed and pasted `..`" landed, loud rule 1's second exception, pin (g) flipped for go-to.
  - **The trait method**: `Volume::parent_for_dot_dot`, lexical by default (`cmdr_fs::volume::dot_dot::lexical_parent`).
    `cmdr-local`'s Unix volume and `cmdr-sftp` override it with `dot_dot::parent_across_links` over a `LinkReader`
    (local: `readlink` machine-wide; SFTP: `LSTAT` + `READLINK`, a target off the root refused). It follows a target
    that is itself a link and walks a relative target's own `..`s the same way (40 hops, then refused). A missing name,
    or one under a file (`ENOTDIR`), is no link; any other read failure is refused. Windows, SMB, S3, MTP, ADB,
    archives, virtual volumes, and `InMemoryVolume` keep the default.
  - **The resolver**: `cmdr_fs::typed_path::resolve_typed_with(text, base, by: TypedBy, host)`, `TypedBy` = `Person`
    (address bar and go-to: `~`, Explorer's trailing-dot trim on Windows), `PersonViaCommandLine` (CLI, deep links: no
    `~`, literal names), `Agent` (`~`, literal, `OffRoot` above a root). The head before the first `..` parses with
    `from_external_text`; each later name joins through `AppNameBuf::from_typed`; `TypedPath.crossed_link` says when a
    volume's answer differed from the lexical one. The app's host is `go_to_path::AppHost` (the registry's longest root,
    routed volumes skipped, the boot volume as a local fallback; every stat and `..` detached under ONE 2 s `Deadline`);
    `AppHost::registry().resolve_typed(â¦)` is the door 10d's MCP inputs take.
  - **The callers**: go-to (`cmdr_fs::go_to_path` now classifies an `AppPath` over an async `GoToHost: TypedPathHost`,
    the browser demo too; `GoToPathResolution::{Directory, File}` gained `crossedLink`, and the dialog's preview names
    the real destination in the `info` tone, new key `goToPath.dialog.throughLinkHint`, translated in all 13 languages).
    The CLI: the script now makes paths absolute by text, steps a relative path's leading `..`s up from `pwd`, and hands
    any other `..` on (the parser no longer refuses one); `reveal::delivery::resolve_dot_dots` answers it at delivery,
    after `setup`. Deep links (`file://`) never carry a `..` (URL parsing removes dot segments) and pass the same door
    untouched. A path with no `..` resolves exactly as before on macOS and Linux.
  - **Cells**: `typed_path_test.rs` (every spec example over a link table: `/x/link/../y`, `../y` from inside `/x/link`,
    `C:/x/link/../y` and `C:\x\link\..\y` under Windows rules, `sftp://h:22/a/link/../b`, the missing prefix, above the
    root for a person, the CLI, and an agent, a timeout), `dot_dot_test.rs` (the walk), `cmdr-local`'s
    `local_posix_link_test.rs::dot_dot` and `cmdr-sftp`'s `dot_dot_test.rs` (a Docker cell plus a no-session refusal) on
    real links, and the app's `go_to_path/{tests,wire_shape_tests}.rs` (pin (g) flipped: a `..` after a folder link
    lands on `real/y`; a hung mount refuses through `WedgedVolume` within the deadline; no volume refuses),
    `cli/script_tests.rs` and `reveal/delivery_test.rs`. Bridge: `go_to_path/mod.rs`'s entry gone (1 â 0); no file
    grew.
  - **Review round** (same branch): `GoToHost::found` tells "couldn't check" (`Err`) from "nothing there", so a hung
    mount is `Invalid` instead of a nearest-ancestor walk to the root; the script's existence check runs on its result,
    so `cmdr ../sibling` from a linked folder finds what `cd` finds.
  - **Transfer box `..` done** (2026-11-21): the probes, the conflict scan, and the copy/move/compress starts resolve it
    at the edge (`commands/wire_path.rs::destination`, over `AppHost::on_volume`), the frontend folds only a relative
    input's leading `..`s; durable design `apps/desktop/src-tauri/src/commands/DETAILS.md` Â§ "Sent paths".
  - **Left for 10d**: the MCP tools' path inputs call `AppHost::registry().resolve_typed(text, base, TypedBy::Agent)`
    and turn `TypedPathRefusal` (`OffRoot`, `Unanswered`, â¦) into typed tool errors. â `OffRoot` is a TEXT check (a
    `..` at an app-path root); it says nothing about where the answer lands. `/allowed/link/..` with `link -> /etc/x`
    resolves to `/etc` with `crossed_link: true` and no refusal, and the resolved path's own last name may be a link
    too. Any allowed-roots rule 10d enforces must check the RESOLVED place (its canonical form, links included), â
    never the typed text. The MCP `nav_to_path` refuses a `..` today (first commit's review round); 10d replaces that
    refusal with this door, so an agent's `..` lands where the volume says, never on a pane path holding one.
- **The routing seam went typed** (2026-11-21, branch `worktree-agent-a47eecf73006454cd`). `VolumeManager::resolve`,
  `resolve_to_list`, `resolve_local_only`, and the two archive predicates take `&AppPath`; `ResolvedVolume.path` is the
  caller's app path (`ResolvedVolume::passthrough_at`; the `&Path` `passthrough` stays for the engines' host traits).
  The archive route confirms and mints typed (`cmdr_archive::confirm_archive_boundary_on_parent`,
  `confirm_remote_archive_boundary`, `confirm_archive_boundary_at`, `ArchiveVolume::new_at`; the `std::path` forms of
  the three confirms left the crate's surface, and `index-crate-isolation`'s `SubsystemItems` rose 32 â 33 for
  `new_at`, as `crates/cmdr-archive/DETAILS.md` foresaw). An archive inside a snapshot is found by
  `archive_routing::archive_below` (typed ancestors under the view's root). The archive registry id hashes the native
  canonical file (`to_native`), or the app text for a scheme path; `forget_archive_index` crosses the engine's `&Path`
  once and lands on the same id (pinned). Archive routing cells are typed over `AppTestDir`, so the 11 Windows S cells
  over `resolve`'s seam (`archive_routing::tests::*`, `archive_watch_integration_test`) read `C:/â¦`.
  - **Byte-exact on macOS and Linux for real input**: the git and restic candidates read the same `std::path` spelling
    as before (`routing::lexical_spelling`, one crossing), so the boundaries' order is unchanged; the archive split is
    `archive_boundary_candidate_at`, which agrees with the `std::path` split on every canonical path
    (`crates/cmdr-archive/DETAILS.md`'s differential run). The viewer's `as_volume_calls_spell_it` (bridge out and back)
    folded into one crossing of the request, the identity it already was.
  - **Callers that read wire text now read it typed at the edge**: `stat_paths_kinds`' route (`wire_path::app_path`),
    the unlock commands and the MCP `unlock` tool (`wire_path::on_volume`: a `..` or relative path refuses as
    `UnlockError::Volume`), the probes' volume pick (`path_checks::box_volume`: `read_on_volume`, else the volume by
    id), and `resolve_location`'s archive split (`confirm_archive_boundary_at`, then `to_native`).
  - **Crossings added at seams, one named helper each** (the lead's rule for this refactor: each listed with who removes
    it): `routing::lexical_spelling` (until `cmdr_git::portal_route` and `cmdr_fs::restic_view` take app paths;
    `cmdr-git`'s surface is capped, David's call), `listing_host/volumes.rs::listing_path` (step 3 below, the listing
    engine's paths), `operations_host/volumes.rs::engine_path` and `wire_path::checked_as_app_path` (10d, the write
    engine's host trait and starts), `about/through.rs::row_as_app_path` (10f, with `through.rs`'s rows, #1310), and
    10d's `inspect/archive.rs::agent_text_as_app_path`, `multi_rename/app_host.rs::rename_dir_as_app_path`,
    `listing_index_sizes/count/mod.rs::counted_dir_as_app_path`. Bridge calls workspace-wide 1,105 â 1,102 (the route
    files: `archive_routing.rs` 8 â 2, `restic_routing.rs` and `boundary_on_parent.rs` gone).
  - `path_routes_over_its_parent` (the lexical gate) keeps `&Path`; its callers hold `std::path` values today.
  - **Windows not run on the builder this step** (it was busy): the routing cells are expected green, the TSV's `rerun`
    column still says `red@ff3c3e0eab`. `clippy-windows` is green.
  - **Not started, still 10c's**: the listing host's MTP and SMB `&Path` seams (`ListingHost::directory_changed`, a
    `cmdr-fs` host trait every backend crate calls: `cmdr-smb`, `cmdr-mtp`, `cmdr-nfs`, `cmdr-local`, `cmdr-fs`'s
    patching, the browser host), `cmdr-listing`'s crossings with `ListingVolumes` (typing its `resolve` needs the
    engine's listing locations typed first, or `streaming.rs` gains a second crossing), and `CoveredEntry.path`
    (`cmdr-index`'s, read by 10d's `search/`).

- **The listing engine went typed inside** (2026-11-22, branch `worktree-agent-ae4e56b258767a21d`): `ListingPath`,
  `ListingRequest.path` (read at the IPC edge with `wire_path::on_volume`: a trailing `/` goes, a `..` refuses the
  start), `ListingEnd`, the engine's own seams (`ListingVolumes`, `ListingOverlay`, `ListingIndex`, `respelled`,
  `OpenedListing`), `stored_spellings`, and `entries_as_stored`. `listing_host/volumes.rs::listing_path` and
  `cmdr-listing`'s per-call bridges are gone; the stalled-on wording and the permission arm cross `to_native` past their
  splits (`cmdr_fs::volume::friendly_error::listing_error_at`, `enrich_with_provider_at`,
  `listing_error_for_restricted_empty_root_at`). Durable design: `crates/cmdr-listing/DETAILS.md` Â§ "App paths at the
  engine's edge".
  - **Not done, and why**: `cmdr_fs`'s `ListingHost` and the `Listings` methods behind it (`directory_changed`,
    `authoritative_listing`, `cached_entries`, `listings_for_path_on_volume`, `location`, `open_listing_paths`,
    `refresh_archive_listings`) keep `std::path` at one crossing (`cached_listing.rs::{from_host_seam, to_host_seam}`):
    typing them puts a crossing in each of `cmdr-operations`' oracle callers (`scan_walker`, `scan_oracle`,
    `conflict_preflight`, `look_alike`, `scan_preview/volume`, `delete/volume_scan`, `archive_edit/engine`) and in
    multi-rename's `name_rules_at`, so it lands with the write engine's typed starts (10d). `CoveredEntry.path`,
    `rename_dir_as_app_path`, and `counted_dir_as_app_path` wait on the same step.

### 10d. The app, part two: operations host, tools, and the rest

- **Size**: M. After 10c (review round 2: it calls 10c's `file_system/` helpers and `resolve_typed`); it may start on a
  branch off 10c's first green push.
- **Scope**: `operations_host/`, `tools/` (MCP), `agent/`, `search/`, `listing_index_sizes/`, `multi_rename/`,
  `services_menu/`, and whatever 10c didn't own.
- **Landmines**: the MCP tools take paths from an agent, the input most likely to carry `..`, a trailing `/`, or a
  relative path; they go through `resolve_typed` (a `..` is the volume's answer; past the root is `OffRoot`) with the
  same deliberate-change commit as 10c, and a refusal is a typed tool error, never a silent re-anchor onto the root
  (today's `root_anchored` joins a relative path onto the root). Agent surfaces never carry an escape: outputs use the
  shown form plus `unaddressable`, and an input holding U+FFFD that a sibling's shown name matches is `AmbiguousName`
  (Â§ "The escape on the wire and in the frontend", Agent surfaces).
- **Test plan**: Windows: the remaining app failures that name "not an absolute Windows app path" (54 of the 65:
  `multi_rename/` 44, `tools/actions/multi_rename_test.rs` 9, `redact/` 1; review round 4 by name, none in
  `operations_host/`) or a `\photos`-style path; `cmdr` totals reported against the baseline. macOS/Linux: suite, plus
  an MCP write given `/x/link/../y` lands where `ls` says and one given a path past the root refuses typed; Linux: an
  MCP listing of the non-UTF-8 fixture shows `caf\u{FFFD}.txt` flagged `unaddressable`, and an MCP delete of
  `caf\u{FFFD}.txt` with both files present refuses as `AmbiguousName`.
- **DONE**: the app's bridge count is zero, and the backend crates' `testing::scratch_dir` `std::path` twins (WebDAV,
  NFS) are gone with their last callers, the app's backend suites.
- **First step done** (2026-11-22, branch `worktree-agent-a2853e627913731e2`): the MCP tools take typed paths, loud rule
  1's second exception for the agent surface. Durable design: `apps/desktop/src-tauri/src/tools/DETAILS.md` Â§ "An
  agent's path".
  - **One door in the dispatch**: `tools/agent_path.rs::typed_params`, right after the schema gate in `execute_tool`
    (and in `agent/tool_gate.rs`'s `propose_rename_plan` branch, which skips it), rewrites every declared path field
    through `AppHost::registry().resolve_typed(â¦, TypedBy::Agent)` before a person is asked or a handler reads it.
    Refusals are typed tool errors, `data: { reason, field, path }`: `pathRelative`, `pathOffRoot`, `pathUnanswered`,
    `pathNotAPath`, `pathLeadsOffVolume`. `fields(tool)` lists the fields (JSON pointers with `*`, scope strings,
    `await`'s `value` under `condition: "path"`); `every_path_shaped_schema_property_is_a_declared_field` walks every
    schema. `nav_to_path`'s `dotDotName` refusal is gone (pin flipped: `x/link/../y` lands where `ls` says).
  - **The allowed-roots rule judges the resolved place**: the location resolver's named-volume check sees the resolved
    path (`a/link/..` with the link on `b` is `pathOutsideVolume` for volume `a`), and a WRITE's place (a proposal's
    destination, rename parent, selector root, sources) has its physical place (a compare-only `canonicalize` of the
    longest existing prefix; a source's own last name not followed) checked against the registered volume its text
    names: another one is `pathLeadsOffVolume` with `resolved`. Reads follow links as the OS does. Cells:
    `tools/agent_path_tests.rs`, real links over `AppTestDir`, every OS (`skip_without_symlinks`).
  - **Stored `suggested_ops` destinations are typed**: `WritableDestination::new` types its path
    (`agent/store/proposals/mod.rs::typed_place`), `ProposalGroup.destination` is `Option<AppPathBuf>`, the destination
    check reads it as is (its `read_on_volume` step gone), and a legacy row holding a `..` reads as no destination (its
    approval refuses `TargetMissing`). While `ESCAPES_LIVE` is false a destination holding an escape-range code point is
    refused at creation (the starts read it back as a `PathBuf`); 10f lifts it.
  - Bridge: `inspect/archive.rs::agent_text_as_app_path` gone (the route reads the resolved text as an app path).
  - The Linux non-UTF-8 `unaddressable` / `AmbiguousName` cells wait for 10f's producers.
- **Second step done** (2026-11-22, branch `worktree-agent-af2ac782e654b358b`):
  - **The scan preview's route** reads its first source through `wire_path::app_path`; `checked_as_app_path` is gone.
  - **`cmdr://importance?path=` and `&under=`** pass the door (`resources/importance.rs::typed_query` over
    `agent_path::read_path`, a refusal the tools' typed error via `ResourceError::Path`); the resource's own `$HOME`
    tilde is gone (the door spells home as the app path, which the Windows cell needed).
  - **#1354, two of five**: a review re-runs the door's `WriteTarget` walk per render (`write_lands_off_volume`) and the
    card names `destinationLandsOn` (disclosure, never an execution refusal); an escape-range destination refuses
    `destinationNameUnsupported` (`UnwritableDestination`), no longer the archive message.
  - **`listing_index_sizes/count/measure.rs`** types both seams (the local root crosses at `to_native_plain`, the volume
    scan parses the row text with `from_literal_store` / POSIX); `search/`'s test bridges are typed.
  - Bridge calls workspace-wide 1,100 â 1,091; the app's 351.
  - **Left, and why**: `operations_host/volumes.rs::engine_path` stays: `OperationVolumes` is `cmdr-operations`' trait
    and its ~15 engine callers hold `PathBuf`s (sources, temps, ledger rows), so typing it moves the crossing into the
    engine and grows the total; it goes with the engine's starts (`move_files_start` and friends taking app paths), a
    `cmdr-operations` effort. `multi_rename/app_host.rs::rename_dir_as_app_path` and
    `listing_index_sizes/count/mod.rs::counted_dir_as_app_path` read `cmdr-listing`'s `PathBuf` location (10c's
    `listings().location()` leftover). `search/volumes.rs`'s and `tools/connect.rs`'s `to_string_lossy` text exits are
    10f's. `services_menu/` carries app text in `PathBuf`s from `menu/`'s `MenuState.context.paths` to the pasteboard
    (an OS exit): it types with `menu/`. The Windows `multi_rename/` "dir lists" cells pass on the hosted lane already
    (none in `ci-known-failures.tsv`); `tools::actions::multi_rename::â¦a_proposal_stagesâ¦` (cluster S) stays. #1337
    stays open: it needs a `TypedPathHost` long-name step inside the resolve deadline, a placement call.
  - **The DONE line is not met**: 351 app bridge calls remain, 242 in `operations_host/` (the backend suites, which also
    hold the WebDAV and NFS `scratch_dir` twins' last callers), 52 `file_system/`, 12 `network/`.
- **Third step done** (2026-11-22, branch `worktree-agent-a48144939a26577f5`, stacked on #1365's branch):
  - **An agent's rename proposals join by the app path** (`multi_rename`'s tool, `propose/rename`'s plan and preflight,
    the spine's `shared_parent`, the `rename` tool's target): a Windows drive folder's rows read `C:/x/a.jpg` (four
    Windows known failures gone); a name a folder can't hold as one name refuses typed. Design:
    `apps/desktop/src-tauri/src/tools/DETAILS.md` Â§ "An agent's path".
  - Every `tools/` and `agent/` `appTextToPath` site is gone; an engine that still keys by `std::path` text takes the
    text at `wire_path::as_sent_std_path`.
  - **Left, and why**: the backend suites' bridge calls (about 220) and their `pathFromBareText` sites wait on the write
    engine's starts taking app paths: every suite holds its sources as `PathBuf` for those starts, so typing a suite
    first moves each crossing to an `as_std_legacy` beside the start. Same for `operations_host/volumes.rs::engine_path`
    and `agent/suggested_ops/bridge/` (its `op_ids` key and rows are the starts' `PathBuf`s). `tools/sensitive.rs`'s
    realpath gate and `tools/read/inspect/` (the viewer's archive seam) cross at a typed native door that fails closed
    on a `..`, a call for their owners. `agent/store/migrations.rs` is a shipped step, â not edited.
    `agent/chat/runtime/cmdr_md.rs` reads an env var's native path (true native, a `pathFromBareText` false positive).
    Issue #1351's `/C:\â¦` journal rows weren't reproduced off Windows: the journal's `local_split` stores whatever the
    engine's `PathBuf` holds, and the reader (`escape_mark.rs::app_path_as`) already reads a drive row's `\`; the fix is
    the engine journaling typed ends, with the starts.
- **Fourth step done** (2026-11-22, with 10a's sixth pass, same branch): `operations_host/volumes.rs::engine_path`, the
  write commands' `PathBuf` sources, and `suggested_ops/bridge/`'s `PathBuf`-keyed op map are gone; the backend suites'
  concrete cells are typed. What's left and why: 10a Â§ "Left after the sixth pass".

### 10f. The escape lands

- **Size**: M. Added in the type's review (2026-11-18): the escape producers used to ride 6, 7, and 9, which land before
  the consumers stop turning app text into `std::path` values; an escape reaching one of those sites opens a file
  literally named with the escape code point (Â§ "Keeping it that way", the escape gate).
- **First, the type-level gate** (Â§ "Keeping it that way"), two commits before any producer: `FileEntry.path` and
  `.name` become typed (the last untyped carriers; IPC parameters went typed in 10c), then `as_str`, `into_string`, and
  the `Display` replacement go into `clippy.toml`'s `disallowed-methods` with a reasoned `expect` at each IPC, store,
  and log exit.
- **Scope**: every escape producer, each in its own commit: every `from_native_unicode` / `from_native_os_unicode` that
  9, 10a, and 10b left becomes `from_native` / `from_native_os`; `cmdr-local`'s readers and walks spell a non-UTF-8 name
  (Linux bytes, Windows unpaired surrogates) losslessly, `FileEntry.name == path.file_name()`, with the frontend's name
  sinks (`shownName`, `nativeNameLength`) and the Linux DOM catch-all, which is loud rule 1's first exception (milestone
  9's text on it moves here, its pins (f) flip here); SFTP's byte escapes (6); SMB's unit escapes if `smb2` hands up a
  lone surrogate (7); NFS's choice (6). â First, #1310: a literal escape-range name reaches the remote and device
  backends escaped from the app and literal from their own listings, so one door must make the two spellings one before
  any producer goes live.
- **Gate**: the type-level gate's two commits are in and clippy is green with them; `app-path-std-bridge`'s
  `appTextToPath` and `pathFromBareText` budgets are empty (the tripwire refuses a production escape producer
  otherwise); and 10aâ10e are landed.
- **DONE**: #1236 closes; the escape gate's budget stays empty for good (the check keeps refusing a new site).
- **Groundwork done** (2026-11-22, branch `worktree-agent-a4a94e12db1c258a8`; one agent): #1310's door and the
  per-volume namer, ahead of the type-level gate. The design: `crates/cmdr-fs/DETAILS.md` Â§ "App paths" (the listed
  escape-range bullet). In short:
  - **One spelling of a listed escape-range name**: a backend's own listing text enters typed only through the
    listed-text door (`AppName::from_listed_text`, the new `AppPathBuf::from_listed_text`), the bridge's and the wire's
    spelling of the same text, and every remote and device backend reads app text back through
    `cmdr_fs::app_path::listed_text`, its exact inverse (`RemoteRoot::to_remote` for SFTP, WebDAV, S3, NFS, and ADB;
    SMB's `to_smb_path`; MTP's `device_name`; patch row names through `listed_row_name`). `RemoteRoot::to_app` and SMB's
    `app_path_of` build through the door; MTP's `listing_path`, report URLs (`url_of`), pre-scan children, and
    `move_rename::sibling` respell the session layer's literal storage text.
  - **Behavior changes**: a name listed with an escape-range code point opens, deletes, and patches on SFTP, WebDAV, S3,
    NFS, SMB, and ADB (it answered `NotFound`), and an MTP folder holding one reports at the URL its pane keys on. App
    text no listed name escapes to (a live escape from a non-UTF-8 local name joined onto a server, both alphabets, a
    non-literal look-alike) is refused there (`None` / `NotFound`), where it went to the server as its code points' own
    bytes, a file literally named with the escape. `RemoteRoot::new`'s typed root still refuses escape-range text (S3's
    connect refusal stands).
  - **The namer**: `Volume::namer` (default `BackendKind::namer`), which `VolumeLookup::namer` asks; `BackendKind::Mtp`
    is units (PTP strings, and what `typed_path`'s scheme rule already picked), and an archive volume answers its
    format's (`ArchiveFormat::namer`: a 7z units, zip and tar bytes).
  - **Left**: MTP's index feed (`ResolvedMtpObject::path`) still hands the literal storage path over typed, and MTP's
    `notify_mutation` reads a `Created`/`Renamed` name as app spelling (its own producers send typed names);
    `MutationEvent`'s names have no stated spelling. Both before 10f's producers: #1355. #1309 (a `//` lead) and #1337
    (Windows short names) stayed out: the first changes the bridge's byte-exact round trip, a design call; the second is
    the app's typed-path resolver, other code.

- **The app's text budgets, swept toward the gate** (2026-11-22, branch `worktree-agent-a69e767ef6bf51251`, on #1373):
  the app's `appTextToPath` 5 â 1 and `pathFromBareText` 156 â 57 (bridge calls 327 â 313). Native text crosses at
  named doors: `src/native_text.rs` (`os_path`, `os_path_buf`, `env_path`: env vars, settings naming a binary or key
  file, a process's executable, test fixture literals) and `volumes::os_mount_path` (the per-OS mount lookups). App text
  reads through `app_text` (`parse_on_volume` for any volume's text, `as_std_path_text` for an engine keyed by
  `std::path` text). The About card is typed at its edge (`Through.path`, `Probe.native`, the OS reads on `&Path`;
  `through.rs`'s bridges gone), `mount_id_for_path` places by `is_on_volume`, and go-to spells a Windows 8.3 short name
  long inside the resolve deadline (#1337). Left and owned: `operations_host/` (the write engine's starts), the listing
  host seam's tests (`listing_host/backends.rs`, the seam is still `std::path`), `tools/sensitive.rs` (#1371),
  `agent/store/migrations.rs` (shipped), `tags_io` (10f), `agent/suggested_ops/bridge/decorator.rs` (the starts'
  `PathBuf` keys), `native_drag/fulfillment.rs` (kept, #1310).
- **10f retires every `app_text::as_std_path_text` use first** (app text byte for byte into an engine keyed by
  `std::path` text; the escape gate can't see these, so they're listed here, and none may hand its value to the OS):
  `file_system/git/commit_counts.rs` (two: the portal's rows), `listing_index_sizes/count/mod.rs` (the view-row check),
  `about/route.rs` (the snapshot's portal read), `about/history.rs` (the operation log's lookup),
  `tools/read/inspect/mod.rs` (the routed and materialize seams; its plain read crosses through `app_text::native`),
  `tools/read/inspect/archive.rs` (two: the archive seam, whose `std::fs::metadata` of the archive file is that seam's
  own crossing), `commands/wire_path.rs` (`as_sent_std_path`, which `commands/file_system/drag/platform/macos.rs` uses
  for a promise drag's rows, #1310), and the cells `tools/read/inspect/archive_tests.rs`,
  `file_viewer/materialize_test.rs` (three), `search/live/tests.rs`, and `search/live/walked_tests.rs`. The review on
  #1379 moved Open with, Services, a local drag's file URLs, the inspect tool's plain read, and a favorite's icon off
  these and the mount door onto `app_text::native`, and `open_mock` records text with no crossing at all.
- **If `ESCAPES_LIVE` went on and then back off** (answered 2026-11-23, from the code): the switch is one-way in
  practice; â never ship an off build after an on build without first making every store below carry its own mark.
  - **The operation log**: the first live open stamps `escape_mark_last_literal_op_id` once
    (`crates/cmdr-operation-log/src/store/escape_mark.rs`), and an off build never stamps or clears it. So every op an
    OFF build writes after the mark reads `Live`. A row naming a file literally called with an escape-range code point
    (valid UTF-8, rare) then decodes to a DIFFERENT name: the N7 hazard the mark exists for. Rollback re-verifies each
    item against its snapshot and skips drift or a gone original, so a delete or overwrite can't hit the wrong file, but
    a put-back (trash, move) whose target is free restores under the decoded name. The rows written WHILE live read
    right in the off build (the mark says `Live`, and `to_native` decodes escapes whatever `ESCAPES_LIVE` says). The fix
    if a round trip is ever needed: the mark becomes a list of op-id intervals (stamped at each switch), still no
    migration of row shapes.
  - **Recents, settings, favorites, tabs, and the frontend's stores** carry no mark: their text reads through
    `app_text::parse` / `from_wire`, which follow the CURRENT build's `ESCAPES_LIVE`. An entry saved live (a Go to path
    history row for `caf\xE9`, spelled with a live escape) reads in an off build as literal text, so it names a folder
    literally called `caf` + U+10FEE9: it opens nothing (`NotFound`), or, if such a folder exists, the wrong one. Saved
    off and read live is the mirror image. None of these stores feeds a write without a person's click on what it
    resolved to, so the failure is a wrong or missing place shown, â never a silent write.
  - **The agent's proposal store and the drive index**: proposals refuse escape-range destinations at creation while off
    (`agent/store/proposals/mod.rs::typed_place`), so a live row read off refuses as `TargetMissing`; the index's stored
    text (`crates/cmdr-index/src/indexing/paths/app_text.rs`) keeps today's lossy spelling until 10f decides it.
- **The per-site list that gates 10f** (2026-11-23, this branch on top of `main` at `5b1c2b99ea`): every site the
  `app-path-std-bridge` tripwire counts, by the check's own regexes over its scanner roots (`apps/` and `crates/`, minus
  `examples/`, `build.rs`, and `crates/cmdr-fs/src/app_path/`). The budgets hold `appTextToPath` 42 and
  `pathFromBareText` 272: 314 sites, 57 of them in the app (1 + 56); 34 and 269 (303) after 10a's seventh pass, whose
  sites are off the list below. Each line names the site, its budget, its class, and whether it's a HAZARD (app text
  that can carry a live escape and reaches the OS or a volume as `std::path` bytes, so it opens a file literally named
  with the escape), then why it's still there, what removes it, and whether it needs an exemption.
  - **Classes**: 178 `test-fixture` (a cell or suite helper hands literal or app text to a `PathBuf`-taking helper; no
    hazard with ASCII literals, but the gate counts them), 45 `name-arith` (`extension`, `file_stem`, `parent`, a
    compare; nothing opened), 43 `engine-body` (production code keyed by `PathBuf`), 28 `native` (env vars, xattr keys,
    a DB or log file, mount tables, `hdiutil` output: not app text), 12 `demo` (`cmdr-listing-wasm` calling an engine
    API still keyed by `PathBuf`; no OS in a browser), 8 `pin` (`std::path` pins that go at milestone 11).
  - **The HAZARD sites are 10f's real blockers**, all `engine-body`: 37 at the snapshot, 26 left after 10a's seventh
    pass (2026-11-23, which took the 11 walker and destination sites, listed there). Left: in the app `tags_io.rs:98`,
    `native_drag/fulfillment.rs:338`, `tools/sensitive.rs:184`; in `cmdr-fs` `volume/server_side.rs:102`; in
    `cmdr-local` `platform/local_posix/scan.rs:57`; 13 in `cmdr-operations` (the create, rename-validity, and
    conflict-preflight-sources entry points still take text, `scan_walker`'s cached walk (it flips with the lossless
    row), plus `leaf_validation`, `target_names`, and `transfer_leaves` holding a new name as `OsStr`); and 8 in
    `cmdr-index` (stored frontier, branch, and rescan text handed to a walk, a stat, or the OS watcher). A walker's
    `legacy_unchecked(Path::new(&entry.path))` round trip isn't the identity once escapes are live: it re-escapes the
    escape as literal text, so the volume gets a mangled name.
  - **â The list is REGEX-SCOPED, so it undercounts the hazards.** The tripwire only sees an identifier named
    `path`/`name`/`dir`/`root` (or `*_path`â¦) or a `.path`/`.name` field on one line, so
    `PathBuf::from(&op.source_path)` and `Path::new(&expanded)` pass it. A sweep of every production `Path::new` /
    `PathBuf::from` / `OsStr::new` / `OsString::from` on a variable (2026-11-23, 173 sites off the list, read one by
    one; most are native: mount tables, env, CLI args, settings naming a binary, server roots) adds 27 hazards, plus 12
    `seam_path` crossings:
    - **Off the list, app or stored text reaching the OS or a volume** (27):
      `agent/suggested_ops/bridge/mod.rs:146, 475, 482, 505` (the approval's sources, rename rows, and leaf keys);
      `commands/agent/bulk_rename.rs:202, 203` (the reviewed rename's rows); `commands/wire_path.rs:139` (`checked`: the
      single trash and the rename pre-flight); `app_text.rs:78` (`as_std_path_text`, whose engines answer the OS on some
      seams, listed above); `commands/file_viewer.rs:116`; `clipboard/platform/macos.rs:98` (a paste's target);
      `priority/roots.rs:444` (a favorite's text as an index priority root); `services_menu/selection.rs:179` and
      `quick_look/platform/macos.rs:40, 73` (OS exits); `tools/actions/mod.rs:417` (an agent path's `exists`);
      `listing_index_sizes/count/measure.rs:87`; in `cmdr-operations` `create.rs:500`, and
      `types/transfer_leaves.rs:108`; in `cmdr-fs` `archive_format.rs:228` (the archive boundary split) and
      `twin_path.rs:150, 160` (an MTP twin's join); in `cmdr-viewer` `session.rs:293`; in `cmdr-index`
      `paths/app_text.rs:89` (the door the 8 above route through), `lifecycle/phases/stitch.rs:94`, and
      `watch/event_loop/live.rs:140`.
    - **The `seam_path` crossings** (a named helper over `legacy_unchecked`, counted once in `wire_paths.rs`), 14 at the
      snapshot, 11 left: `in_flight_sweep.rs:287, 404`, `create.rs:55, 71`, `archive_edit/engine.rs:104`,
      `create_numbered.rs:44`, `conflict_preflight.rs:195` (the sources), `rename/bulk/by_move.rs:84, 85, 214`, and
      `routing/destination.rs`'s fallback for a root or text the typed anchor has no reading for (Windows suites'
      native-rooted volumes). A `std::path` value the engine made from a typed path round-trips losslessly on macOS and
      Linux, but on Windows a POSIX volume's byte escape has no OS spelling, so `as_std_legacy` keeps its code point as
      text and `seam_path` re-escapes it as literal: a mangled name. 10 of the 11 are new here (`create.rs:55` is on the
      list).
    - **Corrected hazard count: 56** after 10a's eleventh pass (22 on the list, 25 off it, 9 `seam_path`: the
      preflight's sources, `leaf_validation`'s leaf join, `target_names`, and `transfer_leaves`' two went typed). **62**
      after 10a's ninth pass, milestone 1 (26 on the list, 26 off it, 10 `seam_path`: the two driver roots
      `journal.rs::record_volume_transfer_source` read are typed now); 64 after the eighth (26, 26, 12, those two new
      there); 62 before it (26, 26, 10); 76 at the snapshot (37, 27, 12), 15 gone with 10a's seventh pass:
      `resolve_dest_path` (off the list), the 11 walker and destination sites, and the `seam_path` crossings at
      `conflict_preflight.rs:142`, `routing.rs:686`, and `routing/destination.rs:41`, one new (the anchor's fallback).
      The demo's off-list sites (`cmdr-listing-wasm`'s `paths.rs:41`, `search.rs:263`, `tree.rs:129`) reach no OS, so no
      hazard. A regex can't make this list complete; the type-level gate (Â§ "Keeping it that way") is what can.
  - **27 sites need an exemption**, and the check has no directive for one today: env vars (`cmdr-adb`, the `cmdr-index`
    benches, the SFTP fixture's key dir), xattr key names (`cmdr-archive`, `cmdr-smb`, `cmdr-fs` caps tests), the SQLite
    and in-flight log file paths, `hdiutil` output, the SMB fixture's mount point, `rustic_core`'s `OsStr` node names,
    and the local trash's native records. Each either moves behind a named native door the scanner doesn't match (as the
    app's `src/native_text.rs` did) or takes an `allowed-â¦` directive, which is a check change and David's call.
  - **The exemptions to decide** (2026-11-25, after 10a's eleventh pass; "door" = a named native door in `cmdr-fs`, like
    the app's `native_text::env_path`; "directive" = a new `allowed-native-text: <reason>` the scanner honors):
    1. `cmdr-adb` `server.rs:244`, the adb binary from an env var (`OsString`): door.
    2. `cmdr-adb` `server/binary_search.rs:101`, `ANDROID_HOME`-style env dirs: door.
    3. `cmdr-adb` `server/binary_search.rs:130`, a hardcoded native search path: door.
    4. `cmdr-fs` `sqlite_util.rs:543`, a DB's `-wal`/`-shm` file names: door.
    5. `cmdr-operations` `in_flight_log_writer.rs:191`, the ledger log's own file: door.
    6. `cmdr-index` `manager/start.rs:450`, `CMDR_E2E_START_PATH`: door.
    7. `cmdr-index` seven benches and spikes read a root from an env var: door (one `env_path`).
    8. `cmdr-sftp` `volume/testing.rs:197`, the fixture's key dir env var: door (`var_os`).
    9. `cmdr-smb` `volume/test_support.rs:63`, a fixture's mount point: door.
    10. `cmdr-fs` `testing/disk_images/facts.rs:609, 616`, `hdiutil` output: door.
    11. `cmdr-archive` `apple_double.rs:206`, an xattr key, not a path: directive.
    12. `cmdr-archive` `read/tar.rs:194`, a PAX record's xattr key: directive.
    13. `cmdr-smb` `volume/copy_xattrs.rs:143`, a stream name as an xattr key: directive.
    14. Three test xattr-key literals (`cmdr-archive` two, `cmdr-fs` `caps_tests.rs`): directive.
    15. `cmdr-restic` `tree_tests.rs:22`, `rustic_core` takes node names as `OsStr`: directive.
    16. `cmdr-operations` `delete/cloud_trash.rs:303, 323` and `trash_tests.rs:363`, local trash records: directive.
    17. The write engine's native `Path::join`s (`stdJoin`, 71: the local engine, tempdirs, the in-flight ledger):
        per-site `allowed-std-join` (works today), or exempt the local-engine folders by path in the check.
  - **The list** (line numbers at this branch's commit; the meter re-derives them, so re-run the check's regexes after
    any edit):
    - `apps/desktop/src-tauri/src/agent/store/migrations.rs`
      - :568 pathFromBareText, name-arith, no hazard: migration groups proposal_ops.source_path by std::path parent
        (stored path text's parent compared as text, nothing opened). Goes with: AppPath::parent on the stored text
        (parse the row as AppPathBuf). No exemption.
    - `apps/desktop/src-tauri/src/file_system/tags_io.rs`
      - :98 pathFromBareText, engine-body, HAZARD: IPC Vec<String> paths go Path::new then legacy_unchecked (re-escapes)
        into volume tag calls. Goes with: typing the tags IPC params as AppPathBuf via from_wire at the command edge. No
        exemption.
    - `apps/desktop/src-tauri/src/listing_host/backends.rs`
      - :134 pathFromBareText, test-fixture, no hazard: AppListings::authoritative_listing takes &Path (literal ASCII
        test path). Goes with: typing authoritative_listing to take &AppPath. No exemption.
      - :163 pathFromBareText, test-fixture, no hazard: AppListings::authoritative_listing takes &Path (literal ASCII
        test path). Goes with: typing authoritative_listing to take &AppPath. No exemption.
      - :293 pathFromBareText, test-fixture, no hazard: AppListings::refresh_archive_listings takes &PathBuf root
        (literal ASCII test path). Goes with: typing refresh_archive_listings to take &AppPath. No exemption.
    - `apps/desktop/src-tauri/src/native_drag/fulfillment.rs`
      - :338 appTextToPath, engine-body, HAZARD: volume-listed entry.name becomes ChildName, joined under native
        dest_path, written via std::fs. Goes with: AppName join for the source, native door (to_native) for the Finder
        dest. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/adb_index_test.rs`
      - :442 pathFromBareText, test-fixture, no hazard: IndexHost::apply_directory_change takes &Path parent (literal
        ASCII test path). Goes with: typing apply_directory_change's parent as &AppPath. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/copy_concurrency_bench.rs`
      - :392 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (bench-generated ASCII names).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
      - :449 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (bench-generated ASCII names).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :482 pathFromBareText, test-fixture, no hazard: copy_volumes_with_progress dest takes &Path (bench-generated
        ASCII names). Goes with: typing copy_volumes_with_progress's dest as &AppPath. No exemption.
      - :571 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (bench-generated ASCII names).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :585 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (bench-generated ASCII names).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :841 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (bench-generated ASCII names).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/network_archive_test_support.rs`
      - :338 pathFromBareText, test-fixture, no hazard: testing::compress_start sources are Vec<PathBuf> (literal ASCII
        test paths). Goes with: typing compress_start's sources as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/network_dates_test_support.rs`
      - :291 pathFromBareText, test-fixture, no hazard: test helper listed_date takes &Path (literal ASCII test name).
        Goes with: typing listed_date to take &AppPath. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/network_gated_source_test_support.rs`
      - :250 pathFromBareText, test-fixture, no hazard: PathBuf for parent arithmetic and the served-map key (literal
        ASCII test paths). Goes with: AppPathBuf parent and keying served by AppPathBuf. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/network_safety_test_support.rs`
      - :574 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (literal ASCII test paths).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/network_semantics_test_support.rs`
      - :757 pathFromBareText, test-fixture, no hazard: AppPath::legacy_unchecked bridge takes &Path (literal ASCII test
        name). Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :762 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (literal ASCII test name). Goes
        with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/network_transfer_test_support.rs`
      - :725 pathFromBareText, test-fixture, no hazard: start_copy sources are Vec<PathBuf> (literal ASCII test names).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/rename_merge_mtp_tests.rs`
      - :64 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (literal ASCII test path). Goes
        with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/s3_transfer_integration_test.rs`
      - :78 pathFromBareText, test-fixture, no hazard: test helper read_all takes &Path (literal ASCII test name). Goes
        with: typing read_all to take &AppPath. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_flags_test.rs`
      - :102 pathFromBareText, test-fixture, no hazard: run_copy sources are Vec<PathBuf> (literal ASCII test names).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
      - :244 pathFromBareText, test-fixture, no hazard: run_copy sources are Vec<PathBuf> (literal ASCII test names).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_full_concurrency_test.rs`
      - :233 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :262 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (generated ASCII test name).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
      - :459 pathFromBareText, test-fixture, no hazard: test helper hash_volume_file takes &Path (generated ASCII test
        path). Goes with: typing hash_volume_file to take &AppPath. No exemption.
      - :518 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (generated ASCII test name).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_outage_test.rs`
      - :947 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (literal ASCII test path). Goes
        with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_soak_test.rs`
      - :244 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :255 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :260 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (generated ASCII test path).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_stream_write_integration_test.rs`
      - :33 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :58 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :84 pathFromBareText, test-fixture, no hazard: test helper hash_volume_file takes &Path (generated ASCII test
        path). Goes with: typing hash_volume_file to take &AppPath. No exemption.
      - :108 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :124 pathFromBareText, test-fixture, no hazard: StagingTemp::mint takes &Path (literal test names, no
        escape-range code points). Goes with: typing StagingTemp::mint to take &AppPath. No exemption.
      - :141 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (literal test names, no
        escape-range code points). Goes with: building the AppPathBuf straight from the text (from_wire/parse). No
        exemption.
      - :148 pathFromBareText, test-fixture, no hazard: test helper hash_volume_file takes &Path (literal test names, no
        escape-range code points). Goes with: typing hash_volume_file to take &AppPath. No exemption.
      - :154 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :181 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :194 pathFromBareText, test-fixture, no hazard: StagingTemp::mint takes &Path (literal Greek test name, no
        escape-range code points). Goes with: typing StagingTemp::mint to take &AppPath. No exemption.
      - :212 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (literal Greek test name, no
        escape-range code points). Goes with: building the AppPathBuf straight from the text (from_wire/parse). No
        exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_stress_test.rs`
      - :71 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :87 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
      - :92 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (generated ASCII test path).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_transfer_semantics_test.rs`
      - :283 pathFromBareText, test-fixture, no hazard: copy source list is Vec<PathBuf> (generated ASCII test name).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
      - :385 pathFromBareText, test-fixture, no hazard: testing::app_path_of takes &Path (generated ASCII test path).
        Goes with: building the AppPathBuf straight from the text (from_wire/parse). No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/smb_xattrs_test.rs`
      - :109 pathFromBareText, test-fixture, no hazard: run_copy sources are Vec<PathBuf> (literal ASCII test names).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
      - :117 pathFromBareText, test-fixture, no hazard: test helper finder_xattrs takes &Path (literal ASCII test
        names). Goes with: typing finder_xattrs to take &str or &AppName. No exemption.
      - :143 pathFromBareText, test-fixture, no hazard: test helper finder_xattrs takes &Path (literal ASCII test
        names). Goes with: typing finder_xattrs to take &str or &AppName. No exemption.
      - :214 pathFromBareText, test-fixture, no hazard: run_copy sources are Vec<PathBuf> (literal ASCII test names).
        Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/backend_suites/webdav_wire_cost_test.rs`
      - :53 pathFromBareText, test-fixture, no hazard: webdav transfer helper sources are &[PathBuf] (literal ASCII test
        paths). Goes with: typing the copy engine's source list as Vec<AppPathBuf>. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/copy_snapshot_out_tests.rs`
      - :338 pathFromBareText, test-fixture, no hazard: WriteOperationError::SourcesSpanVolumes path String compared to
        a PathBuf (compare only, in a test). Goes with: typing SourcesSpanVolumes.path as AppPathBuf and comparing
        AppPaths. No exemption.
    - `apps/desktop/src-tauri/src/operations_host/restic_guard_tests.rs`
      - :342 pathFromBareText, test-fixture, no hazard: WriteOperationError::SourcesSpanVolumes path String compared to
        a PathBuf (compare only, in a test). Goes with: typing SourcesSpanVolumes.path as AppPathBuf and comparing
        AppPaths. No exemption.
    - `apps/desktop/src-tauri/src/tools/sensitive.rs`
      - :181 pathFromBareText, name-arith, no hazard: gate_on takes the tool's path as &str, asks std::path::is_absolute
        (is_absolute is a text check, nothing opened). Goes with: typing gate_on's path as &AppPath (AppPath
        absoluteness). No exemption.
      - :184 pathFromBareText, engine-body, HAZARD: tool path text goes to resolve_existing (std::fs::canonicalize); an
        escape misses the realpath check. Goes with: the native door: to_native on a typed AppPath before
        resolve_existing. No exemption.
    - `crates/cmdr-adb/src/server.rs`
      - :244 pathFromBareText, native, no hazard: ADB env var arrives as OsString (env var, OS text). Goes with:
        nothing: OsString into PathBuf is the native door. Exemption: yes, env var is native OS text.
    - `crates/cmdr-adb/src/server/binary_search.rs`
      - :101 pathFromBareText, native, no hazard: ANDROID_HOME-style env dirs arrive as OsString (env var, OS text).
        Goes with: nothing: OsString into PathBuf is the native door. Exemption: yes, env var is native OS text.
      - :130 pathFromBareText, native, no hazard: DirBase::Root search candidate is a native literal path (static
        search-table literal, native). Goes with: nothing: compile-time native literal. Exemption: yes, hardcoded native
        binary location.
    - `crates/cmdr-archive/src/apple_double.rs`
      - :206 pathFromBareText, native, no hazard: Xattr.name is OsString (xattr keys are OS names) (xattr key name, not
        a path). Goes with: nothing: xattr keys aren't app paths. Exemption: yes, xattr key name, not path text.
    - `crates/cmdr-archive/src/apple_double_test.rs`
      - :22 pathFromBareText, native, no hazard: Xattr.name is OsString (xattr key literal in a test). Goes with:
        nothing: xattr keys aren't app paths. Exemption: yes, xattr key name, not path text.
    - `crates/cmdr-archive/src/read/apple_double_index_test.rs`
      - :44 pathFromBareText, native, no hazard: Xattr.name is OsString (xattr key literal in a test). Goes with:
        nothing: xattr keys aren't app paths. Exemption: yes, xattr key name, not path text.
    - `crates/cmdr-archive/src/read/tar.rs`
      - :194 pathFromBareText, native, no hazard: Xattr.name is OsString (xattr key from a tar PAX record, not a path).
        Goes with: nothing: xattr keys aren't app paths. Exemption: yes, xattr key name, not path text.
    - `crates/cmdr-fs/src/apple_double_test.rs`
      - :8 pathFromBareText, name-arith, no hazard: is_sidecar_name takes &OsStr (native walk names) (literal test
        names, prefix check only). Goes with: an AppName twin of is_sidecar_name, or exempt the test. No exemption.
    - `crates/cmdr-fs/src/cloud_provider.rs`
      - :224 pathFromBareText, test-fixture, no hazard: cloud_provider::locate takes &Path (literal ASCII test path).
        Goes with: typing locate to take &AppPath. No exemption.
    - `crates/cmdr-fs/src/entry.rs`
      - :94 pathFromBareText, name-arith, no hazard: special_folders::icon_id_for_path takes &Path (compared to
        special-folder paths, nothing opened). Goes with: typing icon_id_for_path to take &AppPath. No exemption.
      - :103 pathFromBareText, name-arith, no hazard: Path::extension on the entry name (extension text only). Goes
        with: AppName extension helper. No exemption.
    - `crates/cmdr-fs/src/mac_only_files.rs`
      - :44 pathFromBareText, name-arith, no hazard: is_mac_only_file_name takes &OsStr (`._`/.DS_Store byte compare,
        nothing opened). Goes with: an AppName/&str twin of is_mac_only_file_name. No exemption.
    - `crates/cmdr-fs/src/mac_only_files_test.rs`
      - :8 pathFromBareText, name-arith, no hazard: is_mac_only_file_name takes &OsStr (literal test names). Goes with:
        an AppName/&str twin of is_mac_only_file_name. No exemption.
      - :23 pathFromBareText, name-arith, no hazard: is_mac_only_file_name takes &OsStr (literal test names). Goes with:
        an AppName/&str twin of is_mac_only_file_name. No exemption.
    - `crates/cmdr-fs/src/metadata/caps_tests.rs`
      - :60 pathFromBareText, native, no hazard: Xattr.name is OsString (xattr key literal in a test). Goes with:
        nothing: xattr keys aren't app paths. Exemption: yes, xattr key name, not path text.
    - `crates/cmdr-fs/src/sqlite_util.rs`
      - :543 pathFromBareText, native, no hazard: database_files appends -wal/-shm to an OsString (DB file path built
        from a native Path). Goes with: nothing: native DB file path. Exemption: yes, native DB file path arithmetic.
    - `crates/cmdr-fs/src/testing/disk_images/facts.rs`
      - :609 pathFromBareText, native, no hazard: AttachedImage.image_path is a native PathBuf (recorded hdiutil image
        path, native). Goes with: nothing: hdiutil output is native. Exemption: yes, hdiutil image path is native OS
        text.
      - :616 pathFromBareText, native, no hazard: SystemEntity.mount_point is a native PathBuf (recorded hdiutil mount
        point, native). Goes with: nothing: hdiutil output is native. Exemption: yes, hdiutil mount point is native OS
        text.
    - `crates/cmdr-fs/src/volume/conformance/listed_dates.rs`
      - :37 appTextToPath, test-fixture, no hazard: entry.path goes Path::new then legacy_unchecked into get_metadata
        (conformance support; an escaped name would fail the test, not touch user data). Goes with: typed FileEntry.path
        (AppPathBuf) passed straight to get_metadata. No exemption.
    - `crates/cmdr-fs/src/volume/conformance/modification_date.rs`
      - :150 appTextToPath, test-fixture, no hazard: entry.name compared to Path::file_name OsStr (compare only in
        conformance support). Goes with: typing the fixture dest as AppPath and comparing AppNames. No exemption.
    - `crates/cmdr-fs/src/volume/friendly_error/tests.rs`
      - :613 pathFromBareText, test-fixture, no hazard: listing_error_from_volume_error takes &Path (literal scheme test
        path). Goes with: typing listing_error_from_volume_error to take &AppPath. No exemption.
      - :644 pathFromBareText, test-fixture, no hazard: listing_error_from_volume_error takes &Path (literal scheme test
        path). Goes with: typing listing_error_from_volume_error to take &AppPath. No exemption.
      - :922 pathFromBareText, test-fixture, no hazard: listing_error_from_volume_error takes &Path (literal test path).
        Goes with: typing listing_error_from_volume_error to take &AppPath. No exemption.
    - `crates/cmdr-fs/src/volume/in_memory.rs`
      - :806 appTextToPath, engine-body, no hazard: InMemoryVolume entries map is keyed by PathBuf (in-memory map, never
        reaches an OS). Goes with: keying InMemoryVolume entries by AppPathBuf. No exemption.
    - `crates/cmdr-fs/src/volume/in_memory/device_tests.rs`
      - :44 pathFromBareText, test-fixture, no hazard: InMemoryVolume entries map is keyed by PathBuf (literal ASCII
        test path). Goes with: keying InMemoryVolume entries by AppPathBuf. No exemption.
    - `crates/cmdr-fs/src/volume/in_memory/imports_test.rs`
      - :23 pathFromBareText, test-fixture, no hazard: AppPath::legacy_unchecked bridge takes &Path (literal ASCII test
        path). Goes with: building the AppPathBuf straight from the text (parse). No exemption.
      - :29 pathFromBareText, test-fixture, no hazard: AppPath::legacy_unchecked bridge takes &Path (literal ASCII test
        path). Goes with: building the AppPathBuf straight from the text (parse). No exemption.
    - `crates/cmdr-fs/src/volume/in_memory/subtree_totals_test.rs`
      - :41 pathFromBareText, test-fixture, no hazard: InMemoryVolume::subtree_totals takes &[PathBuf] (literal ASCII
        test path). Goes with: typing subtree_totals to take &[AppPathBuf]. No exemption.
    - `crates/cmdr-fs/src/volume/in_memory_test.rs`
      - :802 pathFromBareText, test-fixture, no hazard: AppPath::legacy_unchecked bridge takes &Path (generated ASCII
        test path). Goes with: building the AppPathBuf straight from the text (parse). No exemption.
      - :851 pathFromBareText, test-fixture, no hazard: AppPath::legacy_unchecked bridge takes &Path (generated ASCII
        test path). Goes with: building the AppPathBuf straight from the text (parse). No exemption.
      - :1009 pathFromBareText, test-fixture, no hazard: InMemoryVolume::canonicalize takes &Path (literal ASCII test
        path). Goes with: typing canonicalize to take &AppPath. No exemption.
    - `crates/cmdr-fs/src/volume/path_helper_pin_test.rs`
      - :20 pathFromBareText, pin, no hazard: root_anchored takes &Path (pin test of root_anchored's std::path
        behavior). Goes with: goes with the bridge (milestone 11). No exemption.
      - :84 pathFromBareText, pin, no hazard: RemoteRoot::new takes &Path (pin test of RemoteRoot's std::path behavior).
        Goes with: goes with the bridge (milestone 11). No exemption.
      - :92 pathFromBareText, pin, no hazard: RemoteRoot::to_remote_path takes &Path (pin test of RemoteRoot's std::path
        behavior). Goes with: goes with the bridge (milestone 11). No exemption.
      - :231 pathFromBareText, pin, no hazard: ChildName::under takes &Path (pin test of ChildName::under (Path::join)).
        Goes with: goes with the bridge (milestone 11). No exemption.
      - :287 pathFromBareText, pin, no hazard: InMemoryVolume::canonicalize takes &Path (pin test of canonicalize's
        std::path behavior). Goes with: goes with the bridge (milestone 11). No exemption.
    - `crates/cmdr-fs/src/volume/path_order_pin_test.rs`
      - :23 pathFromBareText, pin, no hazard: pins BTreeSet<PathBuf> component-wise order (pin test of PathBuf
        ordering). Goes with: goes with the bridge (milestone 11). No exemption.
      - :48 pathFromBareText, pin, no hazard: pins PathBuf spelling-equality (pin test of PathBuf equality). Goes with:
        goes with the bridge (milestone 11). No exemption.
      - :54 pathFromBareText, pin, no hazard: pins PathBuf spelling-equality (pin test of PathBuf equality). Goes with:
        goes with the bridge (milestone 11). No exemption.
    - `crates/cmdr-fs/src/volume/remote_paths_test.rs`
      - :9 pathFromBareText, test-fixture, no hazard: RemoteRoot::new takes &Path (literal ASCII test path). Goes with:
        typing RemoteRoot::new to take &AppPath. No exemption.
    - `crates/cmdr-fs/src/volume/root_anchored_path_test.rs`
      - :8 pathFromBareText, test-fixture, no hazard: root_anchored takes &Path (literal ASCII test path). Goes with:
        typing root_anchored to take &AppPath. No exemption.
    - `crates/cmdr-fs/src/volume/server_side.rs`
      - :102 appTextToPath, engine-body, HAZARD: server-side walk queues child.path as PathBuf, legacy_unchecked
        re-escapes it for list_directory. Goes with: typing the walk's pending queue as AppPathBuf (child.path typed).
        No exemption.
    - `crates/cmdr-git/src/volume_tests.rs`
      - :487 appTextToPath, test-fixture, no hazard: portal row paths collected as Vec<PathBuf> for an engine call (git
        portal test paths, no escapes). Goes with: typing the called API's paths as AppPathBuf. No exemption.
    - `crates/cmdr-index/src/importance/classify.rs`
      - :80 pathFromBareText, name-arith, no hazard: file_name only; text out, nothing opened; leaf_name text helper via
        std::path::Path::file_name. Goes with: split on last `/` (AppPath/`AppName` or path_prefix helper). No
        exemption.
    - `crates/cmdr-index/src/importance/scheduler/recompute_tests.rs`
      - :195 pathFromBareText, name-arith, no hazard: test fixture text parent; nothing opened; test mkdir closure
        derives parent via Path::parent. Goes with: path_prefix::compute_parent_path or AppPath::parent. No exemption.
      - :202 pathFromBareText, name-arith, no hazard: test fixture text file_name; nothing opened; test mkdir closure
        derives row name via Path::file_name. Goes with: AppPath::file_name or a shared test row helper. No exemption.
    - `crates/cmdr-index/src/importance/scheduler/test_support.rs`
      - :157 pathFromBareText, name-arith, no hazard: test helper text parent; nothing opened; ensure_dir derives parent
        via Path::parent. Goes with: path_prefix::compute_parent_path or AppPath::parent. No exemption.
      - :164 pathFromBareText, name-arith, no hazard: test helper text file_name; nothing opened; ensure_dir derives row
        name via Path::file_name. Goes with: AppPath::file_name. No exemption.
      - :183 appTextToPath, name-arith, no hazard: fake home entry text parent; nothing opened; FileEntry.path parent
        via Path::parent in synthetic index builder. Goes with: typed FileEntry.path (AppPath::parent), 10f type gate.
        No exemption.
    - `crates/cmdr-index/src/importance/scheduler/walk.rs`
      - :539 pathFromBareText, name-arith, no hazard: extension only; escape never a `.`, nothing opened;
        lowercased_extension_into uses Path::extension on a row name. Goes with: str rsplit_once('.') extension helper
        on the name. No exemption.
    - `crates/cmdr-index/src/importance/scorer/types.rs`
      - :304 pathFromBareText, name-arith, no hazard: extension only; test/tooling, nothing opened; extension_count uses
        Path::extension on file names. Goes with: shared str extension helper. No exemption.
    - `crates/cmdr-index/src/indexing/lifecycle/cover/cold_drive_tests/moves.rs`
      - :230 pathFromBareText, test-fixture, no hazard: test compare of branch text against old root; nothing opened;
        Path::starts_with compares branch_paths text to a PathBuf root. Goes with: path_prefix::is_at_or_under on text.
        No exemption.
    - `crates/cmdr-index/src/indexing/lifecycle/cover/mod.rs`
      - :512 pathFromBareText, engine-body, HAZARD: stored frontier text opened by local cover walk; cover walk
        (ensure_walkable, local walker) takes &Path root. Goes with: route through app_text::local_path, made
        escape-decoding (to_native) at 10f. No exemption.
    - `crates/cmdr-index/src/indexing/lifecycle/manager/branch_watch.rs`
      - :206 pathFromBareText, engine-body, HAZARD: branch text becomes OS watch paths; BranchWatchStart.paths:
        Vec<PathBuf> for DriveWatcher start. Goes with: map via app_text::local_path (escape-aware) or typed branch
        paths. No exemption.
      - :237 pathFromBareText, name-arith, no hazard: compare against started PathBufs only; `paths` is Vec<PathBuf>,
        compared to branch text. Goes with: compare text keys, or retype paths with branch_watch.rs:206. No exemption.
      - :238 pathFromBareText, engine-body, HAZARD: branch text handed to inotify/FSEvents;
        DriveWatcher::watch_branch(&Path). Goes with: app_text::local_path (escape-aware) at the watcher seam. No
        exemption.
      - :321 pathFromBareText, engine-body, HAZARD: covered branch text handed to OS watcher;
        DriveWatcher::watch_branch(&Path). Goes with: app_text::local_path (escape-aware) at the watcher seam. No
        exemption.
    - `crates/cmdr-index/src/indexing/lifecycle/manager/start.rs`
      - :450 pathFromBareText, native, no hazard: env var CMDR_E2E_START_PATH is native text; DriveWatcher::start(&Path)
        watcher root from env. Goes with: std::env::var_os into PathBuf (a named env door). Exemption: yes, env var is
        native OS text; no app text involved.
    - `crates/cmdr-index/src/indexing/lifecycle/partial_agg.rs`
      - :101 pathFromBareText, test-fixture, no hazard: test literal paths, nothing opened; OpenListing.path is PathBuf.
        Goes with: OpenListing.path becomes AppPathBuf. No exemption.
    - `crates/cmdr-index/src/indexing/lifecycle/phased_bench.rs`
      - :323 pathFromBareText, native, no hazard: env var CMDR_PHASE_BENCH_ROOTS, native; macOS test bench;
        priority_roots() returns Vec<PathBuf> walked natively. Goes with: split_paths(var_os) via a named env door.
        Exemption: yes, env var is native OS text in an ignored bench.
      - :576 pathFromBareText, test-fixture, no hazard: ignored macOS-only bench, not shipped (would mis-walk an escaped
        name); cover_frontier_root(&Path) over index frontier text. Goes with: app_text::local_path (escape-aware), same
        door as cover/mod.rs:512. No exemption.
    - `crates/cmdr-index/src/indexing/lifecycle/phases/tests.rs`
      - :208 pathFromBareText, native, no hazard: env var CMDR_PHASES_TEST_TREE_DIR, native; tree_parent() PathBuf for
        tempdir_in. Goes with: var_os via a named env door. Exemption: yes, env var is native OS text.
    - `crates/cmdr-index/src/indexing/lifecycle/phases/tests/home_bench.rs`
      - :23 pathFromBareText, native, no hazard: env var CMDR_PHASE_HOME, native; ignored bench; home root PathBuf for a
        native walk. Goes with: var_os via a named env door. Exemption: yes, env var is native OS text.
    - `crates/cmdr-index/src/indexing/lifecycle/phases/tests/interleaving.rs`
      - :574 pathFromBareText, test-fixture, no hazard: ASCII tempdir fixture tree in a test; note_open_listing takes
        PathBuf (OpenListing.path). Goes with: OpenListing.path becomes AppPathBuf; AppPath parent/join. No exemption.
    - `crates/cmdr-index/src/indexing/lifecycle/phases/visits.rs`
      - :78 pathFromBareText, test-fixture, no hazard: test literal paths, nothing opened; OpenListing.path is PathBuf.
        Goes with: OpenListing.path becomes AppPathBuf. No exemption.
    - `crates/cmdr-index/src/indexing/network_scanner/cover_scan.rs`
      - :341 appTextToPath, engine-body, no hazard: share row text in a PathBuf; cmdr-search reads it as text, never
        opens it; pub CoveredEntry.path: PathBuf read by cmdr-search. Goes with: CoveredEntry.path becomes AppPathBuf
        (left for 10c). No exemption.
    - `crates/cmdr-index/src/indexing/read/coverage/tests.rs`
      - :713 pathFromBareText, native, no hazard: env var DB file path; ignored bench;
        IndexStore::open_write_connection(&Path). Goes with: var_os via a named env door. Exemption: yes, env var naming
        a DB file is native text.
    - `crates/cmdr-index/src/indexing/reconcile/local_reconcile/latency_probe.rs`
      - :305 pathFromBareText, test-fixture, no hazard: test literal names, nothing opened; LatencyProbe::record takes
        &Path. Goes with: record takes &str/AppPath key. No exemption.
    - `crates/cmdr-index/src/indexing/reconcile/reconciler/rescan/hold.rs`
      - :537 pathFromBareText, test-fixture, no hazard: test literal root, nothing opened; pending set is
        HashSet<PathBuf>. Goes with: pending_rescans keyed by text or AppPathBuf. No exemption.
      - :543 pathFromBareText, test-fixture, no hazard: test literal root, nothing opened;
        RescanThrottle::record_completion takes &Path. Goes with: throttle keyed by text or AppPath. No exemption.
      - :546 pathFromBareText, test-fixture, no hazard: test literal root, nothing opened; release_and_emit_completion
        takes &Path. Goes with: rescan seam keyed by text or AppPath. No exemption.
    - `crates/cmdr-index/src/indexing/reconcile/verifier.rs`
      - :516 pathFromBareText, engine-body, HAZARD: stored new-dir text opened by subtree scan;
        scanner::scan_subtree(&Path, ...). Goes with: app_text::local_path (escape-aware) or scan_subtree taking stored
        text. No exemption.
    - `crates/cmdr-index/src/indexing/scanner/exclusions.rs`
      - :122 pathFromBareText, engine-body, HAZARD: stored path's parent text stat'ed via symlink_metadata;
        has_pseudo_fs_trio does std::fs::symlink_metadata(Path::join). Goes with: app_text::local_path (escape-aware)
        before the stat. No exemption.
    - `crates/cmdr-index/src/indexing/scanner/walker/bulk_read.rs`
      - :634 pathFromBareText, native, no hazard: env var CMDR_BENCH_DIR or literal; ignored bench; bench walk takes
        &Path root. Goes with: var_os via a named env door. Exemption: yes, env var is native OS text in an ignored
        bench.
    - `crates/cmdr-index/src/indexing/scanner/walker/tests.rs`
      - :62 pathFromBareText, test-fixture, no hazard: test literals keying a mock fs, nothing opened; MockFs keyed by
        PathBuf (walker's DirTask.path). Goes with: test helper via a named fixture door (walker stays native). No
        exemption.
      - :185 pathFromBareText, test-fixture, no hazard: test literal root into mock walker; DirTask.path: PathBuf. Goes
        with: test helper via a named fixture door (walker stays native). No exemption.
    - `crates/cmdr-index/src/indexing/watch/event_loop/replay.rs`
      - :505 pathFromBareText, engine-body, HAZARD: deferred rescan text becomes an OS rescan root;
        Reconciler::route_must_scan_sub_dirs(PathBuf). Goes with: app_text::local_path (escape-aware) or route takes
        stored text. No exemption.
    - `crates/cmdr-index/src/indexing/watch/event_loop/replay_cost_bench.rs`
      - :58 pathFromBareText, native, no hazard: env var DB file path; bench; env_db() -> Option<PathBuf> for the DB.
        Goes with: var_os via a named env door. Exemption: yes, env var naming a DB file is native text.
    - `crates/cmdr-index/src/indexing/watch/event_loop/verification.rs`
      - :119 pathFromBareText, engine-body, HAZARD: stored new-dir text opened by subtree scan;
        scanner::scan_subtree(&Path, ...). Goes with: app_text::local_path (escape-aware) or scan_subtree taking stored
        text. No exemption.
    - `crates/cmdr-index/src/indexing/watch/watcher/fake_journal.rs`
      - :92 appTextToPath, test-fixture, no hazard: fake watcher compares event text to root; nothing opened;
        Path::starts_with on FsChangeEvent.path text. Goes with: path_prefix::is_at_or_under on text. No exemption.
    - `crates/cmdr-index/src/media_index/backend/fake.rs`
      - :240 pathFromBareText, name-arith, no hazard: fake backend stem of path text; nothing opened; stem() via
        Path::file_stem. Goes with: str stem helper on the name. No exemption.
    - `crates/cmdr-index/src/media_index/backend/vision/spike.rs`
      - :53 pathFromBareText, native, no hazard: env var CMDR_SPIKE_DIR / HOME, native; spike; collect_into walks a
        native PathBuf. Goes with: var_os via a named env door. Exemption: yes, env var is native OS text in a spike.
    - `crates/cmdr-index/src/media_index/predicate.rs`
      - :106 pathFromBareText, name-arith, no hazard: extension only; escape never a `.`, nothing opened; ext_of via
        Path::extension. Goes with: shared str extension helper. No exemption.
      - :114 pathFromBareText, name-arith, no hazard: stem only; nothing opened; stem_of via Path::file_stem. Goes with:
        shared str stem helper. No exemption.
    - `crates/cmdr-index/src/media_index/scheduler/enrich_tests.rs`
      - :43 pathFromBareText, name-arith, no hazard: test fixture text parent; nothing opened; ensure_dir derives parent
        via Path::parent. Goes with: path_prefix::compute_parent_path or AppPath::parent. No exemption.
      - :48 pathFromBareText, name-arith, no hazard: test fixture text file_name; nothing opened; ensure_dir derives row
        name via Path::file_name. Goes with: AppPath::file_name. No exemption.
    - `crates/cmdr-index/src/media_index/scheduler/kick_tests.rs`
      - :133 pathFromBareText, name-arith, no hazard: test fixture text parent; nothing opened; ensure_dir derives
        parent via Path::parent. Goes with: path_prefix::compute_parent_path or AppPath::parent. No exemption.
      - :138 pathFromBareText, name-arith, no hazard: test fixture text file_name; nothing opened; ensure_dir derives
        row name via Path::file_name. Goes with: AppPath::file_name. No exemption.
    - `crates/cmdr-listing-wasm/src/commands.rs`
      - :190 pathFromBareText, demo, no hazard: demo's engine takes home as Option<PathBuf> (wasm app_to_os is identity,
        no OS). Goes with: typing the engine's home as AppPathBuf. No exemption.
    - `crates/cmdr-listing-wasm/src/folder_sizes.rs`
      - :93 appTextToPath, demo, no hazard: demo folder-size memo keyed by PathBuf (wasm, no OS). Goes with: keying the
        memo by AppPathBuf. No exemption.
      - :98 appTextToPath, demo, no hazard: demo folder-size memo keyed by PathBuf (wasm, no OS). Goes with: keying the
        memo by AppPathBuf. No exemption.
      - :110 pathFromBareText, demo, no hazard: demo held_at takes &[PathBuf] (wasm, no OS). Goes with: typing
        held_at/dir_stats paths as AppPathBuf. No exemption.
      - :288 appTextToPath, demo, no hazard: demo held_at takes &[PathBuf] (wasm, no OS). Goes with: typing held_at
        paths as AppPathBuf. No exemption.
    - `crates/cmdr-listing-wasm/src/imports.rs`
      - :543 appTextToPath, demo, no hazard: InMemoryVolume import_folder/import_file take &Path (wasm, no OS). Goes
        with: typing InMemoryVolume's import helpers to take &AppPath. No exemption.
    - `crates/cmdr-listing-wasm/src/operations_host/commands/instant.rs`
      - :83 appTextToPath, demo, no hazard: engine check_rename_permission_for_volume takes PathBuf (wasm, no OS). Goes
        with: typing check_rename_permission_for_volume to take AppPathBuf. No exemption.
    - `crates/cmdr-listing-wasm/src/operations_host/commands/transfers.rs`
      - :154 pathFromBareText, demo, no hazard: engine home_folder_guard takes &[PathBuf] (wasm, no OS). Goes with:
        typing home_folder_guard's sources as AppPathBuf. No exemption.
      - :170 pathFromBareText, demo, no hazard: engine start_scan_preview takes Vec<PathBuf> (wasm, no OS). Goes with:
        typing start_scan_preview's sources as Vec<AppPathBuf>. No exemption.
    - `crates/cmdr-listing-wasm/src/search.rs`
      - :320 pathFromBareText, demo, no hazard: demo search folder map holds PathBuf (wasm, no OS). Goes with: keying
        the search folder map by AppPathBuf. No exemption.
    - `crates/cmdr-listing-wasm/src/tree.rs`
      - :139 appTextToPath, demo, no hazard: demo tree splits entry.path with Path::file_name (wasm, no OS; file_name
        text only). Goes with: AppPath::file_name on a typed path. No exemption.
      - :168 appTextToPath, demo, no hazard: InMemoryVolume::with_link_target takes &Path (wasm, no OS). Goes with:
        typing with_link_target to take &AppPath. No exemption.
    - `crates/cmdr-local/src/linux_mounts.rs`
      - :82 pathFromBareText, native, no hazard: mount_table_lists takes &str from to_string_lossy of a native Path
        (caller lossy-stringifies a native Path; compare to mount table). Goes with: taking &Path through
        is_mount_point, dropping the lossy &str hop. No exemption.
      - :200 pathFromBareText, native, no hazard: device_mounted_at takes &str from a native Path (caller
        lossy-stringifies a native Path; compare to mountinfo). Goes with: taking &Path through the caller, dropping the
        lossy &str hop. No exemption.
    - `crates/cmdr-local/src/mount_table.rs`
      - :130 pathFromBareText, native, no hazard: is_mount_point takes &str from to_string_lossy of a native Path
        (caller lossy-stringifies a native Path; compare to getfsstat). Goes with: taking &Path through
        mounts::is_mount_point, dropping the &str hop. No exemption.
    - `crates/cmdr-local/src/platform/local_posix/scan.rs`
      - :57 appTextToPath, engine-body, HAZARD: listed entry.name goes OsStr::new into is_sidecar, which
        symlink_metadata's the sibling. Goes with: the native door: entry's native name kept from the walk, or AppName
        to_native. No exemption.
    - `crates/cmdr-local/src/reading/bulk_test.rs`
      - :120 appTextToPath, name-arith, no hazard: entry.path text compared to a native PathBuf (compare in a test
        against a TestDir native join). Goes with: compare entry.path to AppPathBuf::from_native(dir.join(..)). No
        exemption.
    - `crates/cmdr-local/src/reading_parity_test.rs`
      - :279 appTextToPath, name-arith, no hazard: entry.path text compared to a native PathBuf (compare in a test
        against a native fixture join). Goes with: compare entry.path to AppPathBuf::from_native(fx.root.join(..)). No
        exemption.
    - `crates/cmdr-local/src/space.rs`
      - :81 pathFromBareText, native, no hazard: get_volume_space takes &str from a native Path's to_string_lossy
        (caller lossy-stringifies a native Path). Goes with: taking &Path through get_volume_space, dropping the lossy
        &str hop. No exemption.
    - `crates/cmdr-operations/src/archive_edit/fresh_compress_tests.rs`
      - :705 pathFromBareText, test-fixture, no hazard: literal selection; `compress_start` takes sources as
        `Vec<PathBuf>`. Goes with: typing `compress_start`'s sources as `AppPathBuf`. No exemption.
    - `crates/cmdr-operations/src/archive_edit/fresh_plan_tests.rs`
      - :402 pathFromBareText, test-fixture, no hazard: literal selection; `plan_sources_with_context` takes
        `&[PathBuf]`. Goes with: typing `plan_sources_with_context` sources as `&[AppPathBuf]`. No exemption.
    - `crates/cmdr-operations/src/archive_edit/remote_tests.rs`
      - :620 pathFromBareText, test-fixture, no hazard: literal dir; `sibling_names` takes `&Path`. Goes with: typing
        `sibling_names` to take `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/archive_edit/routing_tests.rs`
      - :66 pathFromBareText, test-fixture, no hazard: literal path; `ensure_zip_writable` takes `&Path`. Goes with:
        typing `ensure_zip_writable` to take `&AppPath`. No exemption.
      - :93 pathFromBareText, test-fixture, no hazard: literal path; `ensure_zip_writable` takes `&Path`. Goes with:
        typing `ensure_zip_writable` to take `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/conflict_preflight.rs`
      - :183 pathFromBareText, engine-body, HAZARD: IPC `source_paths: Vec<String>` to `PathBuf` for `seam_path`,
        `stat_source_paths(&[PathBuf])`. Goes with: taking `source_paths` as `Vec<AppPathBuf>` and typing
        `stat_source_paths`. No exemption.
    - `crates/cmdr-operations/src/conflict_preflight_tests.rs`
      - :434 pathFromBareText, name-arith, no hazard: test helper takes `file_name` of a literal. Goes with:
        `AppPath::file_name` in the `stat` helper. No exemption.
      - :440 pathFromBareText, test-fixture, no hazard: literal; stat map keyed by `PathBuf`
        (`stats_from_watched_panes`). Goes with: typing the stat map keys as `AppPathBuf`. No exemption.
    - `crates/cmdr-operations/src/create.rs`
      - :55 pathFromBareText, engine-body, HAZARD: IPC `parent_path: &str` to `Path` then `seam_path` (re-escapes) for
        the archive gate. Goes with: `create_*_managed` taking `AppPathBuf` parent. No exemption.
      - :70 pathFromBareText, engine-body, HAZARD: `parent_path` text joined as `Path`, `seam_path` into
        `route_serving`. Goes with: `AppPath` join on a typed parent. No exemption.
      - :199 pathFromBareText, engine-body, no hazard: listing-cache key only, nothing opened;
        `patch_listings_after_create` takes `&Path`. Goes with: typing `patch_listings_after_create` to take `&AppPath`.
        No exemption.
      - :276 pathFromBareText, engine-body, no hazard: listing-cache key only, nothing opened;
        `patch_listings_after_create` takes `&Path`. Goes with: typing `patch_listings_after_create` to take `&AppPath`.
        No exemption.
      - :300 pathFromBareText, engine-body, HAZARD: `archive_boundary_candidate(&Path)` splits text; archive path feeds
        the edit driver. Goes with: `archive_boundary_candidate_at(&AppPath)` on a typed parent. No exemption.
      - :414 pathFromBareText, engine-body, HAZARD: `cmdr_archive::path_crosses_archive_boundary(&Path)` stats via
        `std::fs`. Goes with: host `OperationVolumes::path_crosses_archive_boundary(&AppPath)`. No exemption.
      - :418 pathFromBareText, engine-body, HAZARD: `check_new_name`/`name_rules_at` take `&Path`; `probe_local` stats
        it. Goes with: typing `name_rules_at` to `&AppPath`, `to_native` at `probe_local`. No exemption.
      - :481 pathFromBareText, engine-body, no hazard: only `Display`ed into a `NotConnected` message;
        `unregistered_volume_refusal` takes `&Path`. Goes with: typing `unregistered_volume_refusal` to take `&AppPath`.
        No exemption.
    - `crates/cmdr-operations/src/create/tests.rs`
      - :372 pathFromBareText, test-fixture, no hazard: tempdir result; `create_directory_managed` returns path as
        `String`. Goes with: returning `AppPathBuf` and asserting via `to_native`. No exemption.
    - `crates/cmdr-operations/src/delete/already_gone_tests.rs`
      - :313 pathFromBareText, test-fixture, no hazard: literal; probe counter map keyed by `PathBuf`. Goes with: keying
        the probe map by `AppPathBuf`. No exemption.
      - :658 pathFromBareText, test-fixture, no hazard: literal; `RacingVolume.vanishes_after_listing: Option<PathBuf>`.
        Goes with: typing the `RacingVolume` fields as `AppPathBuf`. No exemption.
      - :659 pathFromBareText, test-fixture, no hazard: literal;
        `RacingVolume.delete_answers_not_found: Option<PathBuf>`. Goes with: typing the `RacingVolume` fields as
        `AppPathBuf`. No exemption.
      - :669 pathFromBareText, test-fixture, no hazard: literal through `legacy_unchecked(Path::new)` into
        `create_file`. Goes with: `AppPath::from_normalized` / `app_path!`. No exemption.
    - `crates/cmdr-operations/src/delete/cloud_trash.rs`
      - :303 pathFromBareText, test-fixture, no hazard: literal; facts table keyed `PathBuf` for `routing_with(&Path)`
        (local trash). Goes with: none while trash routing stays native local-disk. Exemption: yes, local-disk trash
        routing is native (`routing_with` over host paths).
      - :323 pathFromBareText, test-fixture, no hazard: literal; `routing_with` takes `&[PathBuf]` (local trash). Goes
        with: none while trash routing stays native local-disk. Exemption: yes, local-disk trash routing is native
        (`routing_with` over host paths).
    - `crates/cmdr-operations/src/delete/delete_outage_tests.rs`
      - :90 pathFromBareText, test-fixture, no hazard: literal; `delete_volume_files_with_progress_inner` takes
        `&[PathBuf]`. Goes with: typing the delete body's sources as `AppPathBuf`. No exemption.
      - :106 pathFromBareText, test-fixture, no hazard: literal through `legacy_unchecked(Path::new)` into `exists`.
        Goes with: `AppPath::from_normalized`. No exemption.
    - `crates/cmdr-operations/src/delete/preview_binding_tests.rs`
      - :41 pathFromBareText, test-fixture, no hazard: literal through `legacy_unchecked(Path::new)` into
        `open_read_stream`. Goes with: `AppPath::from_normalized`. No exemption.
    - `crates/cmdr-operations/src/delete/trash_tests.rs`
      - :363 pathFromBareText, test-fixture, no hazard: literal; `TrashItemError.path: PathBuf` (local trash). Goes
        with: none while trash stays a native local-disk record. Exemption: yes, `TrashItemError.path` names a
        local-disk trash item, native.
    - `crates/cmdr-operations/src/error_classification.rs`
      - :70 pathFromBareText, native, no hazard: text is a local `PathBuf`'s `display`, never an escape;
        `refusing_folder(&Path)` probes OS. Goes with: passing the native `&Path` into `classify_io_error` instead of
        `String`. No exemption.
    - `crates/cmdr-operations/src/in_flight_log_writer.rs`
      - :191 pathFromBareText, native, no hazard: the in-flight log file's own on-disk path plus a suffix. Goes with:
        none. Exemption: yes, the app's own log file on local disk, native by nature.
    - `crates/cmdr-operations/src/in_flight_records_format_tests.rs`
      - :14 pathFromBareText, test-fixture, no hazard: literal; `TrackedItem.path: PathBuf`. Goes with: typing
        `TrackedItem.path` as `AppPathBuf`. No exemption.
    - `crates/cmdr-operations/src/in_flight_sweep_tests.rs`
      - :661 pathFromBareText, test-fixture, no hazard: literal temp name; `TrackedItem.path: PathBuf`. Goes with:
        typing `TrackedItem.path` as `AppPathBuf`. No exemption.
    - `crates/cmdr-operations/src/in_flight_temps_tests.rs`
      - :228 pathFromBareText, test-fixture, no hazard: literal; `app(&Path)` helper bridges into `create_file`. Goes
        with: `AppPath::from_normalized` on the literal. No exemption.
    - `crates/cmdr-operations/src/journal_capture_volume_tests.rs`
      - :372 pathFromBareText, test-fixture, no hazard: literal; `InMemoryVolume::set_modified_at` takes `&Path`. Goes
        with: typing `set_modified_at` to take `&AppPath`. No exemption.
      - :715 pathFromBareText, test-fixture, no hazard: literal; `InMemoryVolume::set_modified_at` takes `&Path`. Goes
        with: typing `set_modified_at` to take `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/leaf_validation.rs`
      - :234 pathFromBareText, engine-body, HAZARD: chosen leaf joined onto `destination: &Path`; `stat` (`file_id`)
        opens it in `NamesAndInodes`. Goes with: typing leaves as `AppName` and `destination` as `&AppPath`. No
        exemption.
    - `crates/cmdr-operations/src/locked_sources_tests.rs`
      - :19 pathFromBareText, test-fixture, no hazard: literal; `LockedScan.locked: Vec<PathBuf>`. Goes with: typing
        `LockedScan`'s paths. No exemption.
      - :20 pathFromBareText, test-fixture, no hazard: literal; `LockedScan.system_locked: Vec<PathBuf>`. Goes with:
        typing `LockedScan`'s paths. No exemption.
    - `crates/cmdr-operations/src/rename/validity.rs`
      - :159 pathFromBareText, engine-body, HAZARD: IPC `dir: String` into `name_rules_at(&Path)`; `probe_local` stats
        it. Goes with: `check_rename_validity_impl` taking `AppPathBuf` dir. No exemption.
      - :172 pathFromBareText, engine-body, HAZARD: `new_path` to `validate_path_length` and
        `check_sibling_conflict_via_volume(&Path)`. Goes with: `AppPath` join on a typed dir; typing the sibling check.
        No exemption.
      - :185 pathFromBareText, engine-body, HAZARD: `old_path` to `check_sibling_conflict_via_volume`,
        `rename_by_move(&Path)`. Goes with: `AppPath` join on a typed dir; typing `rename_by_move`. No exemption.
    - `crates/cmdr-operations/src/rollback/planner_tests/test_support.rs`
      - :440 pathFromBareText, test-fixture, no hazard: literal; `InMemoryVolume::set_modified_at` takes `&Path`. Goes
        with: typing `set_modified_at` to take `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/routed_writes_tests.rs`
      - :247 pathFromBareText, test-fixture, no hazard: literal; `create_numbered_file` takes `&Path`. Goes with: typing
        `create_numbered_file` to take `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/scan_cache.rs`
      - :438 pathFromBareText, test-fixture, no hazard: literal; `FileInfo.path: PathBuf`. Goes with: typing
        `FileInfo.path`. No exemption.
      - :439 pathFromBareText, test-fixture, no hazard: literal; `FileInfo.source_root: PathBuf`. Goes with: typing
        `FileInfo.source_root`. No exemption.
    - `crates/cmdr-operations/src/scan_cache_tests.rs`
      - :41 pathFromBareText, test-fixture, no hazard: literal; scan cache sources are `Vec<PathBuf>`. Goes with: typing
        the scan cache's source paths. No exemption.
    - `crates/cmdr-operations/src/scan_failure_tests.rs`
      - :168 pathFromBareText, name-arith, no hazard: test compares error `path: String` with a `PathBuf`. Goes with:
        typed path in `WriteOperationError` (or compare `AppPath`). No exemption.
      - :200 pathFromBareText, name-arith, no hazard: test compares error `path: String` with a `PathBuf`. Goes with:
        typed path in `WriteOperationError` (or compare `AppPath`). No exemption.
      - :238 pathFromBareText, name-arith, no hazard: test compares error `path: String` with a `PathBuf`. Goes with:
        typed path in `WriteOperationError` (or compare `AppPath`). No exemption.
    - `crates/cmdr-operations/src/scan_preview/volume.rs`
      - :379 appTextToPath, name-arith, no hazard: `file_name` of `entry.path` as a map key; nothing opened. Goes with:
        `AppPath::file_name` on typed `FileEntry.path`. No exemption.
      - :612 appTextToPath, name-arith, no hazard: compares `file_name`s; nothing opened. Goes with:
        `AppPath::file_name` on typed `FileEntry.path`. No exemption.
    - `crates/cmdr-operations/src/scan_tests.rs`
      - :22 pathFromBareText, test-fixture, no hazard: literal; `FileInfo.path: PathBuf`. Goes with: typing
        `FileInfo.path`. No exemption.
      - :23 pathFromBareText, test-fixture, no hazard: literal; `FileInfo.source_root: PathBuf`. Goes with: typing
        `FileInfo.source_root`. No exemption.
    - `crates/cmdr-operations/src/scan_walker.rs`
      - :396 appTextToPath, name-arith, no hazard: `is_left_out(&OsStr)` filters by name; nothing opened. Goes with:
        `is_left_out` taking `&AppName`. No exemption.
      - :401 appTextToPath, engine-body, HAZARD: cached `entry.path` into `walk_dir_recursive(&Path)`, which
        `fs::read_dir`s it. Goes with: crossing typed `entry.path` with `to_native` (local walk). No exemption.
    - `crates/cmdr-operations/src/summary_tests.rs`
      - :6 pathFromBareText, test-fixture, no hazard: literal; summary takes `Vec<PathBuf>`. Goes with: typing the
        summary's paths. No exemption.
    - `crates/cmdr-operations/src/target_names.rs`
      - :37 pathFromBareText, name-arith, no hazard: `components` check that the name is one plain component. Goes with:
        `AppName` parse (`from_app_spelling`). No exemption.
      - :40 pathFromBareText, engine-body, HAZARD: IPC rename text stored as `OsString` leaf, later joined onto the
        destination. Goes with: `TargetNames` holding `AppNameBuf`. No exemption.
    - `crates/cmdr-operations/src/transfer/real_image.rs`
      - :620 pathFromBareText, test-fixture, no hazard: literal; `move_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `move_volumes_with_progress`'s sources. No exemption.
      - :675 pathFromBareText, test-fixture, no hazard: literal; `route_archive_copy_into` takes `Vec<PathBuf>`. Goes
        with: typing `route_archive_copy_into`'s sources. No exemption.
    - `crates/cmdr-operations/src/transfer/recovered_name.rs`
      - :216 pathFromBareText, name-arith, no hazard: compare only: error path text vs `PathBuf` landings. Goes with:
        typed error paths and `AppPathBuf` landings. No exemption.
    - `crates/cmdr-operations/src/transfer/transfer_driver_test_support.rs`
      - :24 pathFromBareText, test-fixture, no hazard: literal; transfer drivers take `Vec<PathBuf>`. Goes with: typing
        the transfer drivers' sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_cancel_tests.rs`
      - :57 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
      - :266 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
      - :348 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_concurrent_driver_tests.rs`
      - :787 pathFromBareText, test-fixture, no hazard: literal; `MisbehaveOnOpenVolume.misbehave_for: Option<PathBuf>`.
        Goes with: typing the test volume's field as `AppPathBuf`. No exemption.
      - :791 pathFromBareText, test-fixture, no hazard: literal; copy driver sources are `Vec<PathBuf>`. Goes with:
        typing `copy_volumes_with_progress`'s sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_crashsafe_tests.rs`
      - :438 appTextToPath, test-fixture, no hazard: in-memory listing of plain seeds; re-bridged into
        `open_read_stream`. Goes with: using typed `FileEntry.path` directly. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_failure_drain_tests.rs`
      - :53 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_flags_tests.rs`
      - :70 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
      - :91 pathFromBareText, test-fixture, no hazard: literal; `InMemoryVolume::set_carried_metadata` takes `&Path`.
        Goes with: typing `set_carried_metadata` to take `&AppPath`. No exemption.
      - :105 pathFromBareText, test-fixture, no hazard: literal; `InMemoryVolume::carried_metadata` takes `&Path`. Goes
        with: typing `carried_metadata` to take `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_landing_race_tests.rs`
      - :471 pathFromBareText, test-fixture, no hazard: literal; `parent` then `legacy_unchecked` into seeding. Goes
        with: `AppPath` parent on a `from_normalized` literal. No exemption.
      - :488 pathFromBareText, test-fixture, no hazard: literal; `parent` then `legacy_unchecked` into seeding. Goes
        with: `AppPath` parent on a `from_normalized` literal. No exemption.
      - :499 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_outage_tests.rs`
      - :56 pathFromBareText, test-fixture, no hazard: literal; `InMemoryVolume::set_modified_at` takes `&Path`. Goes
        with: typing `set_modified_at` to take `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_precheck_tests.rs`
      - :326 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
      - :353 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes dest `&Path`. Goes
        with: typing `copy_volumes_with_progress`'s destination. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_source_hint_tests.rs`
      - :40 pathFromBareText, test-fixture, no hazard: literal; `seed_incoherent_scan_result_for_test` takes
        `Vec<PathBuf>`. Goes with: typing the preview store's source paths. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_tree_order_tests.rs`
      - :115 pathFromBareText, test-fixture, no hazard: literal; transfer drivers take `Vec<PathBuf>`. Goes with: typing
        the transfer drivers' sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/copy_tree_rows_tests.rs`
      - :51 pathFromBareText, test-fixture, no hazard: literal; `parent` then `legacy_unchecked` into
        `create_directory_all`. Goes with: `AppPath` parent on a `from_normalized` literal. No exemption.
      - :71 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
      - :94 pathFromBareText, test-fixture, no hazard: literal; `parent` then `legacy_unchecked` into
        `create_directory_all`. Goes with: `AppPath` parent on a `from_normalized` literal. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/displaced_destination_tests.rs`
      - :124 appTextToPath, test-fixture, no hazard: in-memory listing of plain seeds collected as `PathBuf` for
        assertions. Goes with: collecting typed `FileEntry.path`. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/look_alike_tests.rs`
      - :93 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `copy_volumes_with_progress`'s sources. No exemption.
      - :101 pathFromBareText, test-fixture, no hazard: literal; `copy_volumes_with_progress` takes dest `&Path`. Goes
        with: typing `copy_volumes_with_progress`'s destination. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/merge.rs`
      - :986 appTextToPath, name-arith, no hazard: `DestinationNames::refuses(&OsStr)` checks name rules; nothing
        opened. Goes with: `refuses` taking `&AppName`. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/merge_window_tests.rs`
      - :282 pathFromBareText, test-fixture, no hazard: literal; transfer drivers take `Vec<PathBuf>`. Goes with: typing
        the transfer drivers' sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/move_by_copy_tests.rs`
      - :52 pathFromBareText, test-fixture, no hazard: literal join through `legacy_unchecked` into `create_directory`.
        Goes with: `AppPath` join / `app_path!`. No exemption.
      - :56 pathFromBareText, test-fixture, no hazard: literal join through `legacy_unchecked` into `create_file`. Goes
        with: `AppPath` join / `app_path!`. No exemption.
      - :60 pathFromBareText, test-fixture, no hazard: literal join through `legacy_unchecked` into `create_file`. Goes
        with: `AppPath` join / `app_path!`. No exemption.
      - :65 pathFromBareText, test-fixture, no hazard: literal join through `legacy_unchecked` into `create_file`. Goes
        with: `AppPath` join / `app_path!`. No exemption.
      - :89 pathFromBareText, test-fixture, no hazard: literal; `move_within_same_volume_with_progress` takes
        `&[PathBuf]`. Goes with: typing that body's sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/move_outage_tests.rs`
      - :111 pathFromBareText, test-fixture, no hazard: literal; `move_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `move_volumes_with_progress`'s sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/move_source_drift_tests.rs`
      - :117 pathFromBareText, test-fixture, no hazard: literal through `legacy_unchecked` into `get_metadata`. Goes
        with: `AppPath::from_normalized`. No exemption.
      - :168 pathFromBareText, test-fixture, no hazard: literal; `move_volumes_with_progress` takes `&[PathBuf]`. Goes
        with: typing `move_volumes_with_progress`'s sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/safety_grid_tests.rs`
      - :161 pathFromBareText, test-fixture, no hazard: literal; preview per-path map keyed by `PathBuf`. Goes with:
        typing the preview store's source paths. No exemption.
      - :196 pathFromBareText, test-fixture, no hazard: literal; `seed_incoherent_scan_result_for_test` takes
        `Vec<PathBuf>`. Goes with: typing the preview store's source paths. No exemption.
      - :777 pathFromBareText, test-fixture, no hazard: literal; transfer drivers take `Vec<PathBuf>`. Goes with: typing
        the transfer drivers' sources. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/safety_oracle.rs`
      - :45 pathFromBareText, test-fixture, no hazard: test oracle: literal through `legacy_unchecked` into
        `open_read_stream`. Goes with: `try_read_all` taking `&AppPath`. No exemption.
      - :63 pathFromBareText, test-fixture, no hazard: test oracle: listing text stack through `legacy_unchecked` into
        `list_directory`. Goes with: a typed `AppPathBuf` dir stack. No exemption.
      - :90 pathFromBareText, test-fixture, no hazard: test oracle: listing text stack through `legacy_unchecked` into
        `list_directory`. Goes with: a typed `AppPathBuf` dir stack. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/safe_replace_grid_tests.rs`
      - :350 pathFromBareText, test-fixture, no hazard: literal; `read_all` helper takes `&Path`. Goes with: typing
        `read_all` to take `&AppPath`. No exemption.
      - :365 appTextToPath, test-fixture, no hazard: in-memory listing of plain seeds; dir stack of `PathBuf`
        re-bridged. Goes with: a typed `AppPathBuf` dir stack. No exemption.
      - :670 appTextToPath, test-fixture, no hazard: in-memory listing of plain seeds collected as `PathBuf`. Goes with:
        collecting typed `FileEntry.path`. No exemption.
      - :864 appTextToPath, test-fixture, no hazard: in-memory listing of plain seeds collected as `PathBuf`. Goes with:
        collecting typed `FileEntry.path`. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/self_collision_tests.rs`
      - :428 pathFromBareText, test-fixture, no hazard: literal; `resolve_volume_conflict` takes source `&Path`. Goes
        with: typing `resolve_volume_conflict`'s paths. No exemption.
      - :430 pathFromBareText, test-fixture, no hazard: literal; `resolve_volume_conflict` takes dest `&Path`. Goes
        with: typing `resolve_volume_conflict`'s paths. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/strategy_abort_tests.rs`
      - :67 pathFromBareText, test-fixture, no hazard: literal; `copy_single_path` takes `&Path`. Goes with: typing
        `copy_single_path`'s paths. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/strategy_retry_tests.rs`
      - :61 pathFromBareText, test-fixture, no hazard: literal; `copy_single_path` takes dest `&Path`. Goes with: typing
        `copy_single_path`'s paths. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/strategy_test_support.rs`
      - :416 appTextToPath, test-fixture, no hazard: test volume's recursive delete re-bridges listed plain seeds. Goes
        with: using typed `FileEntry.path` directly. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/swap_outage_tests.rs`
      - :358 appTextToPath, test-fixture, no hazard: in-memory listing of plain seeds collected as `PathBuf`. Goes with:
        collecting typed `FileEntry.path`. No exemption.
      - :370 appTextToPath, test-fixture, no hazard: in-memory listing of plain seeds collected as `PathBuf`. Goes with:
        collecting typed `FileEntry.path`. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/transfer_error.rs`
      - :632 pathFromBareText, name-arith, no hazard: `over_every_name_limit(&Path)` counts the name's length; nothing
        opened. Goes with: `over_every_name_limit` taking `&AppPath`. No exemption.
    - `crates/cmdr-operations/src/transfer/volume/transfer_error_tests.rs`
      - :523 pathFromBareText, test-fixture, no hazard: literal; `PathedVolumeError::new` takes `&Path`. Goes with:
        typing `PathedVolumeError`'s path. No exemption.
      - :605 pathFromBareText, test-fixture, no hazard: literal; `hard_abort_error` takes `&Path`. Goes with: typing
        `hard_abort_error`'s path. No exemption.
    - `crates/cmdr-operations/src/transfer_tree/build_tests.rs`
      - :13 pathFromBareText, test-fixture, no hazard: literal; `TreeRecorder`/`sort_sources` take `PathBuf`s. Goes
        with: typing the transfer tree's paths. No exemption.
    - `crates/cmdr-operations/src/transfer_tree/report.rs`
      - :125 pathFromBareText, name-arith, no hazard: `node_at_path(&Path)` looks up the tree model; nothing opened.
        Goes with: typing the transfer tree's paths (`&AppPath` lookup). No exemption.
    - `crates/cmdr-operations/src/transfer_tree/view_tests.rs`
      - :176 pathFromBareText, test-fixture, no hazard: literal; `TreeRecorder::dir` takes `&Path`. Goes with: typing
        the transfer tree's paths. No exemption.
      - :208 pathFromBareText, test-fixture, no hazard: literal; `TreeRecorder::dir` takes `&Path`. Goes with: typing
        the transfer tree's paths. No exemption.
    - `crates/cmdr-operations/src/types/errors.rs`
      - :665 appTextToPath, name-arith, no hazard: compare only: error path text vs `from: &Path`. Goes with: typed path
        in `WriteOperationError`; `on_item_instead` on `&AppPath`. No exemption.
      - :687 pathFromBareText, engine-body, no hazard: error text from a native path's display; only the
        `/Volumes/<share>` root probed (`mounted_share_refused`). Goes with: passing the native `&Path` to
        `permission_denied` instead of text. No exemption.
    - `crates/cmdr-operations/src/types/transfer_leaves.rs`
      - :109 pathFromBareText, engine-body, HAZARD: chosen leaf `String` handed out as `OsStr`, joined onto the
        destination by the engine. Goes with: `TransferLeaves` holding `AppNameBuf`. No exemption.
    - `crates/cmdr-operations/src/validation_integration_test.rs`
      - :280 pathFromBareText, name-arith, no hazard: test compares error `path: String` with a tempdir `PathBuf`. Goes
        with: typed path in `WriteOperationError`. No exemption.
      - :300 pathFromBareText, name-arith, no hazard: test compares error `path: String` with a tempdir `PathBuf`. Goes
        with: typed path in `WriteOperationError`. No exemption.
    - `crates/cmdr-operations/src/wire_paths.rs`
      - :82 pathFromBareText, name-arith, no hazard: lexical trim, text in and text out; nothing opened
        (`text_without_trailing_separators`). Goes with: `from_wire` trimming a typed destination; drop the text
        variant. No exemption.
    - `crates/cmdr-operation-log/src/query/landed/tests.rs`
      - :33 pathFromBareText, name-arith, no hazard: test split helper uses Path::parent/file_name (parent/file_name
        text split in a test). Goes with: AppPath parent/file_name. No exemption.
      - :122 pathFromBareText, test-fixture, no hazard: landed_at takes &Path (literal scheme test path). Goes with:
        typing landed_at to take &AppPath. No exemption.
    - `crates/cmdr-restic/src/tree_tests.rs`
      - :22 pathFromBareText, test-fixture, no hazard: rustic_core Node::new_node takes &OsStr (literal ASCII test
        name). Goes with: nothing in our code: rustic_core's API is OsStr. Exemption: yes, rustic_core Node names are
        OsStr by upstream API.
    - `crates/cmdr-s3/src/volume/live_hostile_failure_test.rs`
      - :275 pathFromBareText, test-fixture, no hazard: S3 connect_with_state takes &Path root (generated ASCII scratch
        path). Goes with: typing connect_with_state's root as &AppPath. No exemption.
    - `crates/cmdr-sftp/src/volume/conformance_test.rs`
      - :54 pathFromBareText, test-fixture, no hazard: conformance::assert_listing_dates_are_claimed takes &Path
        (generated ASCII scratch path). Goes with: typing the conformance suite to take &AppPath. No exemption.
      - :262 pathFromBareText, test-fixture, no hazard: conformance::assert_a_link_lands_verbatim takes &Path (generated
        ASCII scratch path). Goes with: typing the conformance suite to take &AppPath. No exemption.
      - :293 pathFromBareText, test-fixture, no hazard: conformance::assert_writability_matches_the_mutations_offered
        takes &Path (generated ASCII scratch path). Goes with: typing the conformance suite to take &AppPath. No
        exemption.
      - :429 pathFromBareText, test-fixture, no hazard: conformance::assert_batch_scan_stops_when_told takes &Path
        (generated ASCII scratch path). Goes with: typing the conformance suite to take &AppPath. No exemption.
      - :447 pathFromBareText, test-fixture, no hazard: conformance::assert_batch_scan_asks_inside_the_walk takes &Path
        (generated ASCII scratch path). Goes with: typing the conformance suite to take &AppPath. No exemption.
    - `crates/cmdr-sftp/src/volume/testing.rs`
      - :197 pathFromBareText, native, no hazard: fixture key dir from CMDR_SFTP_KEYS_DIR env var (env var or
        compile-time default, native). Goes with: nothing: env var text is native (var_os would be tidier). Exemption:
        yes, env var is native OS text.
    - `crates/cmdr-smb/src/volume/copy_xattrs.rs`
      - :143 pathFromBareText, native, no hazard: Xattr.name is OsString (SMB stream name as xattr key, not a path).
        Goes with: nothing: xattr keys aren't app paths. Exemption: yes, xattr key name, not path text.
    - `crates/cmdr-smb/src/volume/test_support.rs`
      - :63 pathFromBareText, native, no hazard: SmbVolume mount_path is the native local mount point (literal native
        mount point in a test helper). Goes with: nothing: a mount point is native. Exemption: yes, SMB mount point is
        native OS path.
    - `crates/cmdr-smb/src/volume/watcher.rs`
      - :557 pathFromBareText, name-arith, no hazard: watcher splits share_path with Path::parent (parent text only, for
        a cache forget). Goes with: AppPath/share-path text parent helper. No exemption.

### 11. Close

- **Size**: S.
- **Scope**: first flip `cmdr_fs::volume::conformance` to app paths, taking every backend's `conformance_test.rs`
  crossing with it; then delete `legacy_unchecked` and `as_std_legacy` (S3's `live_hostile_test.rs` `dotted` cells, the
  last to hand a volume `..` through one, become pins that a typed path refuses `..` before any request); turn
  `app-path-std-bridge` into the permanent ban; add `app-path-native-crossing` with its allowlist; remove every
  remaining `#1225` ignore; update D1 and tasks 34â36 in the Windows plan, `crates/cmdr-fs/DETAILS.md` Â§ "App paths"
  (the durable home), `apps/desktop/src/lib/path/DETAILS.md` (a pointer to the Rust twin), and
  `docs/notes/windows-port/build-progress.md` with the builder's full run. Then wipe this spec per
  `docs/specs/DETAILS.md`.
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
text and assertion diffs; `D:\logs\{batch1,final,w2-index,w4-nextest2}â¦` and `D:\tmp\{ops,g2}-nextest.log`; milestone
1's baseline replaces these counts with names). The causes:

- **N, a native tempdir where an app path belongs** ("not an absolute Windows app path"): the test hands `cmdr-local`
  `C:\Users\â¦\Temp\.tmpX` (`list_directory_core`, a `LocalVolume` root or argument), or the index fixture roots its
  tree at `D:\src\cmdr\crates\cmdr-index\cmdr_â¦`. Round three's reading is confirmed, widened from "volume roots" to
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
  `/Volumes/â¦`): 8. Not 8: `cmdr-archive`'s two Unix-mode cells, `cmdr-listing`'s stall attribution (`Unknown` for
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
  plan tests (5) and `tools::sensitive` (1), which build POSIX home and XDG literals for the app's OWN dirs, native by
  Â§ "App path or native path"; `subprocess` (2, no `echo` program); one `HOME is set` cell. #1232 and the Windows build
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
   host's flavor, which refuses it, so only the in-memory volume and test code make one (Â§ "The type"). Rejected: a
   `mem://` root and rewriting 4,800 literals, which buys nothing the two fences don't, at the cost of every fixture.
2. **Non-UTF-8 names: decided, lossless, not lossy.** The premise was wrong: the local engine's walks and the index
   carry raw bytes today, so a lossy type would regress folder copies, and the lossy row is itself a bug that can act on
   a different file. Â§ "Names that aren't UTF-8". Rejected: keeping it lossy plus an issue.
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
   simple case folding) in every root match, and keep their case as typed or reported; `Eq` stays exact. Â§
   "Normalization", Case.
7. **The escape code points: decided, as proposed**, after review round 2 checked them against Unicode's rules and the
   prior schemes, and added the canonical form that makes `to_native` injective. Â§ "Names that aren't UTF-8".
8. **Rule 1's exceptions: decided, inside this effort**, each its own characterized commit with a before/after pin (loud
   rule 1). Review round 2 folded the typed-`..` resolver into the second.
9. **Today's live bugs: the non-UTF-8 family is #1236** (filed in review round 2, with the name-addressed protocols
   round two found). The trailing-`/` case (a wire path `/x/link/` stats and walks the link's target) has no issue: the
   frontend never sends one (rows are built by `join`), so only a raw IPC or MCP input can, and 10d fixes it. Its write
   half is already guarded at the engine's starts (`crates/cmdr-operations/src/wire_paths.rs`), which 10a absorbs.

Round two's three, decided by the lead (2026-11-18) and written in by review round 3:

10. **The trailing-`/` MCP case: decided, it rides milestone 10d**, with no separate issue.
11. **Cross-namer copies: decided, refusal only.** Offering a fallback name ("land it as `caf\u{FFFD}.txt`") stays out
    of this plan.
12. **Typed `..`: decided, a leading `..` follows the pane** (lexical, like the `..` row and `cd`); every other `..`
    follows the volume's OS. Review round 3 ran the Windows half: lexical is what Win32 does through junctions and
    directory symlinks (Â§ "Typed and pasted `..`").

Round three's two, decided by the lead (2026-11-18) and written in by review round 4:

13. **Per-folder case sensitivity on NTFS: decided, Windows plan task 35.** A folder WSL made (or one switched with
    `fsutil setCaseSensitiveInfo`) can hold `A` and `a`, which a volume-wide `NameRules` and the drive index's lowercase
    key would merge, and so would the conflict scan. The local Windows volume reports case sensitivity as a property PER
    DIRECTORY (`FileCaseSensitiveInformation`), and the index and the conflict scan ask it there. Not this plan: app
    paths stay exact, so nothing here makes it worse.
14. **`to_native_plain`'s refusal: decided, split by layer.** This plan owns the `NotPlainSafe` refusal TYPE (milestone
    3, with its reasons: trailing dot or space, reserved device name, length). The tasks that build its consumers
    (trash, open with, child processes, and the rest of Â§ "The native boundary"'s list) own the user-facing wording, so
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
  non-UTF-8 names go from "lossy, as today" to a lossless escape inside the type (Â§ "Names that aren't UTF-8", loud
  rule 2), and the bridge with it; `Eq`/`Hash` go from bytewise to component-wise so bridge values dedup as `PathBuf`
  did; new Â§ "Normalization" (`from_wire` refuses `..`, drops a trailing `/` as a deliberate pinned change) and Â§ "The
  native boundary" (typed refusals, `:` and the alternate-data-stream hazard, `/x` on Windows never reaching the OS,
  trailing dots and reserved names reachable but not creatable); the operation log gains drive and UNC roots (milestone
  8); S3 key collisions (milestone 6); the flip's diff audit, `app_path!` canonical-only rule, freeze, and revert path
  (milestone 4); pins (d)â(h) in milestone 1; the five review questions answered and four opened. Renumbered this
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
  show the shown form and refuse ambiguous U+FFFD input; new Â§ "The escape on the wire and in the frontend" (every
  stop, every persisted value decided); new Â§ "Typed and pasted `..`" (the volume answers `..`; round one kept
  go-to-path lexical and refused `..` elsewhere); UNC root folding written in (the lead's decision); the order: 2 first,
  the wave-B "no `pub` signature changes" rule, 10b free to run with wave B, `cmdr-multi-rename` and `cmdr-listing-wasm`
  moved to a new 10e after 10a, 10c before 10d, the frontend escape helpers moved from 9 to 3; milestone 4 gained a
  compiler-driven codemod, five agent tiers, a reproduction audit in place of the line-classifier, and a freeze shrunk
  to the landing window; milestone 1 gained pins (f)(7)â(8) and the Windows baseline list (i); new Â§ "Test strategy"
  (nets, gaps, every Windows cluster mapped to its milestone); questions 6â9 answered, 10â12 opened. Filed #1236 for
  the live non-UTF-8 bugs.
- **Round 3, Windows-native correctness on the real machine** (2026-11-18). A std-only Rust probe (`rustc`, no cargo)
  plus PowerShell on the builder, Windows 10 Pro 22H2 build 19045, NTFS (`D:` 8.3 off, `C:` 8.3 on), in a scratch
  folder, all removed after (shares, `subst`, and folders checked gone). Each claim, ran or not:
  - Ran: drive forms (`C:`, `C:foo`, `/x` â current drive, `\\?\D:` without `\`, lowercase drive, `/`, `.`, and `\\`
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
    cell); new Â§ "One file, several app paths" (the into-itself guard stays native and canonical, the loopback-UNC gap
    goes to task 35's identity); the Windows lexical `..` confirmed, with the verbatim reason it must happen in the app;
    NTFS cells for milestone 9; the constructor-root landmine and test-tempdir helper in milestone 9; two invariants.
    Questions 10â12 written in as the lead decided them; 13 (per-folder case) and 14 opened.
- **Round 4, the milestone mapping against the real failing tests** (2026-11-18). Read the builder's existing nextest
  logs, nothing rerun (the logs match the code this plan builds on: the app run at `df87f01096`, the others from the
  same day's legs): the app and `cmdr-nfs` (`D:\logs\w4-nextest2.log`), `cmdr-operations` (`D:\tmp\ops-nextest.log`),
  `cmdr-index` (`D:\w2-index.log`), `cmdr-restic`/`cmdr-listing-wasm`/`cmdr-multi-rename` (`D:\tmp\g2-nextest.log`), the
  backends (`D:\logs\batch1-nextest.log`), and `cmdr-listing` after its fixture fixes (`D:\logs\final-nextest.log`).
  Each failure's panic text and assertion diff were parsed out (the logs are PowerShell-wrapped, two UTF-16) and
  clustered; Â§ "Windows failures, mapped" holds the table. Confirmed: the 65 "not an absolute Windows app path" panics
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
