# Go to path details

Depth and rationale. `CLAUDE.md` holds the must-knows; the decision rationale and v1 limitations live here.

## Key decisions

- **The backend owns all path resolution; the frontend is a thin presenter.** One `resolve` call serves three callers:
  the live as-you-type warning, the actual jump, and the clipboard-prefill check. A single resolution path means the
  preview and the action can never drift (AGENTS.md principle 3, "smart backend, thin frontend").
- **`..` means what the volume holding the path means** (decided in the app-path plan's review round 2, landed in 10c,
  #1225; David's OK 2026-11-21). The rule and its cells: `docs/specs/app-path-plan.md` § "Typed and pasted `..`".
  - A relative input's LEADING `..`s step up from the pane's folder by text, clamped at the root, as the pane's own
    `..` row and a shell's `cd ..` do: "up from where I am". From a pane inside `/x/link`, `../y` is `/x/y`.
  - Every other `..` asks the volume through `Volume::parent_for_dot_dot`. The local POSIX disk and SFTP answer the
    link target's parent when the last name before the `..` is a symlink (`cmdr_fs::volume::dot_dot`), so
    `/x/link/../y` lands on the target's sibling, as `ls /x/link/../y` does. Windows keeps the lexical default: Win32
    resolves `..` by text before the disk sees it, through junctions and directory symlinks alike, so `C:/x/link/../y`
    is `C:/x/y`. SMB, S3, MTP, ADB, archives, and virtual volumes have no links (or resolve by text) and stay lexical.
  - Faithful by default: only a `..` that crosses a link changes the text, and `crossedLink` tells the dialog so it
    can show where the text really leads (the `info`-tone preview line). That's why this isn't `canonicalize`, which
    needs the whole path to exist (breaking nearest-ancestor) and rewrites every link in the shown path into something
    the user didn't type. A name that doesn't exist is no link, so `<dir>/nope/..` is `<dir>` and a missing tail still
    reaches its nearest existing ancestor. `metadata()` still follows links for the file-vs-dir classification, so a
    symlinked dir navigates into the link path and the listing follows it, correct and intended.
  - A `..` above a root clamps there for a person (go-to, the CLI) and is refused for an agent (`OffRoot`, the MCP
    tools' rule for anything ambiguous; 10d wires them).
- **Every answer is an app path** (`cmdr_fs::typed_path` builds it with `AppPath` arithmetic, never `std::path`: a
  scheme-rooted pane's `sftp://h:22/srv` kept its `//`, #1314 item 1). On Windows a typed `C:\Users\test_user_1`,
  `~\Documents`, or `\\server\share` comes back as `C:/Users/test_user_1/…` or `//server/share/`, and a person's names
  lose their trailing dots and spaces as Explorer's address bar drops them (`from_person_typed`'s rule). `~` expands per
  the grammar (`cmdr_fs::tilde`, #1109).
- **The volume comes from the registry, in memory**: the longest registered root holding the path, routed volumes
  (archives, `.git` portals) skipped, the boot volume for a local path no closer root holds. No syscall and no dial; a
  scheme path whose server isn't registered has nobody to ask, so a `..` there refuses.
- **Two-layer timeout.** Every stat and every `..` in one resolve runs detached under ONE 2 s `Deadline`
  (`AppHost::within`, `cmdr_fs::deadline::timeout_detached_within`): a hung mount answers `ConnectionTimeout`, and the
  work runs on in its own task rather than being dropped (the MTP rule). The command itself then answers inside the
  deadline, and the frontend's `withTimeout` races it. The local `metadata` and the archive check run on the blocking
  pool inside it.
- **A remote pane's relative input classifies through its volume** (`is_directory`), a local path through `metadata`.
- **Recents rides on the shared `crate::recents` list.** This module supplies the entry shape, the dedupe key (the
  resolved path string), and the cap; the file, dedupe, eviction, and quarantine are shared with Search and Selection.
  The cap stays a per-call argument, so this store's fixed `MAX_RECENTS` never becomes a knob the tunable lists carry, or
  the reverse. Where the shared type's edge sits, and why: `apps/desktop/src-tauri/src/recents/DETAILS.md`.
- **A saved server that moved respells the recents under its old address** (`history::follow_server_move`, called by
  `../server_move.rs`), by whole segments. The old address names nothing saved any more, so a row spelled with it would
  open an Add sheet instead of the server. Handle-less, through `config::standalone_app_data_dir()`, the dir the
  app-bound facade resolves too.

## v1 limitations

- **Case-insensitive dedupe.** The dedupe key is a raw resolved-path string, so on case-insensitive APFS
  `/Users/x/Foo` and `/Users/x/foo` show as two entries. Worst case: a duplicate-looking row. We don't `canonicalize()`
  to fix it (symlink / nearest-ancestor reasons above). Pinned by `the_dedupe_key_is_case_sensitive` in `history.rs`, so
  a change to it is deliberate.
