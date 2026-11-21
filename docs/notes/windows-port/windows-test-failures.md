# Windows test failures, by cause

Every failure of one whole-workspace run on the Windows builder, sorted into causes: which ones the app path effort's
per-crate conversions (#1225, `docs/specs/app-path-plan.md` milestones 5–10) take with them, which ones are a test's own
Windows problem, and which are product bugs. It picks up where `app-path-baseline.md` stopped: that list left 407
failures "cause unread" (their output names no cause), and this note reads them.

- **The run**: `main` at `8db5b83ac4`, 2026-11-19, in `D:\src\cmdr-triage` with `CARGO_TARGET_DIR=D:\targets\triage`,
  `cargo nextest run --workspace --exclude cmdr-fsevent-stream --no-fail-fast`: 13,978 tests, 13,037 passed, 875 failed,
  66 timed out, 669 skipped (612 s of tests after a cold build). Against `app-path-baseline.md`: 3 turned green, 10
  newly red (two crash-reporter cells that time out; the rest sit in the clusters below).
- **The record**: `windows-test-failures.tsv` beside this note, one line per failure (crate, test, cluster, whether the
  cluster was read from its output or inherited, first panic line), from `scripts/windows-test-baseline` (`tsv`). The
  raw log and JUnit report stay on the builder (`D:\logs\triage\run2.log`, `main-junit.xml`).
- **How the causes were read**: the tool's classifier reads each failure's output (its rules now cover every cluster
  below); a failure whose output names nothing takes its nearest classified sibling's cluster and is marked `unread`.
  For the unread ones, one or two per module were read by hand against the test's source (§ "Read by hand").
- **Running it so nothing hides**:
  `cargo nextest run --workspace --exclude cmdr-fsevent-stream --no-fail-fast --profile windows-baseline`
  (`.config/nextest.toml`), from a shell that holds the symlink privilege (Developer Mode, or elevated). A link cell
  without it skips itself (`cmdr_fs::testing::skip_without_symlinks`), which nextest counts as a pass; that profile
  keeps each pass's output in the JUnit report, so the tool's `render` and `diff` list every self-skipped cell by name.
  A full run lists none. Safety cells live there, like `unique_name::tests::a_dangling_symlink_counts_as_taken` (a write
  must never follow a dangling link), so a non-empty list means the run didn't cover them.
- **Gotcha, the builder ran out of memory**: the first build died with `os error 1455` ("the paging file is too small")
  mmapping an rlib while linking test binaries; `CARGO_BUILD_JOBS=2` got through (8 GB of RAM, 2 cores).

## Totals

941 failures (the 66 timeouts plus six more tests whose text says they timed out), by cluster, read + inherited:

- **Going with the app path conversions (#1225, milestones 5–10): 794.** S 344 (113 inherited), N 293 (139), P 63 (37),
  H 22 (8), and the 72 timeouts. ❗ Not the trait flip: milestone 4 bridges every body through `legacy_unchecked` /
  `as_std_legacy` and keeps today's handling by design, so it turns none of these green. Each crate's cells go green in
  the milestone that converts that crate (§ "By milestone").
- **Fixed on this branch: 64.** symlink 34, append-truncate 25, zip-creator 3, windows-names 1, home-env 1. 41 of them
  pass now; the 22 `cmdr-restic` link-fixture cells and one ledger cell move on to S and task 35.
- **Owned elsewhere: 83.** task 35 33 (6 inherited), #1226 24 (4), Windows-only expectations 13, task 34 6, Linux errno
  4, product gaps 3.

On the builder, this branch (`253f57d74f`) ran the 169 cells those fixes touch: 136 pass, and the 33 left are S, N, task
35, and timeouts (the 22 `cmdr-restic` link-fixture cells now reach their S cause,
`NotFound("/Volumes/…\\Browse snapshots")`). 24 of the 25 append-truncate cells pass; the 25th,
`a_crash_after_any_step_of_the_swap_loses_no_archive`, now fails further on.
`cargo check -p cmdr-index --features playwright-e2e --lib` builds.

## The clusters

Each: count, two examples, the cause, and whether it's a test or a product problem.

### Going with the app path conversions (#1225, milestones 5–10)

The four clusters are causes, but they turn green by CRATE, since each milestone converts its crates' bodies off the
bridge. The flip (milestone 4) changes signatures only.

- **S, `std::path` arithmetic on an app path (344)**:
  `cmdr about::history::tests::a_copy_off_another_volume_names_where_it_came_from`,
  `cmdr-s3 volume::batch_retry_test::a_throttled_batch_delete_goes_again_and_lands`. `Path::join` and friends put a `\`
  into a `/`-separated app path (`/srv/data\photos`, `sftp://h:22\srv`), or `std::path` re-roots a POSIX path
  (`NotFound("\\sdcard")`). Both product and test code; each crate's conversion replaces the arithmetic, so the asserts
  follow.
- **N, a native tempdir where an app path belongs (293)**:
  `cmdr file_system::watcher_incremental_test::a_batch_publishes_removes_then_adds_then_modifies`,
  `cmdr-index indexing::reconcile::reconciler::tests::subtree::reconcile_subtree_deep_nested_dirs`. A fixture hands
  `C:\Users\Admin\AppData\Local\Temp\…` to code that wants `C:/…`; the app path fence refuses it, or a `/root`-rooted
  test engine never owns it (`Source volume 'Test root' does not support export` in the golden copy trace). Test
  fixtures; each crate's conversion gives them an app-path tempdir.
- **P, a POSIX fixture path read in the host's flavor (63)**:
  `cmdr agent::suggested_ops::tests::the_drive_index_resolves_a_selector_over_a_real_index`
  (`NotOnVolume { path: "/Users/someone/Downloads" }`),
  `cmdr favorites::target::tests::a_local_folder_is_stat_and_reads_yes_or_no`. A literal `/Users/…` or `/Volumes/…` is
  no local path on Windows, so the code under test answers "not here". Test fixtures. Whether each cell becomes
  flavor-aware or Unix-only is that crate's milestone's call.
- **H, a native home path where an app path belongs (22)**: `cmdr go_to_path::tests::tilde_expands_to_home`
  (`C:/Users/Admin` against `C:\\Users\\Admin`), `cmdr tools::actions::tests::test_user_path_param_expands_tilde`
  (`C:\\Users\\Admin/Downloads`). The expectation builds from `dirs::home_dir()` while the code answers an app path; in
  the tools case the code itself glues a native home to `/Downloads`, which milestone 10d fixes in product code.
- **Timed out (72)**: 48 are `cmdr-index`'s `indexing::lifecycle::cover::cold_drive_tests`, which wait on an event a
  native-path walk never sends, plus `cmdr-restic` hub lifecycle (5), `cmdr-s3` pause cells (5), and the app's two crash
  reporter cells (new tonight, 8 s each, output empty). Credited to the app path conversion by assumption, as in
  `app-path-baseline.md`: unverified, since 48 of them (the cold-drive cells) printed nothing. The crash reporter pair
  belongs to #1232's Windows-only crash handling instead. Re-read all 72 after milestones 10b and 10d.

### By milestone

The 794, by the milestone that converts their crate (`docs/specs/app-path-plan.md` § Milestones), counts from the TSV:

- **5, `cmdr-fs`**: none in this run; its 24 cells were `#[ignore]`d for #1225 and run since milestone 5 (2026-11-19).
- **6, network backends**: 123 (`cmdr-s3` 47 with its 5 pause timeouts, `cmdr-nfs` 46, `cmdr-webdav` 23, `cmdr-sftp` 7).
  Seen on the builder (2026-11-19, `--profile windows-baseline`, the four crates only, 0 self-skipped): milestone 5
  already took 71 of these on `main` (`RemoteRoot` splits on `/` alone), so `main` at `450e3ab9f2` failed 56 (NFS 4, S3
  42 with the 5 pause timeouts, WebDAV 10, SFTP 0), and milestone 6's conversion (`31f8887eca`) leaves 5: none
  path-shaped. Still red and not this milestone's: `cmdr-nfs`'s three `errors_test` cells and
  `mutation_test::delete_removes_a_file_an_empty_folder_and_a_link_but_never_recurses` (an NFS status mapped to Win32's
  145 where Linux's `ENOTEMPTY` 41 is expected; the baseline saw it fail earlier, on a path), and `cmdr-s3`'s
  `nanoseconds_survive_and_trailing_zeros_drop` (100 ns `SystemTime`). `apply_metadata_dates_and_modes_a_folder` passes.
- **7, device backends**: 66 (`cmdr-adb` 51, `cmdr-smb` 13, `cmdr-mtp` 2). Converted 2026-11-21 (PR #1302), no builder
  run yet: expected green.
- **8, virtual volumes and leaf crates**: 72 (`cmdr-restic` 45 with its 5 hub timeouts, `cmdr-viewer` 13, `cmdr-archive`
  9, `cmdr-operation-log` 3, `cmdr-listing` 2).
- **9, `cmdr-local`**: none of its own; it's the native crossing 10a–10d wait on. Done (2026-11-19, in `D:\src\cmdr-m9`,
  `cargo nextest run -p cmdr-local`): 30 of 30 before on `main` (`a4449b5cb7`), 40 of 40 after on the branch
  (`d0490c0c11`), 8 skipped both times (the `#[ignore]`d task-35 conformance cells). The 10 new cells are the NTFS names
  (`local_windows_names_test.rs`: `dup`/`dup.`, device names and a trailing space, `a:b` with no stream, unpaired
  surrogates, native entry points), the relative bridge input, and the anchoring table. Its `&Path` entry points now
  take a native `C:\…` path (`list_directory_core`, `get_single_entry`, `rename_local_exclusive`), which is what the N
  cells through `list_directory_core` and the F cells (the finalize handing `rename_local_exclusive` a native path)
  failed on, so part of 10a's clusters turned green before their crate converts: `cmdr-operations` on the branch ran
  1,763 tests, 190 failed, against this note's 299 (115 of those now pass). The six failures not in the TSV are `main`'s
  own (all six fail on `a4449b5cb7` too, run alone): the three `refused_class_tests::*_native_join_refuses` cells and
  the two `link_landing_tests` overwrite cells (both added after this note's run), and
  `source_binding::tests::a_source_replaced_by_a_different_file_of_the_same_size_is_dropped` (node identity, task 35).
  What each needs (by reading, in the PR #1287 review, 2026-11-19; not rerun on the builder):
  - The three `refused_class_tests::*_native_join_refuses` cells (serial copy, concurrent copy, cross-volume move): they
    preset Windows' native join on an in-memory POSIX tree, and the volume engines still join with `std::path`, so they
    wait on 10a's typed join (`app-path-plan.md` milestone 10a).
  - `link_landing_tests::an_overwrite_with_a_link_lands_the_link_and_leaves_no_temp`: can't pass on Windows until task
    35 makes links real. The engine's Windows `symlink` (`crates/cmdr-operations/src/local_disk/platform/windows.rs`) is
    a stub that always answers `Unsupported`, so no link lands, and the cell gates only on whether the OS can make links
    (`skip_without_symlinks`). The destination is safe meanwhile: `overwrite::stage_and_land_link` makes the link under
    a temp first, so a refusal never touches the file that was there.
  - The second `link_landing_tests` overwrite cell: its failure text wasn't captured; rerun
    `cargo nextest run -p cmdr-operations link_landing_tests` on the builder before guessing.
  - The `source_binding` cell, and the product bug the same missing identity causes: #1291 (Windows `file_id` is `None`,
    so `may_be_same_file` is true for every existing destination and an Overwrite copy is silently skipped). The app's
    suites didn't run on the builder this round.
- **10a, `cmdr-operations`**: 229.
- **10b, `cmdr-index`**: 133, the 48 cold-drive timeouts among them.
- **10e, `cmdr-listing-wasm`**: 7.
- **10c and 10d, the app**: 164 (10c the file system and listings half, 10d the rest; the 2 crash reporter timeouts go
  to #1232 instead).
- **Misfiled**: `cmdr-index retry::a_machine_that_stops_short_asks_for_another_go` inherited P, but its fixture is a
  native tempdir with a timing precondition, so it's N or unknown (still 10b).

### Fixed on this branch

- **symlink, a link test without the right to make links (34)**:
  `cmdr-restic volume::tests::a_snapshot_tree_lists_files_folders_links_and_odd_names`,
  `cmdr-operations unique_name::tests::a_dangling_symlink_counts_as_taken`. `ERROR_PRIVILEGE_NOT_HELD` (1314): Windows
  creates a link only with Developer Mode or elevation. Test problem. The cells now skip themselves through
  `cmdr_fs::testing::skip_without_symlinks()`, which prints a skip marker the baseline tool counts (§ the run, above);
  `cmdr-restic`'s shared backup fixture makes its link only when it can, so 19 of its 22 cells now run on Windows (they
  still meet S).
- **append-truncate, a truncate on an append-only handle (25)**:
  `cmdr-operations in_flight_temps::tests::a_landed_temp_is_retired_from_the_log`,
  `cmdr-operations transfer::staged_write::tests::ledger::a_claimed_name_landing_is_on_disk_as_a_landing_before_it_clears_the_way`.
  The test helper's `set_len(0)` failed loudly; the PRODUCT claim did the same thing silently (below).
- **zip-creator, a zip mode written under the host's DOS creator (3)**:
  `cmdr-archive volume::volume_test::a_unix_mode_in_the_external_attributes_reaches_the_entry` (0o666 for 0o755),
  `cmdr-operations archive_edit::fresh_zip::tests::producer_preserves_names_empty_entries_metadata_and_level` (0o40775
  for 0o40750). A product bug and a test fixture with the same cause (below).
- **windows-names (1)**: `cmdr-operations scan_preview_tests::a_transfer_scan_counts_the_names_windows_cant_use` makes
  `a?b.txt` and `12:00 notes`, which Windows refuses (`ERROR_INVALID_NAME`). Now Unix-only.
- **home-env (1)**: `cmdr search::index_source::tests::canonicalize_scope_path_expands_the_home_tilde` read `HOME`,
  which Windows doesn't set. It now reads `host_home_dir()`, the home the expansion reads.

### Owned elsewhere

- **task 35, mounted-inside and node identity (33)**:
  `cmdr-operations delete::already_gone_tests::a_local_delete_carries_on_past_a_file_that_vanished_after_the_scan`,
  `cmdr-operations golden_trace_tests::golden_local_delete`. Every local delete or same-volume move under a temp folder
  refuses with `MountedVolumeInside { mount_point: "C:\\Users\\…\\Temp\\…" }`: the mount-inside guard reads an ordinary
  folder as a mount point. A product bug (a user's delete would refuse the same way), task 35's. Not app paths, so
  milestone 9 leaves it: the guard asks `cmdr_local::mounts::is_mount_point`, whose Windows arm answers "can't say" (no
  mount table yet), and "can't say" is a boundary by design.
- **#1226, a file another handle holds open (24)**:
  `cmdr-index importance::store::legacy_split::tests::a_background_split_is_waited_for`,
  `cmdr search::execute::tests::source_equivalence::memory_holds_the_same_arena_the_index_loads` (`os error 32`).
  Product.
- **Windows-only expectations (13)**: `cmdr data_wipe::plan::tests::a_library_target_must_carry_the_identifier`,
  `cmdr crash_reporter::next_launch_tests::crash_file_stamps_keep_their_bytes_and_read_back`. #1232.
- **task 34, root-volume lookups (6)**: `cmdr location::resolve::tests::a_home_relative_path_resolves_to_root`,
  `cmdr commands::volumes::tests::resolve_location_sftp_path_returns_the_saved_server_without_dialing`.
- **A Linux errno read through the host's codes (4)**:
  `cmdr-adb errors::errors_test::an_unlisted_device_errno_arrives_as_its_linux_class`,
  `cmdr-nfs errors::errors_test::a_lost_connection_is_a_transport_fault_and_a_refusal_is_not`.
- **Product gaps with a Windows plan task (3)**:
  `cmdr instance_lock::tests::acquiring_stamps_our_pid_and_names_the_holder`,
  `cmdr logging::rotating_file::tests::a_writer_rotated_away_by_another_reopens_the_live_file_instead_of_renaming_it`.

## Read by hand

The inherited (`unread`) clusters, checked one or two per module against the source. Where the inheritance was wrong,
the right cluster is named; the TSV keeps the tool's inherited answer.

- **`cmdr-operations` (100 inherited N)**: `create::tests::*`, `rename::tests::*`, `transfer::move_op::*`,
  `reversal::tests::*`, `settle_event_tests::*`, `approved_op_parity_tests::*` register a `/` test root and pass native
  tempdir paths (N, confirmed in `create_file_success` and the golden copy trace). `transfer::copy_strategy::tests::*`
  and `transfer::transaction_integration_test::*` assert `is_ok()` with no detail; they reach `stage_and_land_file`, so
  likely the finalize cluster F (`cmdr-local`'s rename handed a native path), inferred, not proven.
- **`cmdr-operations archive_edit::remote` (25)**: `PathBuf::from("/device/bundle.zip")` on an `InMemoryVolume`, so S.
  Was misread as append-truncate before the classifier stopped inheriting a test-local cause.
- **`cmdr-operations transfer::volume::swap_outage_tests` (8)**: "a whole temp lands without the file streaming again"
  over remote fakes with POSIX paths: S.
- **`cmdr-index indexing::reconcile` and `indexing::lifecycle` (unread N and P)**: native tempdirs into an index that
  stores app paths (N), and `/Users/statustest/…` fixtures (P).
  `a_registered_folder_that_is_not_a_mount_point_stays_on_root`, `enrich_entries_fallback_individual_paths`,
  `enrichment_under_contention`, and `mixed_storm_reaches_consistent_fixed_point` inherited #1226 but are P (POSIX
  fixture roots), not an open file.
- **`cmdr about::*` (18)**: the card is built over a native tempdir path, which the session refuses, so the held job
  never starts (`the slow job starts: Disconnected`): N.
- **`cmdr priority::roots`, `favorites::target`, `agent::line`**: POSIX literals (P).
- **`cmdr-archive boundary*` (8)**: `zip.join("inner/a.txt")` makes `…\real.zip\inner/a.txt`, which the boundary
  detector slices by `/`: S (inherited H).
- **`cmdr-viewer` markdown cells (13)**: `Path::new("/Users/a/proj/docs/README.md")` resolves to `Blocked`: P.
- **`cmdr-s3`, `cmdr-webdav`, `cmdr-smb`, `cmdr-sftp`, `cmdr-nfs`, `cmdr-adb`, `cmdr-mtp`**: `volume.root().join(key)`
  and POSIX remote roots: S. `cmdr-adb`'s `errno 30` (`EROFS`) cells are the fake device refusing `\sdcard\…`.
- **`cmdr-listing stall_test::a_read_that_never_answers_reports_the_listing_stalled_within_the_deadline`**: attributes
  the stall to `Unknown` because its fixture reads `/`, which isn't a local disk on Windows: P. Its comment ("a local
  disk on every machine the tests run on") is the Unix assumption.

## Product bugs found

1. **The in-flight ledger never emptied on Windows** (fixed here). `init_and_sweep` truncated with `set_len(0)` on its
   append handle and dropped the error; Windows grants an append-only handle `FILE_APPEND_DATA` without
   `FILE_WRITE_DATA`, so the truncate is "Access is denied". Every launch replayed every record the log ever held, and
   the log only grew. Now `log_writer::open_emptied` (`crates/cmdr-operations/DETAILS.md` § the ledger file).
2. **A zip Cmdr made on Windows lost its entries' Unix modes** (fixed here). `fresh_zip` wrote `unix_permissions` under
   the `zip` crate's default creator, the host's (DOS on Windows), and a reader takes a DOS entry's attributes for the
   read-only flag alone. An entry carrying a mode now declares a Unix creator.
3. **A local delete under a temp folder refuses as "a mounted volume inside"** (task 35, not fixed): 33 cells.
4. **`cmdr://state` lists only `root` and MTP on Linux** (#1263, not fixed): not intended; the known gap in
   `apps/desktop/src-tauri/src/tools/resources/DETAILS.md`. Fixing it is an agent-facing change on a platform the
   builder can't run, so it waits for a Linux build to verify on.
5. **`platform_backends()` had no arm on Windows with `playwright-e2e`** (fixed here): a compile error in that build.
6. **A link copied over an existing file deleted the file before making the link** (fixed here, older than this run).
   Where the link was refused (always on Windows today, Linux's exFAT and FAT, an immutable folder, a share with no link
   operation), the person's file was gone and nothing replaced it. The link is now made under a temp name and renamed
   over the file, and a refusal is the typed `LinksNotSupported` where the errno can only mean that
   (`crates/cmdr-operations/DETAILS.md` § "Key patterns and gotchas (shared)").
