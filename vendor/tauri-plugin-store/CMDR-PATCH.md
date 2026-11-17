# tauri-plugin-store 2.5.1, patched

The crates.io release of `tauri-plugin-store` 2.5.1 (upstream `tauri-apps/plugins-workspace` at `8c588be4`, path
`plugins/store`), swapped in through the root `Cargo.toml`'s `[patch.crates-io]`. Everything here is the published
package byte for byte, apart from the two changes below and two files left out:

- `Cargo.lock`: a path dependency resolves through the workspace lockfile, so it was never read, and `osv` would scan
  its stale pins as if they shipped.
- `LICENSE.spdx`: `osv-scanner` reads any `*.spdx` as an SBOM and stops on this one. The license texts and copyright
  stay in `LICENSE_MIT` and `LICENSE_APACHE-2.0`.

## The changes

- **Atomic save.** `src/store.rs`: `StoreInner::save` calls a new `write_atomically` instead of `fs::write`. It writes a uniquely named
  sibling temp file (`.<name>.<pid>.<n>.tmp`), syncs it, renames it over the store file, and syncs the parent directory.
  A symlinked store file is resolved first. When the store file exists, the temp file takes its permissions (created
  with its mode on Unix, then set exactly), so a save never changes the store's mode; a new store gets the default ones.
  On failure it removes the temp file and returns the error.
- **No save over a file that didn't load.** `src/store.rs`: `StoreInner` remembers whether its last load (the one at
  build, or a `reload`) found a file it couldn't read or parse (`read_file`; no file at all is a new store and doesn't
  count). While it did, `StoreInner::save` returns the new `Error::LoadFailed { path }` (`src/error.rs`) and writes
  nothing. Every save goes through it: `Store::save`, the debounced auto-save, the pending save on drop, and the
  `RunEvent::Exit` save in `src/lib.rs`. A load that succeeds clears it.

## Why

- Upstream's `fs::write` truncates the file and then writes it, with no sync, so a crash, a forced quit, or a full disk
  mid-save leaves `settings.json`, `shortcuts.json`, or `license.json` truncated or empty (#461, verified against
  upstream `v2` and 2.5.0 on 2026-10-16).
- Upstream's build discards the load's error and starts from an empty store, and the exit handler saves every loaded
  store, changed or not. So a store file that was there but couldn't be read (root-owned after a `sudo` run, a mode a
  restore reset, a disk read error) or parsed was overwritten with the empty store at quit, and `license.json` right
  away by the first license status check (#506, verified against 2.5.0 on 2026-10-20).

Cmdr's own tests for both: `apps/desktop/src-tauri/src/settings/plugin_store_tests.rs`.

2.5.1 (upstream's load/close deadlock fix, which reorders the resource-table and stores locks in `build_inner`,
`get_store`, and the exit save) still saves with `fs::write` and still discards the load error (checked against the
published crate, 2026-11-17). Re-vendoring was upstream's 2.5.0 → 2.5.1 diff applied on top of this copy; it applied
cleanly, and `tests/load_close_race.rs` came along with it.

## Dropping it

When upstream ships both, delete this folder and the `[patch.crates-io]` entry, and keep the tests.
`desktop-rust-vendor-patch-applied` fails as soon as a bump past 2.5.1 makes cargo resolve crates.io again.
