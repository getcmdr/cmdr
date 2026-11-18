# cmdr-update-manifest: details

## The reader's contract

`parse` takes the raw body and answers an `UpdateManifest` or a `ManifestRefusal`, never a panic:

- **Refused** (the app maps each to `ServerRequestError::BadResponse`, which the frontend words as an answer it couldn't
  read and logs at error): over `MAX_MANIFEST_BYTES`, not UTF-8, a body that opens with `<` (a captive portal or CDN
  error page served with a 200), JSON that doesn't parse (truncated, `version` or `platforms` missing, null, or retyped,
  a duplicate member), a `version` that isn't semver.
- **Read entry by entry**: each `platforms` value becomes `Ok(PlatformEntry)` or `Err(why)` (not `{ url, signature }` of
  strings, the URL not absolute `https`, an empty signature), with a warn. `check_manifest` fails only when THIS
  install's first matching key holds an `Err` (`ManifestRefusal::BadEntry`): the release meant to offer it something. A
  readable manifest answers a `ManifestAnswer`: `Update`, `UpToDate`, or `NoBuildForThisPlatform` (a newer release with
  no entry under any of this install's keys, ❌ never read as up to date).
- **Dropped**: a `securityAdvisories` entry that doesn't read (the list only ever adds a notice; a strict read once
  failed every install on every OS), and every member the reader doesn't name.
- **Not an update, with a warn**: a newer version with no entry for any of this install's keys. The gate makes this
  impossible to ship for a platform the release asset manifest names.

Decision: per-entry leniency rather than one strict struct. Why: the readers up to 0.61 parse every entry strictly, so a
future Windows entry's shape bug would have stopped every Mac from updating; this build pays only for what it reads.

## The history

How each shipped reader parsed, recovered from `git log --follow apps/desktop/src-tauri/src/updater/manifest.rs` and the
plugin's source (verified 2026-11-18, `tauri-plugin-updater` 2.14.1 in `Cargo.lock`, `updater.rs`
`impl Deserialize for RemoteRelease`):

- **Cmdr 0.7.1 and earlier** checked through `tauri-plugin-updater` 2. `RemoteRelease` reads `version` (a leading `v`
  trimmed), `notes` and `pub_date` as optional strings (`pub_date` RFC 3339 or the whole read fails), and `platforms` as
  a map of `{ url: Url, signature: String }`: one entry anywhere with a URL that doesn't parse fails it. Without
  `platforms` it wants a top-level `url` and `signature`.
- **Cmdr 0.8.0 to 0.61.x** (`190a637767` onward): `version: String` and
  `platforms: HashMap<String, { url: String, signature: String }>`, unknown members ignored, every entry strict. A
  non-semver version reads as no update.
- **Today's reader**: above.

Every published manifest has had the same five members (`version`, `dmgSizes`, `notes`, `pub_date`, `platforms`) until
`downloads` joined; `history/` keeps v0.7.1 (unversioned GitHub URLs), v0.35.0, and v0.54.0 (versioned names), and the
writer's fixtures cover v0.61.0 (mirror URLs) and today's build. Every reader reads every one
(`every_published_shape_reads_in_every_reader_that_shipped`).

## The gate

`history::gate(body)` runs the three readers: each must parse it, the old two must find each of `darwin-aarch64` and
`darwin-x86_64` (the only installs they shipped to) that today's reader finds too, today's must read every entry, and a
`securityAdvisories` entry must be one this build keeps. `cmdr-check-update-manifest <file>` prints one line per reader
or every refusal, exits 1 on any. The release pipeline builds it in its own read-only job (`manifest-readers`: the cargo
build runs third-party build scripts, kept away from `publish`'s write token) and `publish` runs it on the candidate and
again on the copy it reads back from the release. The release-side rules (every URL an asset, sizes, signatures, version
order) are the Go validator's: `scripts/release-assets/DETAILS.md` § The `latest.json` gate. A macOS key neither finds
is a platform the release asset manifest dropped (a reviewed change, which the Go validator holds `platforms` to): the
gate names who gets no update and lets it through (`docs/guides/releasing.md` § The release gate).

## The public surface

Nine root items (the reader, its `ManifestAnswer`, and `history`), and six in `history` (`Reader` with `ALL` and
`label`, `Reading`, `read`, `gate`), capped in `index-crate-isolation-ceilings.go`. `history` is public because the gate
binary and the app's plugin mirror test call it; nothing in the app's runtime does.

## Tests

- `tests.rs`: the contract (each refusal, per-entry leniency, the advisory entry), every published shape in every
  reader, added fields pass and removed or retyped ones fail, the field-name probe against the writer's output, and two
  proptests: damaged bytes (cut, flipped, appended) never panic in any reader or the gate, and any JSON value in place
  of an optional member or another platform's entry never costs `darwin-aarch64` its update.
- `apps/desktop/src-tauri/src/updater/manifest.rs`: the plugin mirror against the real `RemoteRelease`.
