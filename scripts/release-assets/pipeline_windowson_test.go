package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

// The release pipeline dry run with Windows switched on: the same harness and v0.61.0 fixture as
// `TestReleasePipelineDryRun`, with the release tools built `-tags cmdr_windows_on`
// (`manifest/shipping_windows_on.go`), so the day-one path (the plan, `windows-publish`,
// `windows-attest`, and the jobs a Windows leg reaches) runs in CI while nothing ships it. The
// goldens are `testdata/golden/windows-on/`; review a moved one, then
// `go test -run TestReleasePipelineDryRunWithWindowsOn -update`. `windows-build` isn't here: it
// runs Windows and the Tauri bundler.

// windowsLeg is the Windows files on a release that carries the leg whole, signatures included.
var windowsLeg = []string{
	"Cmdr_0.61.0_x64-setup.exe", "Cmdr_0.61.0_x64-setup.exe.sig", "Cmdr_0.61.0_x64.msix",
	"cmdr-group-policy-0.61.0.zip", "Cmdr_0.61.0_security-advisories.json", "Cmdr_0.61.0_security-advisories.json.sig",
}

// windowsBuild is what `windows-build` hands `windows-publish`.
var windowsBuild = []string{"Cmdr_0.61.0_x64-setup.exe", "Cmdr_0.61.0_x64.msix", "cmdr-group-policy-0.61.0.zip"}

var noLatest = []string{"latest.json"}

var windowsScenarios = []scenario{
	// The plan: every output, the Windows ones included.
	{name: "guard", job: "guard", event: "push", mode: "ci"},
	// `publish` with the leg whole on the release: Windows in `latest.json`, the checksums, and the mirror.
	{name: "publish-ci", job: "publish", event: "push", mode: "ci", drop: noLatest, add: windowsLeg, artifacts: gateArtifact},
	// The leg isn't up when `publish` looks: macOS ships alone, with a warning.
	{name: "publish-late", job: "publish", event: "push", mode: "ci", drop: noLatest, artifacts: gateArtifact},
	// Half a leg (the installer's signature missing) is no leg.
	{name: "publish-half", job: "publish", event: "push", mode: "ci", drop: noLatest, add: windowsLeg[2:], artifacts: gateArtifact},
	// A re-run keeps the legs of the `latest.json` an earlier attempt put up (macOS-only, and it
	// passes the validator), though the leg is whole now.
	{name: "publish-rerun", job: "publish", event: "push", mode: "ci", add: windowsLeg, earlierLatest: "latest.built.json", artifacts: gateArtifact},
	// The fixture's own `latest.json` predates `downloads`, so the validator refuses it: not kept.
	{name: "publish-rerun-stale", job: "publish", event: "push", mode: "ci", add: windowsLeg, artifacts: gateArtifact},
	// A re-run after a damaged `latest.json` upload: that file is never carried; the snapshot decides.
	{name: "publish-rerun-damaged", job: "publish", event: "push", mode: "ci", add: windowsLeg, damage: noLatest, artifacts: gateArtifact},
	// `local` mode's dispatch: the laptop signed the leg, so it's verified and carried.
	{name: "publish-dispatch", job: "publish", event: "workflow_dispatch", mode: "local", drop: noLatest, add: windowsLeg, artifacts: gateArtifact},
	{name: "vet", job: "vet-update-archives", event: "push", mode: "ci", add: windowsLeg},
	{name: "vet-late", job: "vet-update-archives", event: "push", mode: "ci"},
	// The trusted intake: exactly the planned files, vetted, signed, verified, uploaded.
	{name: "windows-publish-ci", job: "windows-publish", event: "push", mode: "ci", made: map[string][]string{"windows-x86_64": windowsBuild}},
	{name: "windows-publish-local", job: "windows-publish", event: "push", mode: "local", made: map[string][]string{"windows-x86_64": windowsBuild}},
	// A build hook dropped a macOS-named file into the artifact: refused by name, nothing signed.
	{name: "windows-publish-hostile", job: "windows-publish", event: "push", mode: "ci", made: map[string][]string{"windows-x86_64": append(slices.Clone(windowsBuild), "Cmdr_0.61.0_universal.dmg")}},
	// `publish` wrote `latest.json` first: the late leg isn't uploaded.
	{name: "windows-publish-too-late", job: "windows-publish", event: "push", mode: "ci", made: map[string][]string{"windows-x86_64": windowsBuild}},
	{name: "windows-attest", job: "windows-attest", event: "push", mode: "ci", before: "windows-publish-ci", drop: noLatest, made: map[string][]string{"windows-x86_64": windowsBuild}},
	{name: "sbom", job: "sbom", event: "push", mode: "ci"},
	{name: "attest-ci", job: "attest", event: "push", mode: "ci", before: "sbom", add: windowsLeg, artifacts: map[string]string{"sboms": "sbom"}},
}

func TestReleasePipelineDryRunWithWindowsOn(t *testing.T) {
	list := slices.Clone(windowsScenarios)
	for i := range list {
		list[i].windowsOn = true
		// `windows-publish` refuses once `latest.json` is up, which the fixture release carries.
		if list[i].job == "windows-publish" && list[i].name != "windows-publish-too-late" {
			list[i].drop = noLatest
		}
	}
	runScenarios(t, readPipeline(t), list, filepath.Join("testdata", "golden", "windows-on"))
}

// The Windows-on build tag is the dry run's alone: no workflow or script may build a release tool
// with it, which would ship Windows without the `ShippedOSes` line.
func TestNoReleaseToolBuildsWithWindowsOn(t *testing.T) {
	root := repoRoot(t)
	for _, dir := range []string{".github", "scripts"} {
		err := filepath.WalkDir(filepath.Join(root, dir), func(path string, d os.DirEntry, err error) error {
			if err != nil || d.IsDir() || strings.HasSuffix(path, "_test.go") || strings.HasSuffix(path, ".md") || strings.HasSuffix(path, "shipping_windows_on.go") {
				return err
			}
			raw, err := os.ReadFile(path)
			if err == nil && strings.Contains(string(raw), "cmdr_windows_on") {
				t.Errorf("%s names the dry run's cmdr_windows_on build tag", path)
			}
			return err
		})
		must(t, err)
	}
}

// windowsAsset makes a Windows file at path: the dry run's NSIS installer (v0.61.0, the repo's
// publisher: `nsis/testdata/gen.sh`), a signature shaped like the Tauri signer's by the app's main
// key (naming the version for the installer, as `--app-version` does), or a stand-in.
func (h *harness) windowsAsset(path string) {
	t := h.t
	name := filepath.Base(path)
	switch {
	case strings.HasSuffix(name, "-setup.exe"):
		copyFile(t, filepath.Join(repoRoot(t), "scripts/release-assets/nsis/testdata/dryrun-0.61.0-setup.exe"), path, 0o644)
	case strings.HasSuffix(name, ".sig"):
		signed := strings.TrimSuffix(name, ".sig")
		comment := "timestamp:1763049596\tfile:" + signed
		if strings.HasSuffix(signed, "-setup.exe") {
			comment += "\tversion:" + fixtureVersion
		}
		must(t, os.WriteFile(path, []byte(dryRunSignature(t, comment)), 0o644))
	default:
		must(t, os.WriteFile(path, []byte("fixture "+name+"\n"), 0o644))
	}
}

// dryRunSignature is a Tauri signature's shape with the app's main key ID and the given trusted
// comment: what the `latest.json` validator reads (the cryptography is `release-finish`'s to check).
func dryRunSignature(t *testing.T, comment string) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(repoRoot(t), "apps/desktop/src-tauri/tauri.conf.json"))
	must(t, err)
	var conf struct {
		Plugins struct {
			Updater struct {
				Pubkey string `json:"pubkey"`
			} `json:"updater"`
		} `json:"plugins"`
	}
	must(t, json.Unmarshal(raw, &conf))
	text, err := base64.StdEncoding.DecodeString(conf.Plugins.Updater.Pubkey)
	must(t, err)
	key, err := base64.StdEncoding.DecodeString(strings.Split(strings.TrimSpace(string(text)), "\n")[1])
	must(t, err)
	body := base64.StdEncoding.EncodeToString(append(append([]byte("ED"), key[2:10]...), make([]byte, 64)...))
	sig := "untrusted comment: signature from tauri secret key\n" + body + "\ntrusted comment: " + comment + "\n" +
		base64.StdEncoding.EncodeToString(make([]byte, 64)) + "\n"
	return base64.StdEncoding.EncodeToString([]byte(sig))
}

// reportOutputs records `guard`'s outputs, sorted, a multi-line one indented.
func (h *harness) reportOutputs(report *strings.Builder) {
	outs := h.jobOutputs["guard"]
	keys := make([]string, 0, len(outs))
	for k := range outs {
		keys = append(keys, k)
	}
	slices.Sort(keys)
	fmt.Fprintf(report, "\n# outputs\n")
	for _, k := range keys {
		v := outs[k]
		if strings.Contains(v, "\n") {
			fmt.Fprintf(report, "%s:\n  %s\n", k, strings.ReplaceAll(v, "\n", "\n  "))
			continue
		}
		fmt.Fprintf(report, "%s=%s\n", k, v)
	}
}
