package main

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

var windowsOn = manifest.ForOSes("0.70.0", manifest.MacOS, manifest.Windows)

func TestTheLaptopSignsTheCoreAndEachWholeLeg(t *testing.T) {
	if got, want := legSigned(windowsOn, manifest.Windows), []string{"Cmdr_0.70.0_x64-setup.exe", "Cmdr_0.70.0_security-advisories.json"}; !slices.Equal(got, want) {
		t.Fatalf("the Windows leg signs %v, want %v", got, want)
	}
	var core releaseInfo
	for _, name := range manifest.Names(windowsOn.Core().Files) {
		core.Assets = append(core.Assets, releaseAsset{Name: name, State: "uploaded"})
	}
	legs, lines := legsOnDraft(windowsOn, core)
	if len(legs) != 0 || len(lines) != 1 || !strings.Contains(lines[0], "ships without its windows files") {
		t.Fatalf("a draft without Windows files: legs %v, lines %q", legs, lines)
	}
	whole := core
	for _, name := range []string{"Cmdr_0.70.0_x64-setup.exe", "Cmdr_0.70.0_x64.msix", "cmdr-group-policy-0.70.0.zip", "Cmdr_0.70.0_security-advisories.json"} {
		whole.Assets = append(whole.Assets, releaseAsset{Name: name, State: "uploaded"})
	}
	if legs, lines := legsOnDraft(windowsOn, whole); !slices.Equal(legs, []manifest.OS{manifest.Windows}) || len(lines) != 0 {
		t.Fatalf("a draft with the whole leg: legs %v, lines %q", legs, lines)
	}
	// The last Windows file still being saved: the leg isn't on the draft yet.
	whole.Assets[len(whole.Assets)-1].State = "starter"
	if legs, _ := legsOnDraft(windowsOn, whole); len(legs) != 0 {
		t.Fatalf("a half-saved leg counted: %v", legs)
	}
}

// `-verify-dir` needs the core's signatures and checks a carried leg's when they're there.
func TestVerifyNamesTakesTheLegsWhoseSignaturesAreThere(t *testing.T) {
	dir := t.TempDir()
	core := manifest.Names(windowsOn.Core().Signed())
	if got := verifyNames(windowsOn, dir); !slices.Equal(got, core) {
		t.Fatalf("no leg signatures: %v, want the core %v", got, core)
	}
	if err := os.WriteFile(filepath.Join(dir, "Cmdr_0.70.0_x64-setup.exe.sig"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if got := verifyNames(windowsOn, dir); !slices.Equal(got, append(slices.Clone(core), "Cmdr_0.70.0_x64-setup.exe")) {
		t.Fatalf("with the installer's signature: %v", got)
	}
}

func TestTheLaptopDoesntWaitForTheWindowsJobs(t *testing.T) {
	done := func(name, conclusion string) ciJob {
		return ciJob{Name: "Release / " + name, Status: "completed", Conclusion: conclusion}
	}
	running := ciJob{Name: "Release / Build the Windows packages", Status: "in_progress"}
	jobs := []ciJob{done("Verify this release tag", "success"), done("Sign and notarize (universal)", "success"), running}
	if ok, _ := coreDone(jobs); ok {
		t.Fatal("the core counted as done before `attest` finished")
	}
	jobs = append(jobs, done(lastCoreJob, "success"), done("Publish release", "skipped"))
	if ok, failed := coreDone(jobs); !ok || len(failed) != 0 {
		t.Fatalf("the core is done while Windows builds: %v, %v", ok, failed)
	}
	jobs = append(jobs, ciJob{Name: "Release / Build the browser demo", Status: "in_progress"})
	if ok, _ := coreDone(jobs); ok {
		t.Fatal("a core job still running counted as done")
	}
	jobs[len(jobs)-1] = done("Build the browser demo", "failure")
	if ok, failed := coreDone(jobs); !ok || len(failed) != 1 {
		t.Fatalf("a red core job: %v, %v", ok, failed)
	}
}

// The job names the laptop matches are the workflow's own.
func TestTheLegJobNamesAreTheWorkflows(t *testing.T) {
	raw, err := os.ReadFile("../../.github/workflows/release-pipeline.yml")
	if err != nil {
		t.Fatal(err)
	}
	src := string(raw)
	for id, name := range map[string]string{
		"windows-build":   legJobNames[0],
		"windows-publish": legJobNames[1],
		"windows-attest":  legJobNames[2],
		"attest":          lastCoreJob,
	} {
		if !strings.Contains(src, "\n  "+id+":\n    name: "+name+"\n") {
			t.Errorf("release-pipeline.yml has no job %s named %q", id, name)
		}
	}
}
