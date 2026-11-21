# `cmdr-restic`

Reads a restic backup repository, wherever it lives, as a read-only volume of snapshots (#422). Built on `rustic_core`,
read THROUGH the repo's parent `Volume`, so a repo on SMB, SFTP, S3, or a phone opens like a local one. No `tauri`, no
English a user reads. The app's half: `apps/desktop/src-tauri/src/file_system/restic/CLAUDE.md`.

## Module map

- `volume.rs`: `ResticVolume`, the view at `<repo>/Browse snapshots`, and its `PasswordLocked` unlock. `hub.rs`:
  `ResticRepos`, the shared state (passwords, opened repos, snapshot lists, `latest`, the is-it-a-repo memo);
  `index_load.rs` (the shared, cancellable index load) and the hub's child `index_slot.rs` (cap, idle drop) hold its
  indexes; `index_feed.rs` is the leaf the load fills and the adapter reads, so neither names the other. The hub's other
  children: `latest_pin.rs` decides when `latest` may move, `passwords.rs` unlocks, files, forgets, and looks up.
- `path.rs` (re-exports `cmdr_fs::restic_view`, classifies paths in the view), `snapshots.rs`, `tree.rs`,
  `read_stream.rs`, `repo.rs` (open, index, typed errors), `backend.rs` (rustic's storage over the parent), `detect.rs`.
- `test_fixtures.rs` (`testing` only): real v1 and v2 repos built with rustic, plus an unlock door.

## Must-knows

- **Read-only, always.** Every `WriteBackend` method refuses; tests pin zero write attempts across browse and read.
- **❌ Never call rustic on an async worker**: the adapter `block_on`s the parent. Use `host.runtime().spawn_blocking`.
- **❌ Never classify a rustic error by its text** (DETAILS § "Typed errors"), ❌ never detect by trying a password.
- **A dated folder resolves by its id suffix, ❌ never the time prefix.** An ambiguous suffix is `NotFound`.
- **`latest` doesn't move while anything reads under it**: copies address files by path.
- **`to_indexed` consumes the open repo**, so the index is its own open (`IndexedRepo`).
- **One index load per repo, cancelled by the LAST waiter leaving** (a dropped future). ❌ Never abort a parent read;
  detach it (a dropped device transaction wedges a phone). Progress is real bytes.
- **Index files stream through `IndexFeed`'s bounded window at every size.** Any rustic call that fans out (the index
  build, a snapshot refresh) runs in `repo::on_own_pool`. ❌ Never on rayon's global pool: search and icons stall.
- **A pack gone under a loaded index (a prune elsewhere) reloads it once**: read through `ResticVolume::on_index` or the
  stream's `IndexSource`, ❌ never `indexed` plus a bare read.
- **❌ Never evict an index in use**; eviction and the idle drop take the index only, ❌ never the password.
- **Passwords go through `secret_store`**, filed only by `remember_password` after a verified unlock nobody cancelled.
  ❌ No hub-wide lock across scrypt, a Keychain call, or a parent read.
- **A forget wins**: anything that installs repo state (password, opened repo, snapshots, index) commits only while
  `RepoSlot::forgets` is unchanged since it started. ❌ Never add an install site without that check.
- **Mode is `& 0o777` of Go's `os.FileMode`**; names go through `Node::name()`, ❌ never the raw field.
- **A hostile repo can't reach scrypt**: the adapter caps key count, size, and summed cost first.
- **The heavy state is the hub's**, keyed by `(volume id, repo root)` as spelled; ❌ no `canonicalize`, ❌ no static.
- **Paths are `AppPath`s, ❌ never `std::path`**: fixed names join with `path::child`; rows and errors spell through
  `path::child_text` / `text_of`, which keep main's text byte for byte (DETAILS § "App paths").
- The public surface is capped by `index-crate-isolation`; argue an addition in `DETAILS.md`.

The view, the hub, the index lifecycle, the adapter, errors, naming, fixtures, and the capped surface: `DETAILS.md`.
