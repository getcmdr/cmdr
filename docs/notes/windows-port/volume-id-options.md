# Windows volume ids: the options (task 34)

A PISS for what was the open decision in `docs/specs/windows-port-plan.md` task 34: which id a Windows drive, folder
mount, and UNC share gets. **Decided**: David chose (d) on 2026-11-18, recorded as D7 in the plan. Researched 2026-11-18
on the integration branch (code reading; Windows API behavior marked "to verify" wasn't run on the builder).

## Problem

- D1 makes every Windows app path name its root (`E:/x`, `//server/share/x`), but a volume id is a separate string that
  keys durable state, and Windows reuses and reshuffles drive letters far more than macOS reuses `/Volumes` names.
- Pick the id wrong and a drive's index, favorites, and remembered paths either orphan on every letter change or, worse,
  attach to whichever other disk takes the letter.
- One share has three addresses on Windows (a mapped `Z:`, `//server/share`, and Cmdr's own SMB backend), and `subst`,
  folder mounts, and the system drive's `"root"` id each need an answer.

## What macOS and Linux do today (the precedent)

- **The id is identity, the root is an address.** `cmdr_fs::volume::ids` is the one funnel.
  `volumes/platform/macos/ids.rs::volume_id_for` runs a ladder: an SMB mount keys on `(server, port, share)` (`smb-`),
  another network mount on its path (`path-`), a local mount on its filesystem UUID (`vol-`), else its path. Linux's
  `ids.rs` runs the same ladder through `/dev/disk/by-uuid`. `/` is always `"root"`.
- **An id is never a raw path anyway**: it becomes a filename (`index-{id}.db`, `importance-`, `media-`), so it's
  `{scheme}-{slug}-{digest}`. Even option (a) would be `path-c-<digest>`, not `C:/`.
- **Two disks named `Backup`**: the second mounts at `/Volumes/Backup 1`, gets its own `vol-` id from its UUID, and each
  keeps its own index. Same disk back at a new mount name: same id, same index.
- **Where an id is persisted, and what a moved root does to it:**
  - Drive index and importance (`index-{id}.db`, `importance-{id}.db`): rows are volume-relative
    (`location/store_path.rs`), so a moved root costs nothing. An id that disappears is swept by
    `indexing/resources/retention.rs` (root exempt).
  - Media index: full path strings, so a moved root re-enriches.
  - Favorites (`favorites/store.rs`): store the volume id plus the root at add time and rebase onto the volume's current
    root. Dedup on id plus path-under-root, ❌ never path alone (#349).
  - `lastUsedPaths[volumeId]` and tabs: tabs restore by PATH (`pane/initialization.ts::resolveVolumeId` re-resolves the
    volume from the path and trusts the stored id only for `network` and SMB). So a restored tab on `/Volumes/Backup/x`
    lands on whatever disk is at that path now, or walks up to an existing ancestor.
  - Operation log (`cmdr-operations` `journal.rs`): source and destination volume ids plus absolute paths. Rollback
    finds the volume by id; a moved root leaves the recorded paths stale (it skips as gone or drifted), and it never
    rebases.
  - MCP and agent tools (`location/resolve.rs`): a named volume rebases a path under an earlier root (`RootHistory`, in
    memory, seeded from saved shares' last mount paths); a bare path goes to the deepest live root. ❌ No "saved last
    root" tier, because a reused mount point would resolve to the wrong volume.
  - Saved servers: keyed by their own scheme ids (`sftp-`, `smb-`, …), unaffected by this decision.
  - Telemetry and deep links: carry no volume id.
- **`"root"` is special in about 210 Rust lines (59 files) and 84 frontend lines (51 files)**: index storage lives on
  it, priority roots answer only for it (`priority/roots.rs`), retention never deletes it, the MCP volumes resource has
  a hardcoded non-macOS row (`tools/resources/volumes.rs` ~469), and the frontend's `ROOT_VOLUME_ID` drives the index
  checklist and the size hourglass.

## Options

### (a) The id is the root path (`path_volume_id("E:/")`, `path_volume_id("//server/share/")`)

- **Stores**: nothing to migrate (no Windows installs). Simplest mapping.
- **Removable drive back on another letter**: a new id, so its index orphans (rescan) and its favorites and
  `lastUsedPaths` point at the old letter.
- **Two drives swap letters**: each inherits the other's index, favorites, and op-log volume. This is the exact "reused
  mount point resolves to the wrong volume" failure `location/resolve.rs` refuses on macOS. The index survives it only
  as a cache; a rollback leans entirely on the planner's drift checks.
- **One share, three addresses**: three volumes, so three indexes and, once Cmdr's SMB backend upgrades a share, a
  second session beside the OS one. Breaks the macOS rule that an OS mount and the direct backend share one `smb-` id.
- **`subst` and folder mounts**: each is its own path id; fine.
- **System drive**: either `path-c-…` (rewrites ~300 `"root"` sites and breaks the "root" contract) or `"root"` as an
  exception, which makes (a) not self-describing after all.
- **Later tasks**: 39's device-arrival watcher (`CM_Register_Notification`) reports a device, not a letter, so each
  arrival must map back to a letter first.

### (b) A stable OS identity is the id; the letter is only display

- The volume GUID path (`\\?\Volume{…}\`) from `GetVolumeNameForVolumeMountPointW`, or the volume serial from
  `GetVolumeInformationW`. As literally framed, it fights D1: D1 puts the letter in every app path, so the letter can't
  be display-only. Read with D1 it becomes (d).
- GUID vs serial: the GUID is per machine and per partition (the mount manager keeps it in
  `HKLM\SYSTEM\MountedDevices`), so the same stick gets the same GUID each time, a cloned disk doesn't collide, and a
  reformat keeps it (the index then reconciles a new tree). The serial lives on the filesystem: FAT and exFAT carry only
  32 bits, clones share it, a reformat changes it, and it's reported through `subst` drives (the underlying drive's) and
  mapped drives (the server's), so it would merge things that aren't one volume. To verify on the builder. GUID wins.
- UNC shares have no GUID, so (b) alone has no answer for them.

### (c) Hybrid: the root path is the id everywhere; a GUID keys only the drive index

- **Stores**: the index survives a letter change, but favorites, `lastUsedPaths`, the op log, and MCP stay letter-keyed,
  so a letter swap still hands one disk's favorites and rollbacks to the other. It fixes the cheapest store (a cache)
  and leaves the ones holding user intent.
- **Code**: two identities per volume and a persisted id-to-GUID map the index consults. No macOS or Linux store works
  this way, so it's a Windows-only concept in shared code.
- **One share, three addresses**: still three volumes.

### (d) The macOS ladder, applied as is (recommended)

- **Rule**, in a new `volumes/platform/windows/ids.rs` through the same `cmdr_fs::volume::ids` constructors:
  1. A mapped drive or a UNC root keys on `(server, port, share)` via `smb_volume_id` (`WNetGetConnectionW` gives `Z:`'s
     target). So `Z:`, `//server/share`, and Cmdr's SMB backend are ONE volume, as an OS mount and the direct backend
     are on macOS.
  2. A local drive or folder mount keys on its volume GUID via `local_volume_id` (`vol-`).
  3. Nothing else (a `subst` drive, whose `GetVolumeNameForVolumeMountPointW` fails, to verify): `path_volume_id` on its
     root.
  4. The system drive stays `"root"`: `local_volume_id`'s `mount_path == "/"` check gains a Windows twin for
     `%SystemDrive%`.
- **Removable drive back on another letter**: same id, so the index and favorites stay (favorites rebase onto the new
  root). Tabs restore by path, as on macOS, so a tab on the old letter lands wherever that letter points now. The op
  log's old paths skip as gone.
- **Two drives swap letters**: ids follow the disks. Indexes and favorites are right. Restored tabs and the op log's
  paths follow the letter, same as macOS's reused mount name (see the argument against, below).
- **One share, three addresses**: one id, one canonical root. The rule "one id publishes one location at one root"
  (`canonical_root::collapse_by_volume_id`, shortest wins) picks `Z:/` when mapped, else `//server/share/`. The other
  spelling needs to resolve as an alias root (the `earlier_roots` rebase already does it for MCP; Go to path and tab
  restore need the same). That alias is the one genuinely new piece.
- **`subst`**: a `path-` volume of its own; the into-itself guard already canonicalizes native paths (app-path plan §
  "One file, several app paths"). Arguably `subst` drives shouldn't register as volumes at all, only resolve; a detail
  for task 39.
- **Folder mounts** (`C:/mnt/data`, no letter): its GUID id, root `C:/mnt/data/`, found by `mount_id_for_path`'s
  longest-mount-first rule, exactly like `/Volumes/x` under `/`. `FindFirstVolumeW` plus
  `GetVolumePathNamesForVolumeNameW` lists GUIDs with all their letters and folders in one sweep, which is task 35's
  mount table for free.
- **macOS and Linux**: nothing changes; it's a Windows arm of an existing seam.
- **Frontend and `bindings.ts`**: no shape change (ids stay opaque strings, `volumeScheme` already knows every scheme).
  `ROOT_VOLUME_ID` keeps meaning the system drive.
- **Index**: `ROOT_VOLUME_ID` and the boot-volume priority roots keep working for `C:`; what still changes is the root
  index's path space, which has to root at `C:/` (task 34's other item, needed under every option).

## Recommendation: (d)

- It's not a new design: it's the rule two OSes already run, with the one Windows-shaped addition (alias roots for a
  mapped share). Identity-keyed state (index, favorites, `lastUsedPaths`, the op log's volume) follows the disk, and the
  share question gets the answer macOS already gives.
- **The strongest argument against**: on Windows the id and the letter in every stored path come apart often, and only
  part of the app rebases a path when a root moves. Favorites and MCP do; tabs restore by path, `RootHistory` is in
  memory only, and rollback never rebases. On macOS that gap bites rarely (a second same-named disk); on Windows a
  letter swap is routine, so a restored tab can open the other disk's `E:/x`. Closing it means a persisted per-id last
  root (or tab restore trusting the stored id like it does for SMB), which is a small follow-up but touches the
  frontend's restore path. Option (a) avoids the gap by never letting id and letter diverge, at the price of letting
  state follow the wrong disk.

## Size

- **(a)**: M. Ids are trivial; keeping or replacing `"root"` and teaching task 39's watcher to map devices to letters
  are the work. The wrong-disk cases stay as known bugs.
- **(b)**: not shippable alone (no UNC answer, fights D1).
- **(c)**: M+. (a)'s work plus a persisted GUID map and a second identity in the index; leaves (a)'s wrong-disk cases
  for every store but the index.
- **(d)**: M. A Windows `ids.rs` (about 150 lines plus tests) and the `"root"` twin for `%SystemDrive%`; alias roots for
  mapped shares (the main design piece, S); the persisted last root so tabs follow a moved letter (S, also fixes macOS's
  rarer case). Makes tasks 35 (the mount table falls out of the GUID sweep), 39 (an arrival already names the GUID), and
  45 (a mapped share is the `smb-` id) simpler; leaves the app-path plan's milestones untouched, since app paths and
  their roots are D1's either way.

Clear win or tradeoff? A tradeoff, mostly won by (d): it's the only option where user intent follows the disk, and its
gap (tabs after a letter swap) is one that macOS already has and one small fix closes for both.
