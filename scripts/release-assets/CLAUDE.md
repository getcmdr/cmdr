# Release assets

The release asset manifest: one row per file a release publishes (os, arch, kind, file, updater key, optional, Rust
targets), and the CLI the release pipeline's shell steps call (`../release-assets.sh`). The flow it drives:
`docs/guides/releasing.md` § The release asset manifest.

## Module map

- `manifest/manifest.go`: the rows, `Validate`, and the derived lists (updates, downloads, binaries, SBOMs, subjects).
  `release-finish`, `release-mirror`, and `release-evidence` import it (`replace` in their `go.mod`).
- `plan.go` (the `guard` job's step outputs), `latest.go` + `jsonout.go` (`latest.json`), `main.go` (the CLI).
- `pipeline_test.go` + `workflow_test.go`: the dry run of `release-pipeline.yml` against v0.61.0 (`testdata/`).

## Must-knows

- **A new platform is rows, plus the jobs that build, vet, and attest it.** `plan` fails until `vet-update-archives`
  vets the OS (`-vetted`) and `attest` has a step per SBOM (`-attested`); `Validate` fails a build with no Rust SBOM.
- **`latest.json` stays byte-stable**: `jsonout.go` prints what `jq --indent 2` printed, and the update rows run
  universal first, the platform order of every manifest so far. Tests hold it to v0.61.0's published bytes.
- **An optional file is a glob in `actions/attest`'s `subject-path`** (`subjectPattern`): the action fails on a literal
  path that matches nothing.
- **Changed a release job? Run `go test ./...` here.** A moved output fails against `testdata/golden/`; review it, then
  `go test -run TestReleasePipelineDryRun -update`. The fakes in `testdata/fakebin/` fail on any call they don't know.
- ❌ Don't drop `dmgSizes` from `latest.json` before the release after the first one with `downloads`: website builds
  from before read it.

Design, the dry run's reach, and what only a real release proves: `DETAILS.md`.
