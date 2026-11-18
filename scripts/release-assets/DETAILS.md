# Release assets: details

## The rows

Each `manifest.Asset` is one published file. Its roles come from two places:

- **Download** (people fetch it: `checksums.txt`, the EU mirror, `latest.json`'s `downloads`) comes from the kind:
  `dmg`, `pkg`, and the Linux and Windows packages (`appimage`, `deb`, `nsis`, `msix`).
- **Update payload** (signed, in `latest.json`'s `platforms`, vetted, mirrored with its `.sig`) comes from a non-empty
  `UpdaterKey`. One file can be both: the Windows NSIS `-setup.exe` and the Linux AppImage are their OS's update
  payload.
- **SBOMs** are rows too (`rust-sbom`, `frontend-sbom`). A Rust SBOM describes one target triple; it's bound to every
  build whose `Targets` share it, so the universal macOS build and the pkg carry both macOS SBOMs. The frontend SBOM is
  bound to every build.

Filenames are data (`File` with `{version}`), never derived from os and arch: Tauri's bundler spells the Intel arch
`x64` in filenames while the rest of the repo says `x86_64`, and SBOM names for a new OS must not collide with macOS's.

Decision: one flat row per file, not a build with nested files. Why: every consumer asks a per-file question (sign it?
hash it? mirror it? attest it against what?), and a Windows row is then data alone. `Validate` catches the mistakes a
new row can make (a duplicate name or updater key, an optional update payload, a build no SBOM describes, an updater key
the app doesn't read for that OS and arch).

## Update payloads off macOS

The app's updater (`apps/desktop/src-tauri/src/updater/DETAILS.md` § Windows and Linux) asks for these and refuses the
release without them:

- **The signature names the version.** A Windows or Linux payload (`File.SignsVersion()`) is signed with
  `tauri signer sign --app-version <version>`, which writes `version:<v>` into minisign's signed trusted comment; the
  updater can't open an installer before it runs, so it binds the version there and refuses a signature without it.
  macOS archives are signed without it (the bundle's `Info.plist` binds the version).
- **The key is one the app reads**: `windows-x86_64-nsis` then `windows-x86_64`, `linux-x86_64-appimage` then
  `linux-x86_64` (`install_kind.rs`'s `platform_keys`, mirrored by `updaterKeys`). The manifest uses the bare
  `windows-x86_64` / `linux-x86_64`.
- **An optional signed advisory list**: a top-level `securityAdvisories: { url, signature }` in `latest.json`, the list
  published as a release asset with its own minisign signature. Without it Windows and Linux show no security notice;
  the update still installs.

Wired so far, ahead of any Windows or Linux row: `release-finish` passes `--app-version` for such a row (`signerArgs`),
and the validator refuses a signature whose trusted comment lacks `version:<v>` for one (`signatureNamesVersion`), so a
version-less signature from either signing mode fails `publish` instead of reaching `latest.json`. Left for Windows port
plan task 52: the rows, the flag in the CI `sign` job (`ci` mode), and the advisory list. The reader's
`every_field_the_reader_names_is_one_the_writer_writes` (`crates/cmdr-update-manifest/src/tests.rs`) holds that last gap
open on purpose: it lists `securityAdvisories` as not written yet, against `testdata/v0.61.0/latest.built.json`, the
fixture both sides share.

## Who reads it

- **`guard`** runs `release-assets.sh plan` once, and its outputs feed `sbom` (which SBOMs to generate),
  `vet-update-archives` (the macOS payloads), `publish` (which signatures must exist), and `attest` (the required
  builds, and each SBOM's subjects). Those jobs need no Go. `guard` already had Go for the mirror preflight.
- **`publish`** runs `latest-json` and `checksum-files` (it installs Go for the mirror step anyway).
- **The Go tools** import the package: `release-finish` signs and verifies `Updates()`, `release-mirror` mirrors
  updates, signatures, and downloads and checks `latest.json` against them, `release-evidence` derives the expected
  asset list.
- **The website** reads `latest.json`'s `downloads` (`apps/website/src/lib/release-downloads.ts`), falling back to
  `dmgSizes` for manifests from before it.

Not driven by it: the `compile` and `sign` matrices and the `sign` job's staging (bundler output paths are bundler
knowledge; `publish`'s "every URL is an asset" check and `attest`'s presence check catch a drifted name), `bump-tap`
(Homebrew wants the universal DMG), and the api-server's `/download` route (Windows port plan task 31).

## `latest.json`

Members in this order: `version`, `dmgSizes`, `notes`, `pub_date`, `platforms`, `downloads`. Everything before
`downloads` is the bytes v0.61.0 published (`latest_test.go`, and the dry run's `requirePublishedPrefix`), so the move
from the jq step changed nothing an installed updater or an old website build reads. `jsonout.go` escapes like jq
(`jvp_dump_string`): `encoding/json` would escape U+2028, U+2029, and `<>&` and leave DEL bare
(`TestWriteStringMatchesJq` runs the real jq). A required download or update payload missing from the release, or a
missing signature, is an error; the old step wrote a 0 size for a missing DMG.

`dmgSizes` stays one more release after the first with `downloads` (Windows port plan task 30): a website build from an
older commit (a rollback, or a deploy of an old `main`) reads only `dmgSizes`. Drop it in the release after, along with
the fallback in `release-downloads.ts` and in `apps/website/scripts/fetch-live-manifest.ts`, and from `validate.go`'s
`topLevelKeys` (the validator requires it until then).

## The `latest.json` gate

A bad manifest strands every install (a malformed `securityAdvisories` entry once would have failed every update check
on every OS), so nothing ships one. Two validators, each run twice, all in `publish` before the mirror and the Latest
mark, so a failure leaves the live manifest untouched:

- **`validateLatest`** (`validate.go`), the release side. `latest-json` runs it on the bytes it built and writes nothing
  it refuses; `validate-latest` runs it on the copy `gh release download` reads back after the upload (which must also
  `cmp` equal). It checks: at most 256 KiB of UTF-8, one JSON object with nothing after it, no member twice and no
  `null` anywhere (serde refuses a duplicate struct field, and Go reads a null into a string as `""`); exactly the
  members in `topLevelKeys` (`securityAdvisories` optional); `version` the tag's, Rust-semver, and newer than `guard`'s
  `live-version` (empty for a deliberate republish); `notes` a string (empty ones publish, with a warning from
  `latest-json`: `docs/guides/releasing.md` § The release gate); `pub_date` as `YYYY-MM-DDTHH:MM:SSZ` (the 0.7 plugin
  refuses the whole manifest over a bad one); `platforms` keyed by exactly the release asset manifest's updater keys,
  each `{ signature, url }` with the URL `<base>/<file>` on the release and non-empty, the signature the release's own
  `.sig` byte for byte, minisign-shaped, by a key the app trusts (`tauri.conf.json`'s or `updater-backup.pub`'s ID), and
  naming the version where `SignsVersion`; `downloads` exactly `manifest.Present`'s list in order with the release's
  sizes; `dmgSizes` the three DMG sizes; a `securityAdvisories` entry's URL an asset of the release and its signature by
  a trusted key. It answers every problem at once.
- **The shipped readers** (`crates/cmdr-update-manifest`, `history::gate`): the Tauri plugin of Cmdr 0.7, the reader of
  0.8–0.61, and today's, each of which must read the file and find each install's entry. Built by the `manifest-readers`
  job, run by `publish` on the candidate and the read-back. `crates/cmdr-update-manifest/DETAILS.md`.

Decision: the Rust readers run as a binary from their own job, not inside the Go validator. Why: the Go side can't parse
the way serde and the plugin do, and building Rust in `publish` would run crate build scripts beside its write token.
The two halves share fixtures (`latest.built.json`), so a writer change that a reader refuses fails a test on either
side before any release.

## The dry run

`pipeline_test.go` reads `release-pipeline.yml` with a small line-based reader (`workflow_test.go`, no YAML dependency),
runs every `run:` step of `guard`, `sbom`, `vet-update-archives`, `publish`, and `attest` (minus the few in `skipped`,
which reach outside the release) with `bash -eo pipefail`, resolves `${{ }}` expressions and step-level `if:` itself,
and fails on any expression or condition it doesn't know. `actions/attest` steps are resolved, not run: the golden
records the SBOM and the files the subject patterns match, and a literal path that matches nothing is recorded as the
error the action would raise. `RELEASE_PIPELINE_WORKFLOW=<file>` runs it against another copy of the workflow (an older
commit's). `RELEASE_PIPELINE_VERBOSE=1` logs a failing step's output.

The fixture is v0.61.0 as published: its asset list and sizes, its three signatures, `latest.json`, `checksums.txt`, and
its CHANGELOG section. Other assets are short stand-ins (the sizes in `gh api` stay real), so `checksums.txt` in the
goldens hashes stand-ins.

The fake `gh release upload` replaces the asset in the fixture release, as `--clobber` does, so the read-back reads the
upload. `guard`'s `live-version` is `0.60.0` (its step reads the live manifest, so it's `skipped`), and the
`manifest-gate` artifact is `testdata/fakebin/manifest-gate`, which only checks for JSON: the real readers run over the
same bytes in the crate's tests (`latest.built.json`).

What only a real release proves: the `sign` job's staging and upload (it runs Apple tools), GitHub's handling of
multi-line job outputs into `actions/attest` (the dry run substitutes them textually), the real `cargo cyclonedx`
output, the mirror's uploads, the `manifest-readers` build and artifact hand-off, the live-version output, and that
`gh release download` returns the uploaded `latest.json` byte for byte. The first real release after a change here is
worth watching through `publish` and `attest`.

## Gotchas

- **A `subject-path` line without `*?[` is a literal for `actions/attest`**, and a literal that matches nothing fails
  the action (`isLiteralPath` in
  [actions/attest's subject.ts](https://github.com/actions/attest/blob/main/src/subject.ts), verified by reading it,
  2026-11-18). An optional file's line is written `name[.]ext` (`subjectPattern`).
- **`release-assets.sh` builds, then runs from the caller's directory**, so relative paths in its flags hold.
