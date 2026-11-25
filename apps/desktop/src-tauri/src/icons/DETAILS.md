# Icons module details

Depth and rationale for OS icon retrieval and caching. `CLAUDE.md` holds the must-knows; the tier narratives,
detection-timing decision, and disk-cache mechanism live here.

This is the Rust `src/icons/` module. (`src-tauri/icons/`, a sibling at the crate root, holds the app *bundle* icons,
unrelated.)

## Asking macOS for the pixels (`platform/macos/workspace.rs`)

`NSWorkspace.iconForFile:` answers an `NSImage`, a resolution-independent recipe rather than a buffer, so every fetch draws it: allocate an `NSBitmapImageRep` at the target size, wrap it in an `NSGraphicsContext`, draw, and read `bitmapData`. The bitmap is `RGBA8`, non-planar and tightly packed, which is exactly what `image::RgbaImage::from_raw` adopts with no copy of its own.

- **Per-call bitmap and context, never a shared one.** The fetch runs on rayon workers and on the dedicated 8 MB-stack threads, so one drawing surface would be two threads compositing into the same buffer. The allocation is noise next to the Launch Services round trip.
- **`NSCompositingOperation::Copy`, not `SourceOver`.** The bitmap arrives as uninitialized memory, so blending over it mixes garbage into every transparent pixel. `renders_a_real_icon_at_the_requested_size` pins this on the corner alpha.
- **`render_ns_image` is the draw on its own**, taking any `NSImage`: `render_file_icon` is that plus the `NSWorkspace`
  lookup. ⚠️ It SETS the image's size while drawing, so an `NSImage` shared with something on screen comes back resized.
  (A menu needs no pixels: the `Share` submenu hands each service's own `NSImage` straight to its item.)
- **The path is canonicalized first**, which doubles as the existence check (`iconForFile:` hands back a plausible generic document icon for a path that isn't there, and that would cache under an id meaning something else) and resolves symlinks. The id scheme wants that: `symlink-file` / `symlink-dir` are separate ids fetched from their own samples.

Decision/Why: this replaced the `file_icon_provider` crate, which did the same drawing but also reached `UTType` from a caching struct Cmdr never constructed. That reference alone hard-linked `UniformTypeIdentifiers.framework` into the binary, and since that framework arrived in macOS 11, dyld refused to launch Cmdr at all on the 10.15 floor `tauri.macos.conf.json` promises. Sixty lines of `NSWorkspace` we own beats a dependency that can put a framework in the binary for code we don't run, and it drops `gio`, `gtk`, and `windows` from the macOS graph as a side effect. `desktop-macos-framework-floor` now fails the build on any framework newer than the floor; the version evidence is in `docs/notes/system-requirements-and-es2025.md`.

## Asking Windows for the pixels (`platform/windows/`)

`shell_image.rs` asks `IShellItemImageFactory::GetImage(32×32, SIIGBF_ICONONLY)` on a Shell item made from the path, and reads the `HBITMAP` back with `GetDIBits` as 32-bit top-down BGRA, then converts premultiplied BGRA to straight RGBA. Every arm then encodes at `ICON_SIZE` (32), which the list draws at 16 CSS px, so 100–200% scaling is covered as on macOS.

- **Decision/Why `IShellItemImageFactory`, not `SHGetFileInfoW`**: `GetImage` hands back exactly the size asked for, picking the icon resource's own frame for it (the hand-drawn 32 px frame, not a resample), whatever the process's DPI. `SHGetFileInfoW` only knows the system's small and large sizes (`SM_CXICON`, which moves with DPI), and its `SHGetImageList(SHIL_JUMBO)` route draws a type with no 256 px frame as a 48 px icon in the corner of a 256 px canvas. (Microsoft Learn: `IShellItemImageFactory::GetImage`, `SHGetImageList`, read 2026-11-24; the pixels are unverified on hardware, see the real-Windows checklist in #942 task 41.)
- **`SIIGBF_ICONONLY`, always**: a thumbnail would read the file's content, and on a cloud placeholder start a download.
- **Premultiplied unless proven straight**: the Shell's 32-bit icons come back premultiplied, so each color is divided by its alpha. A buffer where some color exceeds its alpha can't be premultiplied and is taken as straight; a buffer with every alpha 0 (an old 24-bit icon) is opaque; a buffer of nothing but zero bytes is no icon (`None`), not a black square. Pinned by the `bgra_*` and `an_all_zero_buffer_is_no_icon` tests.
- **One STA per asking thread** (`../platform/windows/apartment.rs`'s `ensure_on_this_thread`, a `thread_local`, left when the thread ends): the image factory and `ShellLink` are documented for STA, and an apartment-threaded third-party icon handler can fail quietly from an MTA. A thread another library already put in an MTA keeps it. The same pattern as `cmdr-operations`' trash and `commands/file_actions/platform/windows.rs`.
- **Type ids ask Cmdr's own samples**, as on macOS: `ext:{x}` an empty `<temp>/cmdr-icon-samples/sample.{x}`, `dir` the sample folder, and `file` / `symlink*` an extensionless `sample-file` (the Unix arms' `/etc/hosts` has no Windows twin). So a type lookup never touches a user's file, and samples skip the no-touch rule (a `%TEMP%` on a network home folder still draws every type).

**Files with their own icon (`path:{file}`).** Explorer draws a program, a shortcut, and an icon file with the icon it holds or points at, so `file_extension::has_own_icon` (a pure name check, `crates/cmdr-fs/src/file_extension/platform/windows.rs` holds the list) gives such a file its `path:` key during listing (`cmdr_fs::icons::own_icons`). It shares the `path:` lifecycle (LRU-capped, never in localStorage) and the disk tier, whose mtime token tracks a rebuilt `.exe`. A shortcut (`.lnk`, `.appref-ms`) stays out of the disk tier (`own_icons::keeps_on_disk`): its icon follows a file its own mtime can't see.

- Decision/Why no `.url`: an Internet shortcut's `IconFile=` can name `\\host\…`, and drawing it opens an SMB connection on listing, which can offer the user's NTLM credentials. It always shows its type icon.

**The no-touch rule (`place.rs`, `may_read_own_icon`).** The Shell reads the file to draw its own icon, so a real place is asked only when it sits on a fixed, removable, or RAM drive (`GetDriveTypeW` on the letter; a UNC path and a verbatim `\\?\` one answer no by shape, a mapped drive from its letter's kind, none reaching the server), isn't itself a symlink or junction (its reparse tag, read with `FindFirstFileW`, never followed; Cloud Files tags don't count), and holds its bytes (`GetFileAttributesW` shows no `OFFLINE` / `RECALL_ON_OPEN` / `RECALL_ON_DATA_ACCESS`; reading attributes opens no stream, so it starts no download). A path that doesn't exist (inside an archive, on a phone or a protocol share whose path has no native form) answers no too. `get_icons` asks this BEFORE the disk tier and shows the type's icon instead (`stand_in_icon_id`: `ext:{x}` for an own-icon file, `dir` for a folder id), cached in memory under the original id only.

- **A shortcut also needs both its places local** (`shortcut.rs`): its icon location and target, read through `IShellLinkW` + `IPersistFile::Load`, `GetIconLocation`, and `GetPath(SLGP_RAWPATH)`, ❌ never `Resolve` (which searches for a moved target, network included). `%VAR%`s expand first. A shortcut that fails shows the `.lnk` type icon. This check is COM, so it runs at fetch time on the icon thread, not in the cheap `may_read_own_icon`.
- Known gap: a junction in a PARENT folder (`C:\x` pointing at `\\nas\share`, then `C:\x\tool.exe`) passes, and `GetFileAttributesW` on such a place reaches the share. Catching it would cost a check per ancestor. The bounded fetch (below) keeps a hang there to that one icon.
- Special folders work through the same path: `real_path_for_real_folder_id` hands their location over as app text (`of_native`), since Windows names it `C:\Users\…`.

**The shortcut arrow (`overlay.rs`).** Explorer draws a link (`.lnk`, `.url`, `.appref-ms`: any type whose registration carries `IsShortcut`) with the small arrow, and `GetImage` doesn't: overlays are the view's job, and no `SIIGBF_*` flag asks for them. So `shell_image::render` asks the Shell item for `SFGAO_LINK` (answered from the type, no file read) and composites the stock `SIID_LINK` overlay (a full-size icon with the arrow in its corner, read at 32 px from the location `SHGetStockIconInfo` names, which honors a user's `Shell Icons` override) over the image, Porter-Duff "over". `render_plain` is the Shell's image alone. This covers real links and the `ext:lnk` / `ext:url` type samples alike, so a shortcut Cmdr may not read shows the generic shortcut icon WITH the arrow; changing those bounded keys' pixels took `CACHE_SCHEMA` 4. Pinned by `the_shells_image_of_a_link_has_no_arrow_and_ours_does` (which also pins the premise: the link's plain image equals its target's, so the arrow is never drawn twice).

- Decision/Why symlinks and junctions don't get it: the FE already draws Cmdr's link badge on every symlink row on every OS (`FileIcon.svelte`'s `.symlink-badge`), with no lookup. The arrow on top would mark the same row twice; parity with Explorer's arrow there is David's call (`#942` task 41).
- A stock icon location that `place::may_read` refuses (an override on `\\host\…`) draws nothing: the link shows without its arrow, and no connection opens.

**`desktop.ini` folders (`desktop_ini.rs`).** Threat model: **a listed folder is attacker-controlled input.** An extracted archive (7-Zip restores the read-only and system attributes) or a mounted VHDX (a "fixed" drive) can hold a folder whose `desktop.ini` names `\\host\…`, and a Shell asked for that folder's icon opens an SMB connection just because it was listed, which can offer the user's NTLM credentials. So:

- **What Cmdr never hands to the Shell:** a plain folder (not special, not a drive root), ❌ ever. Its icon comes from `overlay::extract_icon` (`SHDefExtractIconW`) on the one icon file Cmdr vetted, so no second reader of `desktop.ini` can decide differently. (An earlier cut vetted the file and then asked the Shell, which reads the FIRST key where Cmdr kept the last: `IconResource=\\attacker…` then a local line passed.)
- **What Cmdr reads:** the folder's attributes (read-only or system, Windows' own "customized" mark), then its `desktop.ini` as `GetPrivateProfileString` would: the first `[.ShellClassInfo]` section, the first line of each key (an empty first `IconResource=,0` names nothing), `IconResource=` over `IconFile=` + `IconIndex=`. Refused, showing the plain folder: a file over 64 KiB (`MAX_LEN`, so no key hides past the cap), any `CLSID` / `CLSID2` / `UICLSID` key in any section (`CLSID2={0AFACED1-…}` plus a `target.lnk` is a folder shortcut the Shell resolves), an icon file that isn't an icon kind (`.ico`, `.dll`, `.exe`, …), and any `.` or `..` step.
- **Which icon files pass (`vet_icon_file`):** a relative name inside the folder with no reparse point on any step from the folder to the file (checked step by step with `GetFileAttributesW`, which never follows a link, so nothing past a junction to a share is read), or an absolute path under the Windows or Program Files folders as the known-folder API names them, ❌ never as the text does. Everything else, a UNC path included, is the plain folder.
- **No `desktop.ini` icons on removable drives** (`place::on_fixed_drive`): one rule with the drive rows' stock icon for a stick.
- The visible-folder ask (`custom_folder_icon_ids` → `per_path::custom_folder_icon`, which asks the `platform::draws_own_folder_icon` seam before any xattr; a bool, so `platform` never depends on `per_path`, a module cycle) answers `Artwork` for a vetted folder, and the fetch (`own_icon`) vets again, answering `None` for a folder that no longer qualifies so the row keeps the gold `dir`.
- Decision/Why the attribute is read per visible folder, not carried on `FileEntry`: the lister's find data holds it, but plumbing a new IPC field through every entry costs more than the ~50 `GetFileAttributesW` calls on the visible set, which `place::local_attributes` makes anyway for the no-touch rule.
- Special folders (Documents and friends) keep coming from their known location through the Shell (`special:`), never from `desktop.ini`.
- Tinting: a customized folder draws untinted, like macOS artwork (`Artwork` keeps it out of the filter). The standard folders (`special:*`) stay out of it on Windows too: the `standardFolderIcons` convention (`ownColors` on Windows, `accentTinted` elsewhere, `src-tauri/src/platform/conventions.rs`) gates them in `FileIcon.svelte`, since Windows draws them as full-color artwork the grayscale-first filter flattened into plain gold folders (#1479). So under Cmdr gold on Windows only the generic `dir` / `symlink-dir` turn gold; whether even those should is David's open call (#942, Needs David 5).
- The disk tier's token on Windows is the later of the folder's and its `desktop.ini`'s mtime (`icon_source_modified`): the Customize tab rewrites the file in place, leaving the folder's mtime alone.
- Real-Windows check (#942 task 41): an extracted archive holding a folder whose `desktop.ini` names a UNC icon opens no connection (`net use` and a packet capture before and after listing it).

**Drive icons (`apps/desktop/src-tauri/src/volumes/platform/windows/drive_icons.rs`).** The switcher's rows carry `LocationInfo.icon`, as on macOS. Only the system drive asks the Shell (its Windows logo), on its own 8 MB thread (`system_drive_icon`, waited on for 400 ms; the answer is kept for the run, and one that lands late republishes the list through `emit_volumes_changed`). Every other drive shows its kind's stock icon (`SHGetStockIconInfo`) with no call to it, so nothing on the `list_locations` thread touches a non-system drive:

- Another fixed drive: Decision/Why stock: external USB disks and mounted downloaded VHDXs report as fixed, and the Shell would parse their `autorun.inf` `icon=` or root `desktop.ini`, either of which can name a UNC path. The cost is that a second internal disk shows the generic drive.
- A share (mapped or UNC): a dead or sleeping server hangs any call.
- An optical drive: the Shell reads the disc's `autorun.inf`, spinning it up.
- A removable drive: its `autorun.inf` icon is a file off a stick anyone could hand over.
- A `subst` letter and a folder mount.

A favorite row asks for its `special:` id (through `get_icons_within`) only when it lies on the system drive, so a Desktop redirected to `D:` or a share is never touched; it shows no icon there. Only `list_locations` fills icons, never `resolve_path_volume_fast` (per navigation).

**What the new keys do to the caches.** A `desktop.ini` folder is one `path:` entry per customized folder seen, bounded by `PATH_KEY_CAP` in memory and `MAX_ENTRIES` on disk. The system drive's icon is one value kept for the run; stock icons live in a per-run map of at most six entries; neither touches the disk tier.


**One slow place costs only its own icon (`per_place_fetch.rs`).** `get_icons` runs under the IPC command's 2 s deadline, and a timed-out call answers NOTHING, so a real-place lookup stuck on a dead share used to blank the whole batch (types included), and the frontend retries only once. So `get_icons` resolves the type ids first, then hands real places to `per_place_fetch`, which runs each on its own detached thread and waits only for what's left of a 1.5 s budget (`PER_PLACE_BUDGET`). A lookup that outlives the wait keeps running and caches its icon (memory, plus disk where allowed) when it lands, so the next request hits it. At most `MAX_IN_FLIGHT` (16) run at once, and a place already in flight isn't asked again, so stuck threads can't pile up with every visit. Pinned by `per_place_fetch_test.rs`.

## Cache tiers and persistence

`mod.rs` holds the fetch paths; the hot in-memory tier (`IconCache`, its LRU, the `clear_*` entry points) lives in
`memory_cache.rs`, the warm on-disk tier in `disk_cache.rs`.

`dir` / `ext:*` / `file` / `symlink*` / `special:*` are inherently bounded, so they're uncapped in the in-memory cache
and persist to localStorage on the FE. `path:*` / `pkg:*` are unbounded (grow with folders visited), so they're LRU-
capped (`PATH_KEY_CAP`, 256) and never persisted to localStorage. The Rust side keeps a persistent on-disk warm tier
for the real-folder ids (`special:*` / `pkg:*` / `path:*`), keyed by folder mtime (see § Persistent on-disk cache).

`clear_directory_icon_cache` drops the keys macOS appearance-tints (`dir`, `symlink-dir`, `path:*`, `pkg:*`,
`special:*`) plus the whole disk cache, on a theme/accent change.

**Gotcha: changing how a BOUNDED icon is produced needs a `CACHE_SCHEMA` bump** (`#lib/icon-cache`). Those keys persist
to localStorage and are refetched only on a miss, so an install that already ran the old build keeps serving the old
pixels forever — the fix ships, nothing moves on screen, and it reads as the fix not working. It bit the `dir`-sampling
fix below: the backend was correct, every machine still drew the house. The stamp lives in the persisted envelope
(`{ version, icons }`); a mismatch or a pre-stamp bare map discards the lot and refetches. ❌ Never leave it alone
because "the key didn't change" — the key not changing is exactly the hazard.

## Tier A: the generic folder sample (`folder_sample_path`)

`dir` / `symlink-dir` cover ~99% of rows, and their icon comes from asking the OS about one stand-in directory.

**Decision: the stand-in is an empty `<temp>/cmdr-icon-samples/sample-folder`, ❌ never the home directory.** Sampling
`~` looked free (it always exists, no directory to create) but macOS bakes the home folder's house badge into the
bitmap, so every plain folder in every listing rendered with a house on it; a custom icon assigned to `~` leaked onto
all of them by the same route. A folder Cmdr just created carries neither, so it yields the true generic glyph, still
correctly accent- and appearance-tinted because macOS tints from system state rather than from the path. Pinned by
`the_generic_folder_sample_is_a_cmdr_owned_directory_not_the_home_dir`.

## Tier A: extension samples (`icon_sample_path`)

An `ext:{x}` icon comes from asking the OS about a real file, so Cmdr keeps one empty stand-in per extension. Only the
suffix matters, and the file stays empty on purpose so nothing content-sniffs it into a different icon.

**Decision: samples live in `<temp>/cmdr-icon-samples/sample.{ext}`, not loose in the temp root.** A bare
`<temp>/cmdr_icon_sample.{ext}` is a fixed name any other process can occupy, and it strands one file per extension ever
seen in the temp root forever. A directory keeps them namespaced and removable as a unit. Reuse across runs is
deliberate: re-creating an empty file every launch buys nothing. Pinned by
`extension_samples_live_in_a_cmdr_owned_directory`.

## Tier B: special system folders (`special_folders.rs`)

The finite set: Downloads, Desktop, Documents, Movies, Music, Pictures, Public, the home folder, plus (macOS only)
Applications and the Trash. Detected by canonical path, NOT by name: a folder merely named "Downloads" under
`~/Projects/` is not the real one and stays `dir`. The real paths are resolved once at startup via the `dirs` crate
(`/Applications` and `~/.Trash` are hardcoded; `dirs` has no entry for them). `classify` is a lexical-path `HashMap`
lookup with no disk I/O (no `canonicalize`, which would block on a dead mount), so it's cheap per entry during listing.

`get_icons` re-keys each uncached `special:*` id to its real path, fetches via the 8 MB `fetch_path_icons` thread (the
real folder can be iCloud-synced and descend into `fileproviderd`; see `file_system/CLAUDE.md` § Gotchas), then caches
under the bounded `special:{name}` key. The FE renders the fetched icon and falls back to the generic `dir` glyph while
the fetch is pending, FDA-gated, or timed out: purely additive.

Symlinks to a special location keep `symlink-dir` (the link badge is the salient signal; following the link to classify
would cost a syscall per entry).

## Tier C: genuinely per-path icons (`per_path.rs`)

Packages and custom-icon folders, both unbounded by nature, so the expensive NSWorkspace fetch is gated to folders that
actually deviate, detected cheaply. Two signals with deliberately different detection timing:

- **Packages** (`Safari.app`, `Foo.bundle`, …): `cmdr_fs::file_extension::is_listed_package` is a pure, no-I/O suffix
  check against the product's one package list. Cheap enough to run for every entry, so `get_icon_id` routes a listed
  package straight to a `pkg:{path}` key during listing. A dotted folder the list doesn't know gets the same key once
  macOS calls it a package on a visible row (`apps/desktop/src-tauri/src/file_system/packages.rs` → `FileEntry::set_package`), arriving as a row
  update. `.app` icons are per-app (each distinct), so the key carries the full path; they can't share a bounded
  `special:`-style key.
- **Custom-icon folders**: the `kHasCustomIcon` flag (`0x0400`) in the folder's `com.apple.FinderInfo` xattr (one
  `getxattr`, no NSWorkspace, no TCC). `custom_folder_icon` needs syscalls, so it is NOT run during bulk listing (a
  `getxattr` per directory in a 100k-entry listing would regress the hot path). Instead the FE asks about the bounded
  set of visible directory paths via `get_custom_folder_icon_ids` (→ `icons::custom_folder_icon_ids`), which runs the
  `getxattr`s only for those and returns a `CustomFolderIconId { iconId: "path:{dir}", followsAccent }` for each folder
  that truly has the flag. The `finder_info_has_custom_icon` byte-buffer parser is split out pure for testing (flag at
  offset 8, big-endian `u16`).

**Which custom icons get the gold recolor (`CustomFolderIcon`, issue #110).** macOS 26+ Finder's Customize Folder
draws an SF symbol or an emoji on the plain folder, tinted with the system accent, so under Cmdr gold such a folder
looked like the one accent-colored folder in a gold list. Three kinds, told apart from on-disk metadata alone (verified
on macOS 27.0.1 by rendering `NSWorkspace iconForFile:` for folders built with `xattr -w`, `NSURL` tag writes, and
`NSWorkspace setIcon:`, and by reading IconServices' `ISFolderIconConfiguration initWithURL:`, 2026-10-29):

- **`AccentTinted`** (recolor): `kHasCustomIcon` plus the `com.apple.icon.folder#S` xattr (`{"sym":…}` or
  `{"emoji":…}`), and no colored tag. The exact name matters: IconServices ignores a bare `com.apple.icon.folder`.
  Without `kHasCustomIcon`, macOS draws neither the symbol nor artwork.
- **`TagTinted`** (keep): the same, plus a tag with a color (`_kMDItemUserTags` entry `"Name\nN"`, `N` ≠ 0). Customize
  Folder's color dots ARE Finder tags, so this is the color the person chose. Global
  `AppleDisableTagBasedIconTinting` (Finder > Settings > Tags > "Tint folders based on tags" off) turns it back into
  `AccentTinted`, so the classifier reads it, but only for this case. Legacy `FinderInfo` label bits don't tint.
- **`Artwork`** (keep): `kHasCustomIcon` without the customization xattr, the pasted-image `Icon\r` case. When both a
  customization and an `Icon\r` exist, macOS draws the symbol, so the xattr's presence alone decides.

The FE keeps the flag beside the `path:` key (`isAccentTintedCustomFolder`, dropped with the key) and `FileIcon` adds
the filter. A symbol is drawn in a darker shade of the accent, so the grayscale-first filter recolors it cleanly as one
bitmap.

**Emoji folders under gold are two layers (#814).** The filter grays the whole bitmap first, so on the macOS bitmap the
emoji turned gold-sepia with the folder. So `classify_custom_folder_icon` parses the xattr value
(`customization_emoji`: `{"emoji":"🍕"}` → `"🍕"`; a symbol, bad JSON, or text that can't be one emoji → `None`) and
`AccentTinted { emoji }` reaches the FE as `CustomFolderIconId.emoji` (`getAccentTintedCustomFolderEmoji`). Under gold,
`FileIcon` then draws the plain `dir` icon under the filter and the emoji as text in a sibling `.folder-emoji` span the
filter never reaches. Gold off, it draws macOS's own bitmap as before.

- Decision/Why FE layers, not a Rust composite: the gold recolor IS a CSS filter, so a Rust-side composite would have
  to reimplement it in pixels and produce gold-dependent bitmaps under a key that doesn't vary with gold. Two layers
  reuse the already-cached, variant-correct `dir` base (Increase contrast, dark mode) and change no cached pixels, so
  no `CACHE_SCHEMA` bump and no new epoch-guarded write.
- The span's size and nudge match macOS: at the 32 px fetch, both put a 🙂 at 16×15 px from (8, 9) (verified on macOS
  27.0.1 by diffing an `NSWorkspace iconForFile:` render against the span in Chrome at 2× DPR, 2026-10-29). macOS
  scales the emoji non-linearly with icon size (half the icon at 32 px, a third at 256 px), so the ratio holds for
  the list's 16 pt icon only.

Both flag and pixels share the
`path:` key's lifecycle, so a tag change or a Finder tint-switch flip shows on the next re-detection (navigating back),
not live; neither bumps the folder's mtime, so the disk tier can also serve pre-change pixels until the next accent or
theme flush.

**Why the detection split (perf decision)**: the bulk `list_directory` path runs `get_icon_id` per entry. The package
suffix check is free (string op, no syscall), so it stays inline. The custom-icon `getxattr` is a syscall per dir, so
it's deferred to the bounded visible set. Net: a 100k-entry directory pays zero extra syscalls for custom-icon
detection during listing; the cost is bounded to the ~50 visible rows.

**An icon that was never fetched from the OS** still reaches the frontend in the same base64 WebP form:
`rgba_to_data_url(rgba, w, h)` encodes a raw buffer at `ICON_SIZE`. Its one caller is the "open terminal here" app list
(`../file_system/terminal/platform/macos.rs`), which reads each app's `.icns` straight out of its bundle rather than asking NSWorkspace,
so it needs no TCC permission and can't descend into a FileProvider XPC chain. Nothing here caches it: that list is a
handful of apps, rendered when the settings row opens. It's `#[cfg(target_os = "macos")]` for the same reason its caller
is: `.app` bundles are a macOS thing, and an ungated copy is dead code in the Linux build.

**Volumes** carry their own per-path icon through a separate, already-wired path: `volumes/platform/macos/mod.rs` calls
`icons::get_icon_for_path` at volume-enumeration time and stores the data URL directly on the volume struct (FDA-gated,
returns `None` while pending). Independent of the `iconId` registry used for file-list rows, so no Tier-C wiring is
needed for volumes.

`get_icons` treats every real-folder id uniformly: `real_path_for_real_folder_id` maps `special:{name}` → its resolved
location and `pkg:{path}` / `path:{path}` → the embedded path, fetches each via the 8 MB `fetch_path_icons` thread, and
re-keys the result back to the original id. `pkg:*` shares the `path:*` lifecycle: both match `is_per_path_key`, are
LRU-capped together under one `PATH_KEY_CAP` budget, and are never persisted to localStorage.

**FE wiring** (`file-explorer/views/file-list-utils.ts` + `icon-cache.ts`): the visible-range fetch collects the
on-screen directory rows' paths and calls `prefetchCustomFolderIcons` → `get_custom_folder_icon_ids`, then fetches the
returned `path:` ids through the normal `prefetchIcons` path (packages already arrive as `pkg:` ids from the listing).

**Gotcha: a custom-icon folder's entry still carries `iconId: "dir"`**, because the detection is deferred and nothing
rewrites the id afterwards. So the RENDERER has to ask by path — `FileIcon.svelte` tries
`getCachedCustomFolderIcon(entry.path)` before `getCachedIcon(entry.iconId)`. Looking up `iconId` alone drew the generic
folder over an icon the prefetch had already fetched and cached: the whole chain worked and the result was invisible,
with no error anywhere. The gold-recolor filter has to check the same thing, since `isFolderIcon` is true for these rows
too and would repaint the user's artwork; it applies only when `isAccentTintedCustomFolder(path)`. Packages don't share the hazard — `pkg:` ids come straight off the listing.
`FilePane` evicts a directory's `path:*` / `pkg:*` keys via `evictPerPathIconsForDir` when its listing ends (navigation
away / unmount), keeping the working set tight and re-detecting a re-icon next time the folder is shown.

## Persistent on-disk cache (`disk_cache.rs`)

**Capped at `MAX_ENTRIES` (4,096), oldest written dropped first**: every 64th store lists the directory and prunes
(`prune_in`). Folders alone rarely reach it, but on Windows every program and icon file visited is an entry of its own.

Real-folder icons (`special:*`, `pkg:*`, `path:*`) rarely change, so they persist across restarts in a warm on-disk
tier under `<data_dir>/icon-cache/` (env-resolved via `CMDR_DATA_DIR`, like the secret store). Each entry is a small
JSON sidecar named by an FNV-1a digest of the icon id (so arbitrary path characters never produce an unsafe filename),
holding `{ token, data_url }`.

**Staleness token = the folder's own mtime** (whole epoch seconds). On a hot-cache miss, `get_icons` calls
`disk_cache::load` BEFORE the cold NSWorkspace fetch; a hit promotes the icon into the in-memory LRU. When the user
re-icons a folder in Finder, the folder's mtime bumps (Finder rewrites the icon resource / `com.apple.FinderInfo`), so
the stored token no longer matches and we re-fetch: durability plus correct invalidation without watching anything. A
missing/corrupt sidecar, an unresolvable mtime (dead mount), or any I/O error is a graceful miss; writes are temp+rename
atomic and best-effort.

**The display variant is part of each entry's name.** macOS draws a different icon under Increase contrast or Invert
colors, which the mtime can't see either, so `entry_path` digests `{id}#{variant}` (the standard look keeps the bare-id
name, so entries from before variants stay valid). Both looks stay warm, and toggling back is instant.

**Theme/accent change wipes the disk cache too** (`disk_cache::clear_all`, called from `clear_directory_icon_cache`):
macOS tints folder glyphs by appearance, which the mtime token can't catch (the folder didn't change, the system did),
so we drop the warm tier wholesale and let icons re-fetch with the new tint. The tier (in-memory hot LRU → on-disk warm
→ NSWorkspace cold) keeps the common case instant while staying honest about appearance and re-icon changes.

## Flushes and in-flight fetches

Three things flush icons: a theme/accent change (`clear_directory_icon_cache`, from the FE on `accent-color-changed`),
the "use app icons for documents" setting (`clear_extension_icon_cache`), and a change of `IconVariant`
(`apply_display_accessibility`, called by `display_accessibility.rs` at startup and on every display-options
notification, BEFORE it emits `display-accessibility-changed`, so the FE's refetch already gets the new look).

**Gotcha/Why the epoch guard**: a fetch can outlive the flush meant to drop it: a real-folder lookup through
`fileproviderd`, a task the 2 s IPC deadline abandoned that keeps running, or the FE's 5 s timed-out retry. A macOS
display-options toggle posts `NSSystemColorsDidChangeNotification` (our accent flush) about 300 ms BEFORE the
display-options notification, so the accent flush's refetch was routinely still in flight when the variant flush ran.
Written back blindly, it put the previous look over the fresh icons a few seconds after the switch, and every later
request served it. So `IconCache` carries a `generation` that every flush bumps; `get_icons` snapshots a `CacheEpoch`
(generation + variant) before asking macOS, and `cache_icon` refuses the write (and the disk store is skipped) when the
generation moved. The FE mirrors it: `icon-cache.ts` bumps `cacheEpoch` synchronously at the start of every flush, and
`applyIconsToCache` drops results fetched under an older one. Pinned by
`a_fetch_started_before_a_flush_is_not_cached_after_it` and the FE `icon-cache display variant` tests.

The FE side follows the variant itself (`initIconDisplayVariant`, main window only): its persisted envelope records the
variant, so a cache written before a change made while Cmdr was closed is dropped at startup.

The pure `load_in` / `store_in` (explicit cache dir) underpin the public `load` / `store` (process-wide `CACHE_DIR`),
so tests run hermetically against a temp dir.
