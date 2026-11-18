# Release assets: details

## The rows

Each `manifest.Asset` is one published file. Its roles come from two places:

- **Download** (people fetch it: `checksums.txt`, the EU mirror, `latest.json`'s `downloads`) comes from the kind:
  `dmg`, `pkg`, the Linux and Windows packages (`appimage`, `deb`, `nsis`, `msix`), and the Group Policy zip
  (`group-policy`, arch-less, a download but no build, so no SBOM describes it).
- **Signed** (a `.sig` by the updater key, `File.Signed`): every update payload, plus the advisory list
  (`advisory-list`), which belongs to no OS (below, § Update payloads off macOS).
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

Wired for the Windows rows: `release-finish` passes `--app-version` for such a row (`signerArgs`), the `windows-publish`
job does the same in `ci` mode (from `stage`'s `signs-version`) and verifies what it signed against the app's keys
(`release-finish -verify-dir … -leg windows`, which also requires the `version:` field), and the validator refuses a
signature whose trusted comment lacks `version:<v>` for one (`signatureNamesVersion`), so a version-less signature from
either signing mode fails `publish` instead of reaching `latest.json`. `vet-nsis` (`nsis/nsis.go`) then reads the
installer without running it, since the version the signature names is all the updater checks: the stub's version
resource and ProductName, every packed copy of the app (its version and CompanyName, found by decompressing the solid
LZMA payload in `nsis/payload.go`), and the uninstall entry's strings (key, `DisplayVersion`, `Publisher`). It names the
build (a stale or mixed-up bundle fails), not its integrity: a hostile build stamps what it likes, and provenance plus
the updater signature prove origin. It also refuses an Authenticode-signed installer until task 50 pins a signer.
Fixtures: `nsis/testdata/gen.sh` (makensis, Tauri's layout); `CMDR_REAL_NSIS=<real installer>` runs it on a real
tauri-cli build (passed on the builder's `Cmdr_0.61.0_x64-setup.exe`, 2026-11-18).

The advisory list is a row too (`AdvisoryList`, no OS, `Signed`): `shipped` adds it once some shipped update payload
signs its version, `latest-json` writes `securityAdvisories` (after `downloads`, so everything before it prints as
before), and the validator then requires that member, its URL the list's on the mirror and its signature the release's
own `.sig`. `windows-publish` writes it from its own checkout (`stage -advisories`), never from the build's artifact.
While no such row ships, the reader's `every_field_the_reader_names_is_one_the_writer_writes`
(`crates/cmdr-update-manifest/src/tests.rs`) keeps listing `securityAdvisories` as not written, against
`testdata/v0.61.0/latest.built.json`, which a macOS-only release still matches; the release that turns Windows on drops
it from `NOT_WRITTEN_YET` with a fixture that carries the member.

## Shipped and wired rows

`Assets` holds every platform's rows; `ShippedOSes` (`shipping.go`, macOS only today) picks which a release publishes,
each OS from its `Since` version and, for Windows, final versions only (a prerelease gets a `::notice::` and no Windows
leg: an MSIX version is `X.Y.Z.0`). `For(version)` resolves only those, so `latest.json`, the checksums, the mirror,
`release-finish`, and `release-evidence` agree, and a version before an OS's `Since` resolves as it was published. `All`
resolves every row for `plan`'s wiring checks: the `-vetted` OSes and `-attested` SBOMs must cover the unshipped Windows
rows too, so the jobs that build, vet, and attest them stay in place while they're skipped. `plan`'s `ships-<os>` gates
those jobs (`windows-build`, `windows-publish`, `windows-attest`, and the Windows steps of `vet-update-archives` and
`attest`), and the dry run reports each skipped step, so a renamed output or a broken condition shows in the goldens.

Decision/Why: a code constant, not a repository variable. Every tool, the laptop's `release-finish` included, must agree
on what a release holds, and a constant is reviewed, versioned, and the same everywhere; a variable would be read by the
workflow alone and could flip a release's contents without a commit. Turning a platform on is then one reviewed line
plus the test that pins it (`TestWindowsShipsOnlyOnceTurnedOn`). Decision/Why `Since`: without it, flipping the constant
would change what every past release "should" hold, and the mirror, `release-evidence`, and a `local` draft tagged
before the flip would demand Windows files those releases never had.

## Legs

`legs.go` splits a release into its core (macOS's rows, the frontend SBOM) and a leg per other OS (its rows plus the
advisory list it brings). A leg is carried whole or not at all: `CompleteLegs` needs every required row (not the SBOM,
which `attest` uploads later) and, once signed, every `.sig`. Decision/Why: a macOS release must never be blocked or
failed by Windows (a security hotfix behind a cold two-hour Windows build), so no macOS job needs a Windows job, and the
release takes the leg only if it's whole when the contents are fixed:

- **`ci` mode**: `publish`'s "Generate latest.json" snapshot (`CarriedWhole`, a `::warning::` per leg left out). Every
  step after it follows that `latest.json` (`-carried-by` for the read-back and `checksum-files`, the mirror's
  `-manifest`), and a re-run passes the `latest.json` an earlier attempt uploaded (`CarriedBy`), since the mirror's
  `checksums.txt` is immutable. `windows-publish` refuses to upload once `latest.json` is up.
- **`local` mode**: the laptop's signing (`release-finish`'s `legsOnDraft`, `signWholeLegs`): it stops waiting once
  every non-Windows job is green (`coreDone`, the job names pinned to the workflow by a test), and a leg that fails
  provenance or vetting is left out with a line. `windows-attest` attests the leg as soon as it's up, so `attest`
  needn't wait for it.
- `release-evidence` takes a leg whole or not at all too: part of one is a problem.

The plan's `signed-files`, `update-archives`, and `binaries` are the core's; a leg's builds are attest subject patterns
(`subjectPattern`), which may match nothing. `stage` (`stage.go`) is `windows-publish`'s intake: exactly the manifest's
Windows rows for the version (minus SBOMs and the advisory list), each a regular file, or it fails naming every extra,
missing, or linked entry, the way `sign` takes exactly one `Cmdr` per `compiled-*` artifact.

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

**With Windows on** (`pipeline_windowson_test.go`, goldens in `testdata/golden/windows-on/`): the release tools build
with `-tags cmdr_windows_on` (`manifest/shipping_windows_on.go` adds Windows to `ShippedOSes`), so the day-one path runs
in CI with nothing shipped: the plan's outputs, `publish` with the leg whole, late, half there, on a re-run, and on a
`local` dispatch, `vet-update-archives` with and without it, `windows-publish` (`ci`, `local`, a hostile artifact, too
late), `windows-attest`, `sbom`, and `attest`. These scenarios also record `::warning::`/`::notice::`/`::error::` lines.
The Windows assets are a makensis-built v0.61.0 installer by the repo's publisher
(`nsis/testdata/dryrun-0.61.0-setup.exe`) and signatures shaped like the Tauri signer's with the main key ID (the fake
`pnpm` writes them for `tauri signer sign`). `windows-build` isn't covered: it runs Windows and the bundler.
`TestNoReleaseToolBuildsWithWindowsOn` keeps the tag out of every workflow and script.

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
