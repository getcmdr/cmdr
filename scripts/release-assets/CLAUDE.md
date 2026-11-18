# Release assets

The release asset manifest: one row per file a release publishes (os, arch, kind, file, updater key, optional, Rust
targets), and the CLI the release pipeline's shell steps call (`../release-assets.sh`). The flow it drives:
`docs/guides/releasing.md` § The release asset manifest.

## Module map

- `manifest/manifest.go`: the rows, `Validate`, and the derived lists (updates, signed files, downloads, binaries,
  SBOMs, subjects); `shipping.go`: `ShippedOSes` (the platform switch, by `Since` version); `legs.go`: core vs. the
  per-OS legs a release carries whole or not at all. `release-finish`, `release-mirror`, and `release-evidence` import
  it (`replace` in their `go.mod`).
- `plan.go` (the `guard` job's step outputs), `latest.go` + `jsonout.go` (`latest.json`), `validate.go` (the one
  `latest.json` validator), `stage.go` (`windows-publish`'s intake of the build artifact), `nsis/` (`vet-nsis`: stub,
  packed app, uninstall entry), `main.go` (the CLI).
- `pipeline_test.go` + `workflow_test.go`: the dry run of `release-pipeline.yml` against v0.61.0 (`testdata/`);
  `pipeline_windowson_test.go`: the same with Windows on (`testdata/golden/windows-on/`).

## Must-knows

- **A new platform is rows, plus the jobs that build, vet, and attest it, then its OS in `ShippedOSes`.** `plan` fails
  until `vet-update-archives` vets the OS (`-vetted`) and `attest` has a step per SBOM (`-attested`), for every row
  shipped or not; `Validate` fails a build with no Rust SBOM. Windows is wired and off: ❌ adding it to `ShippedOSes` is
  David's call, always with a `Since` (`docs/guides/releasing.md` § Windows releases).
- **A leg never holds the core**: the plan's `signed-files`, `update-archives`, and `binaries` are the core's, and no
  macOS job may need a Windows job. Every step after `publish`'s snapshot follows its `latest.json` (`-carried-by`).
- **The trusted side derives names, never reads them from an artifact**: `stage` takes exactly the manifest's files.
- ❌ Never build a release tool with `-tags cmdr_windows_on` (the Windows-on dry run's; a test guards it).
- **`latest.json` stays byte-stable**: `jsonout.go` prints what `jq --indent 2` printed, and the update rows run
  universal first, the platform order of every manifest so far. Tests hold it to v0.61.0's published bytes, and
  `testdata/v0.61.0/latest.built.json` (the Rust readers' fixture) to today's output.
- **No `latest.json` leaves without `validateLatest`**: `latest-json` runs it on what it built, `validate-latest` on the
  copy read back from the release, and `publish` runs every shipped reader (`crates/cmdr-update-manifest`) on both. A
  new member is added to `topLevelKeys` on purpose, with a history fixture there. DETAILS § The `latest.json` gate.
- **A Windows or Linux update payload signs its version** (`File.SignsVersion()`: `--app-version`), or the app refuses
  it. DETAILS § Update payloads off macOS.
- **An optional file is a glob in `actions/attest`'s `subject-path`** (`subjectPattern`): the action fails on a literal
  path that matches nothing.
- **Changed a release job? Run `go test ./...` here.** A moved output fails against `testdata/golden/`; review it, then
  `go test -run TestReleasePipelineDryRun -update`. The fakes in `testdata/fakebin/` fail on any call they don't know.
- ❌ Don't drop `dmgSizes` from `latest.json` before the release after the first one with `downloads`: website builds
  from before read it.

Design, the dry run's reach, and what only a real release proves: `DETAILS.md`. Read it before any non-trivial work
here: editing, planning, reorganizing, or advising.
