package main

import (
	"fmt"
	"regexp"
	"slices"
	"time"

	"cmdr/scripts/release-assets/manifest"
)

// The decisions that make the finish step resumable: each run reads where the release stands
// on GitHub and picks up from there, so it holds no state of its own between runs.

// workflowRun is one row of `gh run list --json databaseId,status,conclusion,createdAt`.
type workflowRun struct {
	ID         int64     `json:"databaseId"`
	Status     string    `json:"status"`
	Conclusion string    `json:"conclusion"`
	CreatedAt  time.Time `json:"createdAt"`
}

func (r workflowRun) completed() bool { return r.Status == "completed" }
func (r workflowRun) succeeded() bool { return r.completed() && r.Conclusion == "success" }

// newest picks the most recently created run; `gh run list` order isn't relied on.
func newest(runs []workflowRun) (workflowRun, bool) {
	if len(runs) == 0 {
		return workflowRun{}, false
	}
	return slices.MaxFunc(runs, func(a, b workflowRun) int { return a.CreatedAt.Compare(b.CreatedAt) }), true
}

type finishAction int

const (
	// actionDispatch: no finishing run yet, so start one.
	actionDispatch finishAction = iota
	// actionWait: the newest finishing run is still going.
	actionWait
	// actionDone: the newest finishing run went green, so the release is out.
	actionDone
	// actionRerunFailed: the newest finishing run ended red. Re-run its failed jobs: a fresh
	// dispatch would be refused by the guard once `publish` committed the manifest.
	actionRerunFailed
)

// nextFinishAction decides from the dispatch runs on the tag what the finish step does next.
func nextFinishAction(runs []workflowRun) (finishAction, workflowRun) {
	run, ok := newest(runs)
	switch {
	case !ok:
		return actionDispatch, run
	case !run.completed():
		return actionWait, run
	case run.succeeded():
		return actionDone, run
	default:
		return actionRerunFailed, run
	}
}

// signedNames are the assets the updater downloads, the only ones this key signs: every
// update payload in the release asset manifest (`scripts/release-assets/manifest`), plus the
// advisory list once a release ships one (`manifest.File.Signed`).
func signedNames(version string) []string {
	return manifest.Names(manifest.For(version).Signed())
}

type releaseAsset struct {
	Name string `json:"name"`
}

// releaseInfo is `gh release view --json isDraft,assets`; gh finds a draft by its tag too.
type releaseInfo struct {
	IsDraft bool           `json:"isDraft"`
	Assets  []releaseAsset `json:"assets"`
}

func (r releaseInfo) missing(names []string) []string {
	var out []string
	for _, name := range names {
		if !slices.ContainsFunc(r.Assets, func(a releaseAsset) bool { return a.Name == name }) {
			out = append(out, name)
		}
	}
	return out
}

var (
	versionPattern = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+$`)
	// A pipeline test tag's version, like `0.52.1-pipelinetest.1`, which only a dry run takes.
	preReleasePattern = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+-[0-9A-Za-z.]*[0-9A-Za-z]$`)
)

func checkVersion(v string, dryRun bool) error {
	if versionPattern.MatchString(v) || (dryRun && preReleasePattern.MatchString(v)) {
		return nil
	}
	return fmt.Errorf("version must look like 0.51.0 (no leading v), got %q", v)
}
