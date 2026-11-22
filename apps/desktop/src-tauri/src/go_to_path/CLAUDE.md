# Go to path (backend)

Backend for the "Go to folder" dialog (⌘G): the user types a path and jumps to it in the focused pane. Owns all path
reasoning plus the recent-paths store; the IPC layer (`commands/go_to_path.rs`) is a thin pass-through. Also the app's
ONE typed-path resolver, `AppHost::resolve_typed`, for every text a person or an agent types.

## Module map

- **`mod.rs`**: `AppHost` (the registry picks the volume holding a path; every stat and `..` runs detached under one
  2 s `Deadline`), `AppHost::resolve_typed` (go-to, the CLI's paths, deep links via `reveal::delivery`, and the MCP
  tools' inputs via `apps/desktop/src-tauri/src/tools/agent_path.rs`; `AppHost::on_volume` for text whose volume is already picked, the transfer box's `..` in
  `commands/wire_path.rs`), and `resolve(input, base_dir) -> GoToPathResolution`. The rules are `cmdr_fs::typed_path` (`~`, the pane's
  folder, each `..`) and `cmdr_fs::go_to_path` (classify), shared with the browser demo
  (`crates/cmdr-listing-wasm/src/go_to.rs`). Cells: `tests.rs`, `wire_shape_tests.rs` (the symlink pins).
- **`long_names.rs`**: a typed path's Windows 8.3 short names spelled long (#1337), inside the resolve's deadline.
- **`history.rs`**: the `RecentPathEntry` shape and `RECENT_PATHS`, a `crate::recents` list keyed on the resolved path
  with a fixed cap

Decision rationale and v1 limitations: `DETAILS.md`.

## Must-knows

- **`GoToPathResolution` is the contract; never classify by string-matching the `reason` message** (AGENTS.md
  § no-error-string-match). Four variants: `Directory { path, crossedLink }`, `File { path, parentDir, fileName,
  crossedLink }`, `NearestAncestor { requested, ancestorDir }`, `Invalid { reason }`. `File.path` is recorded into
  recents verbatim. The tagged-enum serde attrs (`tag = "kind"`, `rename_all = "camelCase"`,
  `rename_all_fields = "camelCase"`) are required (`ipc-enum-camelcase`).
- **Faithful by default; one link resolved only where a `..` crosses it, ❌ never `Path::canonicalize`.** A relative
  input's leading `..`s go up from the pane by text (the `..` row's way); every other `..` asks the volume
  (`Volume::parent_for_dot_dot`), so `/x/link/../y` lands beside the link's TARGET, where `ls` lands, and
  `crossedLink` makes the preview say so. `canonicalize` needs the whole path to exist (breaking nearest-ancestor) and
  rewrites every link in the path. A missing name is no link. Spec: `docs/specs/app-path-plan.md` § "Typed and pasted
  `..`".
- **A volume that can't answer refuses, ❌ never a lexical guess**: a hung mount or an unregistered server is
  `Invalid` (go-to), a dropped path (CLI), or a typed `TypedPathRefusal::Unanswered` (agent). An agent's `..` above a
  root is `OffRoot`; a person's clamps.
- **Time out through `AppHost::within` (`cmdr_fs::deadline`), ❌ never a bare `tokio::time::timeout`**: it detaches,
  so an MTP stat isn't dropped mid-transaction. The frontend's `withTimeout` is the second layer.
- **Recents keys on the resolved target, not the raw input.** Entry `{ id, timestamp, path }` where `path` is the dir,
  file path, or nearest ancestor we actually jumped to; the dedupe key IS that string, so a re-jump moves the row to the
  top. Cap is the const `MAX_RECENTS = 10` (not a setting), passed per call: the dialog shows up to 10 via digit keys
  1-9, 0. The file, quarantine, and locking are `crate::recents` (`apps/desktop/src-tauri/src/recents/CLAUDE.md`).
  Populated only by manual dialog jumps (frontend convention, not enforced here).
