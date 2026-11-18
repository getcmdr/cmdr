package main

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

// The release pipeline dry run: runs the real `run:` scripts of `release-pipeline.yml`'s
// asset-handling jobs against a fixture release (v0.61.0 as published, `testdata/v0.61.0/`), with
// `gh`, `cargo`, `pnpm`, `git`, and `date` faked (`testdata/fakebin/`), and compares what each job
// leaves behind (uploads, signed-off calls, SBOM files, attestation subjects, step outcomes) with
// a golden file. A break in these jobs otherwise shows up only at the next real release.
//
// `go test -run TestReleasePipelineDryRun -update` rewrites the goldens; review the diff.
// RELEASE_PIPELINE_WORKFLOW=<file> runs it against another copy of the workflow (an older commit's,
// to show a change kept the outputs).

var update = flag.Bool("update", false, "rewrite the golden files")

const (
	fixtureTag     = "v0.61.0"
	fixtureVersion = "0.61.0"
	fixtureRepo    = "getcmdr/cmdr"
)

type scenario struct {
	name  string
	job   string
	event string // push or workflow_dispatch
	mode  string // RELEASE_UPDATE_SIGNING: ci or local
	// drop removes these assets from the fixture release.
	drop []string
	// before runs another scenario's job first in the same workspace (the SBOMs `attest` takes).
	before string
	// artifacts maps a `download-artifact` name to the directory (in the workspace) holding it.
	artifacts map[string]string
	// windowsOn builds the release tools with Windows shipped (`manifest/shipping_windows_on.go`)
	// and records the steps' `::warning::`/`::notice::`/`::error::` lines (`pipeline_windowson_test.go`).
	windowsOn bool
	// add puts these assets on the fixture release (`windowsAsset` makes each).
	add []string
	// starter marks these assets as still being saved (GitHub's `state: starter`).
	starter []string
	// damage cuts these assets on the release to half their bytes (a damaged upload).
	damage []string
	// earlierLatest replaces the release's `latest.json` with this `testdata/v0.61.0/` file: what an
	// earlier attempt of this pipeline uploaded (`latest.built.json`, macOS-only).
	earlierLatest string
	// made maps a `download-artifact` name to the files the scenario makes where the step puts it
	// (a build job's artifact), each made by `windowsAsset`.
	made map[string][]string
}

// Steps that reach outside the release (tag signatures, the live manifest, Cloudflare, the website
// deploy) or need a step output the dry run can't make. Every other step with a `run:` runs.
var skipped = map[string]bool{
	"Require a trusted tag signature":                true,
	"Refuse to move the update manifest backwards":   true,
	"Require write access to the EU download bucket": true,
	"Trigger website deploy":                         true,
	"Upload the provenance bundle to the release":    true,
}

// gateArtifact is `manifest-readers`' binary, which the dry run stands in for with
// `testdata/fakebin/manifest-gate` (the Rust readers' own tests read the writer's output,
// `testdata/v0.61.0/latest.built.json`).
var gateArtifact = map[string]string{"manifest-gate": "manifest-gate"}

var scenarios = []scenario{
	{name: "publish-ci", job: "publish", event: "push", mode: "ci", artifacts: gateArtifact},
	{name: "publish-dispatch", job: "publish", event: "workflow_dispatch", mode: "local", artifacts: gateArtifact},
	{name: "publish-no-pkg", job: "publish", event: "push", mode: "ci", drop: []string{"Cmdr_0.61.0_universal.pkg"}, artifacts: gateArtifact},
	{name: "publish-missing-sig", job: "publish", event: "push", mode: "ci", drop: []string{"Cmdr_0.61.0_x64.app.tar.gz.sig"}, artifacts: gateArtifact},
	// A re-run after a damaged `latest.json` upload overwrites it, as it always has.
	{name: "publish-rerun-damaged", job: "publish", event: "push", mode: "ci", damage: []string{"latest.json"}, artifacts: gateArtifact},
	{name: "vet", job: "vet-update-archives", event: "push", mode: "ci"},
	{name: "vet-missing-archive", job: "vet-update-archives", event: "push", mode: "ci", drop: []string{"Cmdr_0.61.0_universal.app.tar.gz"}},
	{name: "sbom", job: "sbom", event: "push", mode: "ci"},
	{name: "attest-ci", job: "attest", event: "push", mode: "ci", before: "sbom", artifacts: map[string]string{"sboms": "sbom"}},
	{name: "attest-dispatch", job: "attest", event: "workflow_dispatch", mode: "local", before: "sbom", artifacts: map[string]string{"sboms": "sbom"}},
	{name: "attest-no-pkg", job: "attest", event: "push", mode: "ci", before: "sbom", artifacts: map[string]string{"sboms": "sbom"}, drop: []string{"Cmdr_0.61.0_universal.pkg"}},
	{name: "attest-missing-dmg", job: "attest", event: "push", mode: "ci", before: "sbom", artifacts: map[string]string{"sboms": "sbom"}, drop: []string{"Cmdr_0.61.0_x64.dmg"}},
}

func TestReleasePipelineDryRun(t *testing.T) {
	runScenarios(t, readPipeline(t), scenarios, filepath.Join("testdata", "golden"))
}

// readPipeline parses the release pipeline (or `RELEASE_PIPELINE_WORKFLOW`'s copy).
func readPipeline(t *testing.T) map[string]*wfJob {
	t.Helper()
	for _, tool := range []string{"bash", "jq", "python3", "shasum"} {
		if _, err := exec.LookPath(tool); err != nil {
			t.Fatalf("the release pipeline dry run needs %s on PATH, as the release runners have it", tool)
		}
	}
	workflowPath := os.Getenv("RELEASE_PIPELINE_WORKFLOW")
	if workflowPath == "" {
		workflowPath = filepath.Join(repoRoot(t), ".github/workflows/release-pipeline.yml")
	}
	src, err := os.ReadFile(workflowPath)
	if err != nil {
		t.Fatal(err)
	}
	jobs, err := parseWorkflow(string(src))
	if err != nil {
		t.Fatal(err)
	}
	return jobs
}

// runScenarios runs each scenario in a fresh workspace and compares its report with
// `<goldens>/<name>.txt`.
func runScenarios(t *testing.T, jobs map[string]*wfJob, list []scenario, goldens string) {
	byName := map[string]scenario{}
	for _, sc := range list {
		byName[sc.name] = sc
	}
	for _, sc := range list {
		t.Run(sc.name, func(t *testing.T) {
			h := newHarness(t, sc)
			if sc.before != "" {
				h.runJob(jobs, byName[sc.before], &strings.Builder{})
			}
			var report strings.Builder
			h.runJob(jobs, sc, &report)
			if sc.job == "guard" {
				h.reportOutputs(&report)
			}
			h.reportUploads(&report)
			if sc.name == "publish-ci" && !sc.windowsOn {
				h.requirePublishedPrefix()
			}
			compareGolden(t, filepath.Join(goldens, sc.name+".txt"), report.String())
		})
	}
}

func repoRoot(t *testing.T) string {
	t.Helper()
	root, err := filepath.Abs("../..")
	if err != nil {
		t.Fatal(err)
	}
	return root
}

type harness struct {
	t          *testing.T
	sc         scenario
	ws         string // GITHUB_WORKSPACE, and every step's working directory
	temp       string // RUNNER_TEMP; the scripts' `/tmp/` paths are moved under it
	bin        string
	releaseDir string
	uploads    string
	log        string
	jobOutputs map[string]map[string]string // needs.<job>.outputs
	jobEnv     map[string]string            // what steps wrote to GITHUB_ENV
}

func newHarness(t *testing.T, sc scenario) *harness {
	t.Helper()
	root := repoRoot(t)
	base := t.TempDir()
	h := &harness{
		t: t, sc: sc,
		ws: filepath.Join(base, "ws"), temp: filepath.Join(base, "temp"), bin: filepath.Join(base, "bin"),
		releaseDir: filepath.Join(base, "release"), uploads: filepath.Join(base, "uploads"),
		log:        filepath.Join(base, "log.txt"),
		jobOutputs: map[string]map[string]string{},
		jobEnv:     map[string]string{},
	}
	for _, d := range []string{h.ws, filepath.Join(h.temp, "tmp"), h.bin, h.releaseDir, h.uploads,
		filepath.Join(h.ws, "scripts"), filepath.Join(h.ws, "apps/desktop/src-tauri")} {
		must(t, os.MkdirAll(d, 0o755))
	}
	must(t, os.WriteFile(h.log, nil, 0o644))

	fake := filepath.Join(root, "scripts/release-assets/testdata/fakebin")
	for _, tool := range []string{"gh", "cargo", "pnpm", "git", "date"} {
		copyFile(t, filepath.Join(fake, tool), filepath.Join(h.bin, tool), 0o755)
	}
	// The repo's other release scripts have their own tests; here they only log what they're handed.
	for _, s := range []string{"release-finish.sh", "release-mirror.sh"} {
		copyFile(t, filepath.Join(fake, "repo-script"), filepath.Join(h.ws, "scripts", s), 0o755)
	}
	// The manifest's own CLI is the code under test, so the workspace gets the real one, and the
	// upload helper the Windows publish job runs over the fake `gh`.
	for _, s := range []string{"release-assets.sh", "release-assets", "release-upload.sh", "release-nsis.sh", "release-nsis"} {
		if _, err := os.Stat(filepath.Join(root, "scripts", s)); err == nil {
			must(t, os.Symlink(filepath.Join(root, "scripts", s), filepath.Join(h.ws, "scripts", s)))
		}
	}
	fixture := filepath.Join(root, "scripts/release-assets/testdata/v0.61.0")
	copyFile(t, filepath.Join(fixture, "CHANGELOG.md"), filepath.Join(h.ws, "CHANGELOG.md"), 0o644)
	if _, ok := sc.artifacts["manifest-gate"]; ok {
		copyFile(t, filepath.Join(fake, "manifest-gate"), filepath.Join(h.ws, "manifest-gate", "cmdr-check-update-manifest"), 0o755)
	}
	// The updater keys the app trusts, which `latest-json` checks every signature's key against, and
	// the Windows config `vet-nsis` reads the publisher from.
	for _, f := range []string{"apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/src-tauri/updater-backup.pub", "apps/desktop/src-tauri/tauri.windows.conf.json"} {
		copyFile(t, filepath.Join(root, f), filepath.Join(h.ws, f), 0o644)
	}
	// The checkout's advisory list, which the Windows publish job signs: a stand-in, so a real
	// advisory doesn't move the goldens.
	must(t, os.MkdirAll(filepath.Join(h.ws, "apps/desktop/src-tauri/resources"), 0o755))
	must(t, os.WriteFile(filepath.Join(h.ws, "apps/desktop/src-tauri/resources/security-advisories.json"), []byte(`{"advisories":[]}`+"\n"), 0o644))

	h.writeRelease(fixture, filepath.Dir(h.releaseDir))
	return h
}

// writeRelease builds the fixture release in base: its files and its `gh api` view.
func (h *harness) writeRelease(fixture, base string) {
	t, sc := h.t, h.sc
	// The release: every asset v0.61.0 carries, minus the scenario's drops. The signatures,
	// `latest.json`, and `checksums.txt` are the real ones; every other file is a short stand-in,
	// while the `gh api` view keeps the real sizes. Each asset is `uploaded` (GitHub's `state`),
	// except the scenario's `starter` ones, still being saved.
	var rel struct {
		TagName string         `json:"tag_name"`
		Assets  []fixtureAsset `json:"assets"`
	}
	raw, err := os.ReadFile(filepath.Join(fixture, "release.json"))
	must(t, err)
	must(t, json.Unmarshal(raw, &rel))
	kept := rel.Assets[:0]
	for _, a := range rel.Assets {
		if slices.Contains(sc.drop, a.Name) {
			continue
		}
		kept = append(kept, a)
		real := filepath.Join(fixture, a.Name)
		if _, err := os.Stat(real); err == nil {
			copyFile(t, real, filepath.Join(h.releaseDir, a.Name), 0o644)
		} else {
			must(t, os.WriteFile(filepath.Join(h.releaseDir, a.Name), []byte("fixture "+a.Name+"\n"), 0o644))
		}
	}
	rel.Assets = kept
	for _, name := range sc.add {
		path := filepath.Join(h.releaseDir, name)
		h.windowsAsset(path)
		info, err := os.Stat(path)
		must(t, err)
		rel.Assets = append(rel.Assets, fixtureAsset{Name: name, Size: info.Size()})
	}
	for i := range rel.Assets {
		rel.Assets[i].State = "uploaded"
		if slices.Contains(sc.starter, rel.Assets[i].Name) {
			rel.Assets[i].State = "starter"
		}
	}
	if sc.earlierLatest != "" {
		copyFile(t, filepath.Join(fixture, sc.earlierLatest), filepath.Join(h.releaseDir, "latest.json"), 0o644)
	}
	// A damaged upload: the first half of the file's bytes.
	for _, name := range sc.damage {
		path := filepath.Join(h.releaseDir, name)
		b, err := os.ReadFile(path)
		must(t, err)
		must(t, os.WriteFile(path, b[:len(b)/2], 0o644))
	}
	out, err := json.Marshal(rel)
	must(t, err)
	must(t, os.WriteFile(filepath.Join(base, "release.json"), out, 0o644))
}

// fixtureAsset is one asset in the fixture release's `gh api` view.
type fixtureAsset struct {
	Name  string `json:"name"`
	Size  int64  `json:"size"`
	State string `json:"state"`
}

func (h *harness) env(outputFile, envFile string) []string {
	goflags := "-buildvcs=false"
	if h.sc.windowsOn {
		goflags += " -tags=cmdr_windows_on"
	}
	return append(os.Environ(),
		"PATH="+h.bin+string(os.PathListSeparator)+os.Getenv("PATH"),
		"GOFLAGS="+goflags,
		"GITHUB_WORKSPACE="+h.ws,
		"GITHUB_REPOSITORY="+fixtureRepo,
		"GITHUB_REF_NAME="+fixtureTag,
		"GITHUB_REF_TYPE=tag",
		"GITHUB_EVENT_NAME="+h.sc.event,
		"GITHUB_SERVER_URL=https://github.com",
		"GITHUB_OUTPUT="+outputFile,
		"GITHUB_ENV="+envFile,
		"RUNNER_TEMP="+h.temp,
		"FIXTURE_TAG="+fixtureTag,
		"FIXTURE_VERSION="+fixtureVersion,
		"FIXTURE_PUB_DATE=2026-11-13T15:59:56Z",
		"FIXTURE_RELEASE_DIR="+h.releaseDir,
		"FIXTURE_RELEASE_JSON="+filepath.Join(filepath.Dir(h.releaseDir), "release.json"),
		"FIXTURE_UPLOADS="+h.uploads,
		"FIXTURE_LOG="+h.log,
		// `local` mode builds into a draft, which the fake `gh`'s release list reports.
		fmt.Sprintf("FIXTURE_DRAFT=%t", h.sc.mode == "local" && h.sc.event == "push"),
	)
}

// resolver answers the `${{ }}` expressions the release jobs use, for one job.
func (h *harness) resolver(stepOutputs map[string]map[string]string) func(string) (string, error) {
	fixed := map[string]string{
		"github.ref_name":             fixtureTag,
		"github.repository":           fixtureRepo,
		"github.event_name":           h.sc.event,
		"github.token":                "fixture-secret",
		"runner.temp":                 h.temp,
		"vars.RELEASE_UPDATE_SIGNING": h.sc.mode,
	}
	return func(expr string) (string, error) {
		if v, ok := fixed[expr]; ok {
			return v, nil
		}
		parts := strings.Split(expr, ".")
		switch {
		case parts[0] == "secrets":
			return "fixture-secret", nil
		case len(parts) == 2 && parts[0] == "vars":
			return "", nil
		case len(parts) == 2 && parts[0] == "env":
			return h.jobEnv[parts[1]], nil
		case len(parts) == 4 && parts[0] == "steps" && parts[2] == "outputs":
			return stepOutputs[parts[1]][parts[3]], nil
		case parts[0] == "needs":
			return h.needs(parts)
		}
		return "", fmt.Errorf("the dry run can't resolve ${{ %s }}; teach harness.resolver", expr)
	}
}

// needs answers `needs.<job>.result` (every upstream job succeeded) and `needs.<job>.outputs.<k>`.
func (h *harness) needs(parts []string) (string, error) {
	switch {
	case len(parts) == 3 && parts[2] == "result":
		return "success", nil
	case len(parts) == 4 && parts[2] == "outputs" && parts[1] == "guard" && parts[3] == "update-signing":
		return h.sc.mode, nil
	case len(parts) == 4 && parts[2] == "outputs" && parts[1] == "guard" && parts[3] == "live-version":
		// The step that reads the live manifest is `skipped`; v0.60.0 was live when v0.61.0 shipped.
		return "0.60.0", nil
	case len(parts) == 4 && parts[2] == "outputs":
		if v, ok := h.jobOutputs[parts[1]][parts[3]]; ok {
			return v, nil
		}
	}
	return "", fmt.Errorf("no value for %s", strings.Join(parts, "."))
}

// runJob runs a job's steps the way a runner would: `guard` first (its outputs feed every job),
// then each step in order until one fails, writing what each did into report.
func (h *harness) runJob(jobs map[string]*wfJob, sc scenario, report *strings.Builder) {
	if _, ok := h.jobOutputs["guard"]; !ok && sc.job != "guard" {
		h.runSteps(jobs, "guard", &strings.Builder{})
	}
	h.runSteps(jobs, sc.job, report)
}

// requirePublishedPrefix holds the generated `latest.json` to the bytes v0.61.0 actually
// published: the same document, with at most new members appended after its last one.
func (h *harness) requirePublishedPrefix() {
	t := h.t
	got, err := os.ReadFile(filepath.Join(h.uploads, "latest.json"))
	must(t, err)
	published, err := os.ReadFile(filepath.Join(repoRoot(t), "scripts/release-assets/testdata/v0.61.0/latest.json"))
	must(t, err)
	body := strings.TrimSuffix(string(published), "\n}\n")
	rest, ok := strings.CutPrefix(string(got), body)
	if !ok || (rest != "\n}\n" && !strings.HasPrefix(rest, ",\n")) {
		t.Fatalf("the generated latest.json isn't v0.61.0's published one (plus appended members):\n%s", got)
	}
}

func (h *harness) runSteps(jobs map[string]*wfJob, jobID string, report *strings.Builder) {
	t := h.t
	job, ok := jobs[jobID]
	if !ok {
		t.Fatalf("the workflow has no job %q", jobID)
	}
	stepOutputs := map[string]map[string]string{}
	resolve := h.resolver(stepOutputs)
	outputs := map[string]string{}
	h.jobOutputs[jobID] = outputs
	clear(h.jobEnv)
	fmt.Fprintf(report, "# job %s\n", jobID)
	for i, step := range job.Steps {
		if skipped[step.Name] {
			continue
		}
		run, err := evalIf(step.If, resolve)
		if err != nil {
			t.Fatalf("step %q: %v", step.Name, err)
		}
		if !run {
			fmt.Fprintf(report, "\n## %s: skipped\n", step.Name)
			continue
		}
		switch {
		case step.Run != "":
			outFile := filepath.Join(h.temp, fmt.Sprintf("output-%s-%d", jobID, i))
			envFile := filepath.Join(h.temp, fmt.Sprintf("env-%s-%d", jobID, i))
			must(t, os.WriteFile(outFile, nil, 0o644))
			must(t, os.WriteFile(envFile, nil, 0o644))
			ok := h.runScript(step, resolve, outFile, envFile, report)
			out := readKV(t, outFile)
			if step.ID != "" {
				stepOutputs[step.ID] = out
			}
			maps(outputs, out)
			maps(h.jobEnv, readKV(t, envFile))
			if !ok {
				return
			}
		case strings.HasPrefix(step.Uses, "actions/attest@"):
			h.reportAttest(step, resolve, report)
		case strings.HasPrefix(step.Uses, "actions/download-artifact@"):
			h.downloadArtifact(step, resolve)
		case strings.HasPrefix(step.Uses, "actions/upload-artifact@"):
			h.reportArtifact(step, resolve, report)
		}
	}
}

func maps(dst, src map[string]string) {
	for k, v := range src {
		dst[k] = v
	}
}

func (h *harness) runScript(step wfStep, resolve func(string) (string, error), outFile, envFile string, report *strings.Builder) bool {
	t := h.t
	script, err := expand(step.Run, resolve)
	if err != nil {
		t.Fatalf("step %q: %v", step.Name, err)
	}
	script = strings.ReplaceAll(script, "/tmp/", filepath.Join(h.temp, "tmp")+"/")
	path := filepath.Join(h.temp, "step.sh")
	must(t, os.WriteFile(path, []byte(script), 0o644))
	cmd := exec.Command("bash", "--noprofile", "--norc", "-eo", "pipefail", path)
	cmd.Dir = h.ws
	cmd.Env = h.env(outFile, envFile)
	for k, v := range step.Env {
		ev, err := expand(v, resolve)
		if err != nil {
			t.Fatalf("step %q env %s: %v", step.Name, k, err)
		}
		cmd.Env = append(cmd.Env, k+"="+ev)
	}
	for k, v := range h.jobEnv {
		cmd.Env = append(cmd.Env, k+"="+v)
	}
	before := h.logLen()
	out, err := cmd.CombinedOutput()
	status := "ok"
	if err != nil {
		status = "FAILED"
	}
	fmt.Fprintf(report, "\n## %s: %s\n", step.Name, status)
	for _, line := range h.logSince(before) {
		fmt.Fprintf(report, "%s\n", h.normalize(line))
	}
	if h.sc.windowsOn {
		for line := range strings.Lines(string(out)) {
			if strings.HasPrefix(line, "::warning::") || strings.HasPrefix(line, "::notice::") || strings.HasPrefix(line, "::error::") {
				fmt.Fprintf(report, "%s\n", h.normalize(strings.TrimRight(line, "\n")))
			}
		}
	}
	if err != nil {
		// The last line a failing script prints says why; that much belongs in the golden.
		lines := strings.Split(strings.TrimSpace(string(out)), "\n")
		fmt.Fprintf(report, "error: %s\n", h.normalize(lines[len(lines)-1]))
		if os.Getenv("RELEASE_PIPELINE_VERBOSE") != "" {
			t.Logf("step %q output:\n%s", step.Name, out)
		}
	}
	return err == nil
}

func (h *harness) logLen() int {
	b, err := os.ReadFile(h.log)
	must(h.t, err)
	return len(b)
}

func (h *harness) logSince(offset int) []string {
	b, err := os.ReadFile(h.log)
	if err != nil || offset > len(b) {
		h.t.Fatalf("reading the call log: %v", err)
		return nil
	}
	s := strings.TrimRight(string(b[offset:]), "\n")
	if s == "" {
		return nil
	}
	return strings.Split(s, "\n")
}

func (h *harness) normalize(s string) string {
	s = strings.ReplaceAll(s, filepath.Join(h.temp, "tmp")+"/", "/tmp/")
	s = strings.ReplaceAll(s, h.temp, "$RUNNER_TEMP")
	s = strings.ReplaceAll(s, h.ws, "$WORKSPACE")
	return s
}

// reportAttest records what an `actions/attest` step would sign: its SBOM and the files its
// subject patterns match right now.
func (h *harness) reportAttest(step wfStep, resolve func(string) (string, error), report *strings.Builder) {
	t := h.t
	fmt.Fprintf(report, "\n## %s: attest\n", step.Name)
	for _, key := range []string{"sbom-path", "subject-path"} {
		v, ok := step.With[key]
		if !ok {
			continue
		}
		v, err := expand(v, resolve)
		if err != nil {
			t.Fatalf("step %q %s: %v", step.Name, key, err)
		}
		var matched []string
		for pattern := range strings.Lines(v) {
			pattern = strings.TrimSpace(pattern)
			if pattern == "" {
				continue
			}
			found, err := filepath.Glob(filepath.Join(h.ws, pattern))
			must(t, err)
			// `actions/attest` fails on a literal path that matches nothing (`isLiteralPath` in
			// its `src/subject.ts`); a glob may match nothing.
			if len(found) == 0 && !strings.ContainsAny(pattern, "*?[") {
				fmt.Fprintf(report, "error: Could not find subject at path: %s\n", pattern)
			}
			for _, f := range found {
				matched = append(matched, strings.TrimPrefix(f, h.ws+"/"))
			}
		}
		slices.Sort(matched)
		matched = slices.Compact(matched)
		fmt.Fprintf(report, "%s:\n", key)
		for _, m := range matched {
			fmt.Fprintf(report, "  %s\n", m)
		}
	}
}

func (h *harness) downloadArtifact(step wfStep, resolve func(string) (string, error)) {
	t := h.t
	if files, ok := h.sc.made[step.With["name"]]; ok {
		dst, err := expand(step.With["path"], resolve)
		must(t, err)
		if !filepath.IsAbs(dst) {
			dst = filepath.Join(h.ws, dst)
		}
		must(t, os.MkdirAll(dst, 0o755))
		for _, name := range files {
			h.windowsAsset(filepath.Join(dst, name))
		}
		return
	}
	src, ok := h.sc.artifacts[step.With["name"]]
	if !ok {
		t.Fatalf("step %q downloads artifact %q, which the scenario doesn't provide", step.Name, step.With["name"])
	}
	dst, err := expand(step.With["path"], resolve)
	must(t, err)
	if filepath.Join(h.ws, dst) == filepath.Join(h.ws, src) {
		return // the earlier job left it right there
	}
	t.Fatalf("step %q: artifact %q goes to %s, but the scenario has it in %s", step.Name, step.With["name"], dst, src)
}

// reportArtifact records a handed-on artifact's files with their digests.
func (h *harness) reportArtifact(step wfStep, resolve func(string) (string, error), report *strings.Builder) {
	t := h.t
	path, err := expand(step.With["path"], resolve)
	must(t, err)
	fmt.Fprintf(report, "\n## %s: artifact %s\n", step.Name, step.With["name"])
	dir := path
	if !filepath.IsAbs(dir) {
		dir = filepath.Join(h.ws, dir)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		fmt.Fprintf(report, "error: %v\n", h.normalize(err.Error()))
		return
	}
	for _, e := range entries {
		fmt.Fprintf(report, "  %s %s\n", e.Name(), digest(t, filepath.Join(dir, e.Name())))
	}
}

// reportUploads records every file the job put on the release, with text files in full.
func (h *harness) reportUploads(report *strings.Builder) {
	t := h.t
	entries, err := os.ReadDir(h.uploads)
	must(t, err)
	if len(entries) == 0 {
		return
	}
	fmt.Fprintf(report, "\n# uploads\n")
	for _, e := range entries {
		path := filepath.Join(h.uploads, e.Name())
		fmt.Fprintf(report, "\n## %s %s\n", e.Name(), digest(t, path))
		if strings.HasSuffix(e.Name(), ".txt") || strings.HasSuffix(e.Name(), "latest.json") {
			b, err := os.ReadFile(path)
			must(t, err)
			report.Write(b)
		}
	}
}

func digest(t *testing.T, path string) string {
	b, err := os.ReadFile(path)
	must(t, err)
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

// readKV parses a GITHUB_OUTPUT / GITHUB_ENV file: `k=v` lines and `k<<DELIM` blocks.
func readKV(t *testing.T, path string) map[string]string {
	f, err := os.Open(path)
	must(t, err)
	defer f.Close()
	out := map[string]string{}
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		line := sc.Text()
		if k, delim, ok := strings.Cut(line, "<<"); ok && !strings.Contains(k, "=") {
			var body []string
			for sc.Scan() && sc.Text() != delim {
				body = append(body, sc.Text())
			}
			out[k] = strings.Join(body, "\n")
			continue
		}
		if k, v, ok := strings.Cut(line, "="); ok {
			out[k] = v
		}
	}
	must(t, sc.Err())
	return out
}

func compareGolden(t *testing.T, path, got string) {
	t.Helper()
	if *update {
		must(t, os.MkdirAll(filepath.Dir(path), 0o755))
		must(t, os.WriteFile(path, []byte(got), 0o644))
		return
	}
	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("%v (run with -update to write it)", err)
	}
	if string(want) != got {
		diffPath := filepath.Join(t.TempDir(), "got.txt")
		_ = os.WriteFile(diffPath, []byte(got), 0o644)
		out, _ := exec.Command("diff", "-u", path, diffPath).CombinedOutput()
		t.Fatalf("the dry run's outputs moved from %s:\n%s", path, out)
	}
}

func copyFile(t *testing.T, src, dst string, mode os.FileMode) {
	t.Helper()
	b, err := os.ReadFile(src)
	must(t, err)
	must(t, os.MkdirAll(filepath.Dir(dst), 0o755))
	must(t, os.WriteFile(dst, b, mode))
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}
