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

File names are data (`File` with `{version}`), never derived from os and arch: Tauri's bundler spells the Intel arch
`x64` in file names while the rest of the repo says `x86_64`, and SBOM names for a new OS must not collide with macOS's.

Decision: one flat row per file, not a build with nested files. Why: every consumer asks a per-file question (sign it?
hash it? mirror it? attest it against what?), and a Windows row is then data alone. `Validate` catches the mistakes a
new row can make (a duplicate name or updater key, an optional update payload, a build no SBOM describes).

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
the fallback in `release-downloads.ts` and in `apps/website/scripts/fetch-live-manifest.ts`.

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

What only a real release proves: the `sign` job's staging and upload (it runs Apple tools), GitHub's handling of
multi-line job outputs into `actions/attest` (the dry run substitutes them textually), the real `cargo cyclonedx`
output, and the mirror's uploads. The first real release after a change here is worth watching through `publish` and
`attest`.

## Gotchas

- **A `subject-path` line without `*?[` is a literal for `actions/attest`**, and a literal that matches nothing fails
  the action (`isLiteralPath` in
  [actions/attest's subject.ts](https://github.com/actions/attest/blob/main/src/subject.ts), verified by reading it,
  2026-11-18). An optional file's line is written `name[.]ext` (`subjectPattern`).
- **`release-assets.sh` builds, then runs from the caller's directory**, so relative paths in its flags hold.
