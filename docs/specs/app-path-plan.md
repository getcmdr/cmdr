# App path plan

App paths (`/`-separated and self-rooted: `/Users/x`, `C:/Users/x`, `//server/share/x`, `sftp://u@h:22/srv/x`,
`mtp://…`) travel through the Rust code as `std::path::Path`/`PathBuf`, so on Windows every `join`, `parent`,
`file_name`, `components`, `starts_with`, and `display` applies Windows rules: remote and virtual paths gain `\`, `a:b`
reads as a drive, and drive paths get reparsed. This plan replaces them with a typed app path that the `Volume` trait
takes, so a native path can't go where an app path belongs.

- Issue: [#1225](https://github.com/getcmdr/cmdr/issues/1225) (and the index's share of it,
  [#1232](https://github.com/getcmdr/cmdr/issues/1232)). Part of #942, the Windows port.
- Decided by David (2026-11-18): option (a) in #1225, a typed app path in the `Volume` signatures.
- Decided by the lead on David's delegation (2026-11-18): consumers that mean "this machine's local volume" name a
  neutral `cmdr_local::LocalVolume`; POSIX-specific tests keep `LocalPosixVolume`. The Windows drive index's name key
  stays lowercase with the Unicode form untouched.
- The path model this implements: D1 in `docs/specs/windows-port-plan.md`. The grammar it builds on:
  `crates/cmdr-fs/src/volume/path_grammar/mod.rs` and its case table `path_grammar_cases.json`.
- Shape of this doc: `docs/guides/multi-agent-refactors.md` § "The loop".

## Loud rules

1. **macOS and Linux behave byte-for-byte as before at every milestone.** Same listings, same paths on the wire, same
   rows in every store, same sort orders. The characterization suite (milestone 1) is the proof; ❌ never edit a pinned
   expectation to green a conversion. If a pin is wrong, stop and tell the lead.
2. **The wire doesn't change.** `FileEntry.path`, every IPC DTO, and `bindings.ts` stay `string`. `bindings.ts` is
   byte-identical after every milestone.
3. **No store migrations.** Persisted formats keep their exact serialized shape (§ "Persisted formats" proves none is
   needed). A milestone that finds it needs one stops and asks.
4. **❌ No `From<PathBuf>`, `From<&Path>`, `AsRef<Path>`, `AsRef<OsStr>`, or `Deref<Target = Path>` on the app path
   types, ever**, not even "temporarily". Construction goes through the named constructors in § "The type"; the one
   transition bridge is named, counted, and deleted at the end.
5. **❌ No `std::path` arithmetic on an app path.** Once a body holds an `&AppPath`, its `join`/`parent`/`file_name`/
   `starts_with` are the app path's own. Converting to `&Path` to do arithmetic is the bug this plan removes.
6. **Native paths cross only at named boundaries**: `AppPath::to_native` going down, `AppPathBuf::from_native` coming
   back, each call site in the `app-path-native-crossing` allowlist (milestone 11).
7. **Case sensitivity belongs to the volume, never the path.** App path equality, hashing, and `starts_with` are exact;
   a folded comparison takes the volume's `NameRules`.
8. **Each milestone lands on its own** with macOS and Linux CI green, through the lead's `pnpm land`. Windows progress
   is measured on the builder (`ssh mami`) per milestone, with the test lists named below.
9. Strip milestone numbers from code, comments, test names, and `reason` strings before landing (the guide's rule). This
   file keeps its own.

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
  plus names.
- **Recents, favorites, settings**: JSON strings (`favorites/store.rs` `path: String`, deduped by
  `normalize_for_dedup`).
- **In-flight records ledger** (`cmdr-operations` `in_flight_records.rs`, JSON, `PathBuf` fields with derived `Ord`):
  local temps the local engine owns and sweeps.
- Why none needs a migration: (1) on macOS and Linux an app path's spelling is the exact string these stores hold today,
  for every path the current code writes (milestone 1 pins "parse then print is the identity" over each store's write
  path); (2) no Windows install has any data yet; (3) stores keep their `String` fields, and the ledger keeps native
  `PathBuf`s (it's the local engine's own file bookkeeping, swept with `std::fs`).

### Where native paths legitimately live

`cmdr-local` (inside both volumes; `local_windows/paths.rs` is today's one Windows crossing), `cmdr-operations`
`local_disk/` and the local copy strategies, `cmdr-index`'s local walker and watch ingestion, `fsevent-stream`,
`cmdr-update-trust`, the app's own-files modules listed above, and OS integrations that hand a user's file to the OS
(drag, reveal, open with, Quick Look, the services menu, eject holders).

### Other facts that shape the design

- **Non-UTF-8 names are lossy today** past `cmdr-local`'s reader: `FileEntry.name` and `.path` are lossy `String`s
  (`reading_parity_test::a_non_utf8_name_lists_lossily` on Linux), `ChildName`, `DestNameIndex`, and `item_identity`
  work on the lossy or UTF-8 form, and fix `fdfa5b58cb` made the reader `lstat` through the raw `OsStr` only so the ROW
  is complete. So a UTF-8 app path loses nothing that isn't already lost.
- **MTP pick segments** (`twin_path.rs`): a row path can carry a component of NUL plus an id (`/DCIM/␀4242/IMG_1.JPG`).
  The app path must accept it, `parent` must keep today's answer (the pick), and `twin_path::folder_of` and `shown` stay
  the pick-aware layer on top.
- **`root_anchored`** (`volume/mod.rs` ~2441) accepts relative paths and the root aliases `""`, `.`, `/`, and joins
  anything off-root onto the root. `RemoteRoot::to_remote_path` accepts relative paths too. So relative paths DO reach
  `Volume` methods today (from backends' own code and tests).
- **Ordering**: `PathBuf`'s `Ord` is component-wise (`/a/b` < `/a-b`), a string's is bytewise (`/a-b` < `/a/b`). Six
  `BTreeMap`/`BTreeSet`s key on `PathBuf` (`cmdr-index` `reconciler.rs`, `cmdr-listing-wasm` `folder_sizes.rs`, tests),
  `in_flight_records` derives `Ord`, and 233 `HashMap`/`HashSet`s key on `PathBuf`.
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

- `/` (POSIX shape). Accepted on every OS: it's the in-memory volume's and the test fixtures' root, and a local volume
  that can't open it refuses it at `to_native` (Windows' does today). Whether this is right is review question 1.
- `C:/` (drive, letter uppercased) and `//server/share/` (UNC, one root): parsed only under `PathFlavor::Windows`. Under
  `Posix`, `//server/share/x` normalizes to `/server/share/x` and `C:/x` is no app path, as the grammar does today.
- `scheme://authority/` (any RFC 3986 scheme): the root is `scheme://authority`, so `parent` clamps there. Where a
  remote VOLUME is rooted (`sftp://u@h:22/srv/data`) is deeper and stays `RemoteRoot`'s business.

A name may hold any character but `/` (and NUL, except as the first character of an MTP pick segment). `\` and `:` are
ordinary in a name: a Linux SFTP server can list `a\b`, and `a:b` must never read as a drive. Drive and UNC paths refuse
`\` anywhere, as the grammar does, since NTFS can't hold one.

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
- `to_native(&self) -> Option<PathBuf>` (`None` for a scheme path, and for a shape the host can't open) and
  `AppPathBuf::from_native(&Path, PathFlavor) -> Option<AppPathBuf>`: today's `path_grammar` bodies, moved.
- `Display`: the wire spelling (for logs, wrapped in `LogPrivate` as today). `shown()`: what a person reads, the ONE
  formatter (Windows `\`, picks taken out through `twin_path::shown`), the twin of the frontend's `shownPath`.
- `Eq`, `Hash`: bytewise (the invariant makes that exact). `Ord`: **component-wise, matching `PathBuf`**, so every
  sorted collection keeps its order (rule 1).

**Construction**, each named and greppable, ❌ no `From` impls:

- `AppPathBuf::parse(text: &str, flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath>`: normalizes (lexical `..`
  clamped at the root, as `remote_paths.rs` and the grammar do).
- `AppPathBuf::from_wire(text: &str)`: `parse` with `PathFlavor::HOST`, for IPC arguments and stored strings.
- `AppPath::from_normalized(text: &str) -> Result<&AppPath, NotAnAppPath>`: zero-copy validation for a string that is
  already canonical (a `FileEntry.path` a backend built); `FileEntry::app_path()` uses it.
- `AppPathBuf::from_native(...)`, `RemoteRoot::to_app_path(...)`, `Volume::root()`, and the derivations above.
- Tests only (`any(test, feature = "testing")`): `app_path!("/a/b")` returns a `&'static AppPath`-like value, panicking
  on a bad literal, so the 4,800 test literals convert by sed.
- **The transition bridge, deleted in milestone 11**: `AppPath::legacy_unchecked(&Path) -> Cow<'_, AppPath>` (wraps the
  UTF-8 text as is, unvalidated; lossy owned for non-UTF-8, which is what the wire does today) and
  `AppPath::as_std_legacy(&self) -> &Path`. Both exist so the trait can flip without a semantic change anywhere; each
  call is counted by the `app-path-std-bridge` check and goes to zero crate by crate.

**Placement**: `cmdr_fs::app_path` (new module), with `path_grammar` becoming its parser and keeping the frontend twin's
case table. The current `path_grammar::AppPath` struct (`AppRoot` plus `Vec<String>`) folds into the new type;
`AppRoot`, `PathFlavor`, and `PathKind` stay. `ChildName::under(&AppPath) -> AppPathBuf` replaces today's
`under(&Path)`.

### Design decisions and what each rejected

- **Owned and borrowed pair over a `str` (`AppPathBuf`/`&AppPath`).** Rejected: the current `AppRoot` + `Vec<String>`
  struct (an allocation per name, no borrowed form, can't borrow a `FileEntry.path`); a single owned type (every
  `Volume` call would clone, and `parent` couldn't be a slice).
- **UTF-8.** Rejected: an `OsStr`-backed type for lossless non-UTF-8 Linux names. The wire and every engine past the
  reader are already lossy (survey), WTF-8 on Windows would leak into arithmetic, and specta/serde want a string. A
  lossless story (an escape spelling for non-UTF-8 bytes) is a wire change of its own; out of scope, file an issue if
  wanted (review question 2).
- **Our own type, not a crate.** Rejected: `camino::Utf8Path` (UTF-8, but std's Windows rules on Windows: the very bug)
  and `typed_path`'s Unix paths (POSIX rules everywhere, but `C:/x`, `//server/share`, and `sftp://h/x` read as
  relative, so `parent` and `starts_with` don't clamp at the root). Neither knows our roots or pick segments.
- **Named constructors only.** Rejected: `From<PathBuf>`/`AsRef<Path>` (makes "unrepresentable" false the day it lands)
  and a lint-only rule over `&Path` (#1225 option b, which David turned down).
- **Path-shaped method names are NOT mimicked.** Rejected: giving `AppPath` a `join(&str)` with `Path`'s semantics so
  bodies compile unchanged. `Path::join` with an absolute argument replaces the base and keeps `..`; an app path that
  silently did otherwise would change macOS behavior in bodies nobody reread. The flip keeps every body on the bridge
  instead, and each crate's milestone converts arithmetic deliberately, under its pins.
- **Component-wise `Ord`.** Rejected: bytewise (cheaper, but reorders the six `BTree*`s, the ledger's derived `Ord`, and
  every `sort()` of paths).
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

Order and parallelism:

- **Wave A, three parallel worktrees**: 1 (pins), 2 (`LocalVolume`), 3 (the type). Disjoint files except a few test
  helpers; 2 touches 262 files, so it lands first if the others conflict.
- **Then 4 alone** (the flip). Nothing else touching Rust in the crates may be in flight: it edits 183 impls and every
  path-taking call.
- **Then 5 alone** (`cmdr-fs` internals): every backend builds on `RemoteRoot` and `root_anchored`.
- **Wave B, four parallel worktrees**: 6 (network backends), 7 (device backends), 8 (virtual volumes and leaf crates), 9
  (`cmdr-local`). Disjoint crates.
- **Wave C, two parallel worktrees**: 10a (`cmdr-operations`, needs 9) and 10b (`cmdr-index`, needs 9).
- **Then 10c and 10d in parallel** (the app, split by directory; need 10a and 10b), **then 11** (close).

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
  Ordering pins: a `BTreeSet<PathBuf>` of `/a/b`, `/a-b`, `/a`, `/a/b/c` iterates in today's order. (e) For each store
  in § "Persisted formats", a round-trip pin: the strings the current write path stores read back to the same values (on
  POSIX this proves "parse then print is the identity" over real writers).
- **Intentions**: make the flip and every crate conversion provably byte-identical on macOS and Linux.
- **Landmines**: pin reality, not the plan. If today's behavior looks wrong (a relative path anchored oddly), pin it
  anyway, add a comment, and tell the lead; fixing it is a separate change. Verify each new pin bites (break the helper
  locally, see red).
- **Test plan**: the new tests pass on macOS and Linux CI. Windows: the grammar table's Windows rows pass against
  today's `path_grammar` where it already answers (parse, parent, basename, shown, to/from native).
- **DONE**: every helper above has a pin, the case table has every shape, the alloc ceilings and ordering pins exist,
  and `pnpm check --fast` plus the touched crates' tests are green.

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

### 3. The type

- **Size**: M.
- **Scope**: `cmdr_fs::app_path` per § "The type": `AppPath`, `AppPathBuf`, `RelativeAppPath`, `NotAnAppPath`, the
  operations, constructors, `Serialize` and `specta::Type` as `string`, `Deserialize` through `from_wire` (for
  in-process formats only; persisted DTOs keep `String`), `app_path!`, the transition bridge, and the
  `app-path-std-bridge` check with an empty allowlist. Fold `path_grammar::AppPath` into it (its 15 callers move);
  `ChildName::under` gains the `&AppPath` form beside the old one. Docs: `crates/cmdr-fs/DETAILS.md` gets an "App paths"
  section (the canonical home for this design once the plan is wiped), `CLAUDE.md` a one-line guardrail.
- **Intentions**: the whole vocabulary exists and is tested before anything uses it.
- **Landmines**: `Ord` is component-wise (§ "Design decisions"). A pick segment (`␀4242`) is a valid name. Under
  `PathFlavor::Posix` `\` and `:` are name characters. `from_normalized` must reject every non-canonical spelling
  (property test: `from_normalized(s).is_ok()` iff `parse(s) == s`). Keep it `wasm32`-clean (no `std::fs`).
- **Test plan**: the case table from milestone 1 runs against the new type for BOTH flavors on every OS; property tests
  for normalize idempotence, `parent`/`join` inverse, `starts_with`/`strip_prefix` agreement, and `Ord` equal to
  `PathBuf`'s on POSIX-shaped inputs. Windows builder: `cargo nextest run -p cmdr-fs` green with the new module.
- **DONE**: the module is complete, documented, and unused outside tests, `path_grammar` delegates to it, and the
  frontend's grammar suite still passes the shared table.

### 4. Flip the trait

- **Size**: L (mechanical, one branch, every crate; a lead with a subagent per crate group, sequential).
- **Scope**: every path-carrying `Volume` method takes `&AppPath` (`&[AppPathBuf]` for batches) and returns
  `AppPathBuf`/`&AppPath`, except link targets (§ "Design decisions"). The path-carrying types beside the trait
  (`CopyScanResult`, `BatchScanResult`, `routed.rs`, `root_echo.rs`, `mkdir_all.rs`) follow. All 183 impls change
  signature and open each overridden body with `let path = path.as_std_legacy();` (or the pair), so no body's arithmetic
  changes. Callers pass `&AppPath`: test literals via `app_path!` (a sed), runtime values via
  `AppPath::legacy_unchecked(&p)` where they hold a `PathBuf`, or the value itself where it already came from a volume.
  The `app-path-std-bridge` allowlist records the counts.
- **Intentions**: the trait is typed from here on, so no new code can pass a native path, and every later milestone is a
  crate-local, parallel conversion with a visible counter. Zero behavior change on any OS.
- **Landmines**: this is the one flag day, and it's unavoidable: `dyn Volume` has 183 impls and 61 path-carrying
  methods. Rejected alternative: a parallel `&AppPath` method beside each `&Path` one with defaults calling each other
  (doubles a 120-method trait, and a pair of defaults that call each other recurses forever when an impl overrides
  neither). Keep it small: no body logic edits, no renames, no drive-by fixes. `legacy_unchecked` must not validate or
  normalize (that would change POSIX behavior for `..` and trailing slashes). Rebase-friendly: land it fast, ask the
  lead to hold other Rust branches in these crates.
- **Test plan**: the full Rust suite on macOS and Linux via `pnpm land` CI; milestone 1's pins are the proof. Windows:
  build and clippy clean; test counts unchanged from before (no fixes expected yet).
- **DONE**: no `Volume` method names `Path` except link targets; every impl compiles on all three OSes; bridge counts
  recorded per file.

### 5. `cmdr-fs` internals

- **Size**: M.
- **Scope**: `RemoteRoot` holds `AppPathBuf`s and does `AppPath` arithmetic (`to_remote_path(&AppPath)`,
  `to_app_path -> Option<AppPathBuf>`); `normalize_remote_path` stops using `Path::components` (server paths are POSIX,
  so a `/`-only normalizer); `app_paths.rs` folds into `AppPath` methods; `root_anchored` takes a wire string and
  returns `AppPathBuf` (callers with a typed path drop it); `scan_walk`, `patching`, `ids`, `twin_path`, `child_name`,
  `in_memory` (root, overrides, link resolution) convert. All `cmdr-fs` bridges go.
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
  hold `//`. Keep the key layer string-based and convert only at the app-path edge. WebDAV `href`s are percent-encoded;
  decode before `to_app_path`, never after.
- **Test plan**: Windows: `cmdr-sftp` 7 (`paths_test`), `cmdr-webdav` 23, `cmdr-s3` 43 plus the 5 pause timeouts,
  `cmdr-nfs` 49 (including the 7 `ExportNotFound` fake mounts). macOS/Linux: each crate's suite plus its Docker fixture
  lane where CI runs one.
- **DONE**: the four crates' bridge counts are zero and their Windows suites green.

### 7. Device backends: SMB, MTP, ADB

- **Size**: M.
- **Scope**: as 6. `cmdr-smb`'s `to_smb_path` and anchored mounts; `cmdr-mtp`'s storage paths and pick segments;
  `cmdr-adb`'s device paths.
- **Landmines**: SMB maps `\` and other forbidden characters into the private-use area in `smb2`; an app path name with
  `\` must reach that mapping intact. MTP picks (`twin_path`) are the one NUL-bearing name. An SMB share mounted by the
  OS on macOS (`/Volumes/x`) is a POSIX-shaped app path through `LocalVolume`, not `smb://`.
- **Test plan**: Windows: `cmdr-adb` 52, `cmdr-smb` 13, `cmdr-mtp` 2.
- **DONE**: bridge counts zero; Windows suites green.

### 8. Virtual volumes and leaf crates

- **Size**: M.
- **Scope**: `cmdr-archive` (boundary detection, URL-rooted entry paths; the local backing file crosses at `to_native`),
  `cmdr-git` (portal paths app-side, `git2` native), `cmdr-restic` (hub and volume; fixtures stop rooting at
  `/Volumes/…` as native), `cmdr-viewer` (markdown link and image resolution as `AppPath` arithmetic; opening a local
  file crosses at `to_native`), `cmdr-listing`, `cmdr-listing-wasm`, `cmdr-operation-log` (scheme-rooted dir prefixes),
  `cmdr-multi-rename`, `cmdr-search` (its two `Path` lines).
- **Landmines**: `cmdr-listing-wasm` builds for `wasm32`. The operation log's dir tree must keep
  `reconstruct_dir_path`'s exact output (pinned). Restic snapshot paths are POSIX paths from the repository, whatever OS
  made the backup.
- **Test plan**: Windows: `cmdr-archive` 5, `cmdr-git` 1 (un-ignore), `cmdr-viewer` 13, `cmdr-listing` 2 (one is
  `foreign_path_test`; the stall-attribution one is not #1225 and may stay), `cmdr-operation-log` 3, and the 61 failures
  plus 10 timeouts over `cmdr-restic`, `cmdr-listing-wasm`, and `cmdr-multi-rename`.
- **DONE**: bridge counts zero in these crates; the named Windows clusters green.

### 9. `cmdr-local`

- **Size**: S.
- **Scope**: both volumes take `&AppPath` natively: `resolve` and `local_windows/paths.rs` collapse into
  `AppPath::to_native` (the Windows `resolve_under` goes, since callers no longer pass relative paths); `reading.rs`
  builds `FileEntry.path` with one `AppPath::join`; `rename_local_exclusive` takes `&AppPath` and crosses inside. Watch
  events coming back from the OS go through `from_native`.
- **Intentions**: the one local crossing per OS, and the listing hot path at one allocation per entry.
- **Landmines**: a non-UTF-8 name on Linux: stat through the raw `OsStr` (fix `fdfa5b58cb`), list with the lossy path,
  exactly as `reading_parity_test` pins. The two listers stay field-for-field identical. Run the alloc ceilings and
  benches (§ "Cost").
- **Test plan**: Windows: `cmdr-local` 30 of 30 plus the conformance cells that only waited on paths. macOS/Linux:
  `reading_parity_test`, the conformance suite, alloc ceilings lowered.
- **DONE**: `cmdr-local` has no bridges, and `rename_local_exclusive` accepts an app path on Windows.

### 10a. `cmdr-operations`

- **Size**: L (1,910 prod and 4,557 test `Path` lines, 108 test volumes).
- **Scope**: the volume engine and the local engine take `AppPath`; the local engine's fast paths (`local_disk/`, the
  copy strategies, trash, cloud trash, the move sweep, validation) cross at `to_native` per OS call. The in-flight
  ledger keeps native `PathBuf`s. Un-gate the `#![cfg(unix)]` test files whose only POSIX-ism was `LocalPosixVolume`
  (milestone 2's list).
- **Landmines**: "EVERY local write lands via `overwrite::stage_and_land_file`": the landing's paths become app paths,
  the temp's stays the engine's. `DestNameIndex` and `item_identity` compare names, not paths: keep them on `&str`.
  Rollback planner and journal rows carry app paths into the operation log (pinned). Big enough to split by subagent
  (transfer, delete and trash, archive edit, rename and create, scans), sequential on one branch.
- **Test plan**: Windows: the 289 failures' path clusters (the finalize's native `C:\…` refusal is the largest; the
  in-memory and remote-fixture ones next). Not in scope: the `MountedVolumeInside` refusals (task 35's mount table and
  file id) and the in-flight ledger lock (#1226's family). macOS/Linux: full suite, `transfer_benchmarks` before/after.
- **DONE**: bridge count zero in `cmdr-operations`; a local copy and move land on the builder's NTFS.

### 10b. `cmdr-index`

- **Size**: M.
- **Scope**: the local scanner, reconciler, and watcher ingest native paths through `from_native` at the walk and event
  edges and hold app paths after; `store/paths.rs` resolves `&AppPath` by names; the exclusion policy's `/`-prefixed
  rules become app-path rules; media and importance stores keep their `TEXT` columns.
- **Landmines**: the drive index's key decision stands (lowercase, Unicode form untouched). The cold-drive fixture roots
  a tempdir; on Windows that's a drive app path, so the index root is that path, not `/`. Some of that may need task
  34's drive volumes: if so, say which tests and stop there.
- **Test plan**: Windows, #1232's clusters: `reconcile::*` (about 25), `lifecycle::phases::tests` (about 12),
  `scanner::convergence_tests`, `scanner::policy_tests`, `store::tests::path_resolution`, `watch::branches` and
  `watch::event_loop::tests::replay_chunks` (with task 12's smoke test), `lifecycle::cover::cold_drive_tests` (47, as
  far as task 34 allows). Not here: `importance::store::legacy_split` (3, #1226). macOS/Linux: suite plus
  `index_benchmarks` before/after.
- **DONE**: bridge count zero in `cmdr-index` and `index-query`; #1232 updated with what's left.

### 10c. The app, part one: file system and listings

- **Size**: M.
- **Scope**: `apps/desktop/src-tauri/src/` `file_system/`, `listing_host/`, `volumes/`, `commands/`, `go_to_path/`,
  `favorites/`, `recents/`, `file_viewer/`, `native_drag/`, `about/`, `network/`. IPC arguments become app paths at the
  command edge through `from_wire`; OS integrations cross at `to_native`.
- **Landmines**: the resolver's un-hinted fallback (`resolve_path_to_volume` walks the mount table to `/`) is what D1
  and `remote_paths.rs` defend against; keep it answering exactly as today on macOS/Linux. Two-layer timeouts on path
  resolution stay. `root_volume`'s Windows answer is task 34's, not this plan's.
- **Test plan**: Windows: the app failures in these directories (of 188), and the `#![cfg(unix)]` files here gated only
  for #1225 un-gate. macOS/Linux: suite and `desktop-e2e-linux` (it touches listings).
- **DONE**: bridge count zero in these directories.

### 10d. The app, part two: operations host, tools, and the rest

- **Size**: M. Parallel with 10c (disjoint directories; the lead merges).
- **Scope**: `operations_host/`, `tools/` (MCP), `agent/`, `search/`, `listing_index_sizes/`, `multi_rename/`,
  `services_menu/`, and whatever 10c didn't own.
- **Test plan**: Windows: the remaining app failures that name "not an absolute Windows app path" or a `\photos`-style
  path; `cmdr` totals reported. macOS/Linux: suite.
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

## #1232's clusters, mapped

- `lifecycle::cover::cold_drive_tests` (47 timeouts): 10b, with task 34 for drive volumes.
- `reconcile::{reconciler,local_reconcile,reconcile_correctness,verifier}` (about 25): 10b.
- `lifecycle::phases::tests` (about 12), `scanner::{convergence_tests,policy_tests}`, `store::tests::path_resolution`:
  10b.
- `importance::store::legacy_split::tests` (3): not this plan (#1226).
- `watch::branches`, `watch::event_loop::tests::replay_chunks`: 10b plus Windows plan task 12.

## Review questions

1. Is accepting the POSIX `/` root on Windows right? It keeps `InMemoryVolume` and 4,800 test literals working, but the
   frontend grammar says a bare `/x` is no app path there. The alternative is a `mem://` root for the in-memory volume
   and rewriting every fixture.
2. Non-UTF-8 names stay lossy (as today). Does anyone want a lossless spelling, and should it be an issue now?
3. Is the flip (milestone 4) really one L, or should it split by implementor group with a short-lived `#[allow]`-style
   shim? The plan says one, to keep the trait from ever being half-typed.
4. Should `Volume::local_path`, `stands_in_for_own_disk`, and `local_backing_file` answer app paths (the plan's pick) or
   native ones? They feed the archive and restic routing.
5. Does `RelativeAppPath` earn its keep, or should `strip_prefix` return `&str`?

## Invariants

- macOS and Linux listings, wire paths, store rows, and sort orders are byte-identical before and after every milestone
  (milestone 1's pins).
- `bindings.ts` is byte-identical; no DTO changes type on the wire.
- No store migrates.
- No `From`/`AsRef`/`Deref` conversion exists between `Path` and `AppPath`.
- `to_native`/`from_native` appear only in allowlisted boundary files.
- App path equality and hashing are exact; folding takes the volume's `NameRules`.
- `AppPath`'s `Ord` equals `PathBuf`'s component order on POSIX-shaped paths.
- A link target is never an app path.
- Every listed entry costs at most the allocations milestone 1 pinned.
- `RemoteRoot` refuses a path off its root, matched by whole components after `..`, in both directions.
