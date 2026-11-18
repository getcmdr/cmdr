package checks

import (
	"fmt"
	"math"
	"os"
	"os/exec"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
)

// Contention-aware re-run for the macOS Playwright lane: the Rust mechanism's twin,
// built on the shared vocabulary in `contention-verdict.go`.
//
// The problem this answers: the lane runs three Tauri instances, three Playwright
// processes, and three 15 fps video recorders at once, on a machine David is working
// on. That routinely produces 1-3 DIFFERENT failures per run, and a spec that failed
// passes 4/4 when re-run alone seconds later. An agent then spends half an hour
// investigating a failure that was never real.
//
// `playwright.config.ts`'s `retries: 1` doesn't catch this, and can't: the retry fires
// about a second after the failure, inside the same shard, with all three apps and all
// three recorders still competing. It is not an independent trial, so a load-induced
// failure fails twice and escapes the retry carve-out as a hard red. This runs AFTER
// every shard has finished, when the machine is quiet, which is the part the retry
// structurally cannot do.
//
// The verdict ladder is the Rust one, in Playwright's terms:
//
//   - Passes alone at the UNCHANGED timeout → the suite was starving it. Contention.
//   - Needs headroom, machine quiet         → it genuinely got slower. Not absorbed.
//   - Needs headroom, machine still busy    → inconclusive; neither claim is made.
//   - Fails alone even with headroom        → a real failure, whatever the load.
//
// **The re-run unit is the spec FILE, never one test.** `fullyParallel: false` +
// `workers: 1` + one app instance shared by the whole file mean a test may depend on
// the ones before it: a fixture the earlier test created, a pane it focused, a tab it
// opened. Re-running one test alone would fail for ordering reasons and produce a
// false "real" verdict, which is worse than no verdict at all.
//
// **A re-run reuses the shard's own app and env**, because an MTP spec needs its
// `CMDR_MTP_FIXTURE_ROOT` and the MTP shard's virtual device, and a non-MTP spec needs
// a shard that was told to leave that root alone. The apps are still alive at this
// point (`cleanupApps` is deferred in `RunDesktopE2EPlaywright`). Only the report and
// output dir differ, so a re-run can never overwrite the original run's evidence.

// MaxE2ERerunFiles bounds the re-run. Each file costs its full runtime, up to twice, so
// this is minutes rather than the Rust lane's seconds. Past it, that many red files at
// once means the machine was too loaded for the run to mean anything, and the honest
// move is to say so instead of spending twenty minutes proving it.
const MaxE2ERerunFiles = 5

// playwrightBaseTimeoutMs mirrors `timeout` in `apps/desktop/test/e2e-playwright/
// playwright.config.ts`. Stage 1 deliberately does NOT pass it: leaving the flag off is
// what makes a pass there mean "starved" rather than "given more time".
const playwrightBaseTimeoutMs = 15000

// e2eHeadroomFactor is how much extra wall-clock stage 2 grants. 4x (60 s) is far past
// any legitimate spec: the suite's own budget flags anything over 2 s.
const e2eHeadroomFactor = 4

// e2eWaitScale turns the load somebody ELSE is putting on the machine into the
// multiplier the suite stretches its waits by (`CMDR_E2E_WAIT_SCALE`, consumed by
// `apps/desktop/test/e2e-playwright/wait-budget.ts`).
//
// The reading is taken BEFORE the shard apps launch, so it measures ambient load only:
// the suite's own three instances are already priced into the budgets the specs ship
// with. Hence `1 + ambient` rather than `ambient` — a quiet box keeps today's numbers
// exactly, and every runnable thread per core that somebody else is running buys the
// suite one extra multiple of patience.
//
// The clamp is `e2eHeadroomFactor` on purpose, the same 4x stage 2 grants: past it,
// waiting is no longer cheaper than re-running the spec alone, which is what the
// isolation re-run is for.
func e2eWaitScale(ambientLoadPerCore float64) float64 {
	scale := 1 + max(ambientLoadPerCore, 0)
	scale = min(scale, e2eHeadroomFactor)
	return math.Round(scale*10) / 10
}

// e2eFailingFile is one spec file that went red, with the tests in it that failed and
// the shard they ran on.
type e2eFailingFile struct {
	// file is the spec as Playwright's JSON report names it, relative to `testDir`
	// (`duplicate-in-place.spec.ts`).
	file string
	// shard is the shard whose app and env the re-run reuses. Zero in unit tests, which
	// never reach the real runner.
	shard shardSpec
	// shardName is carried separately so the report can name it without a live shard.
	shardName string
	// keys are `<file>::<describe chain>::<title>` for each test that failed, the same
	// identity the duration allowlist and the per-test log use.
	keys []string
}

// E2ERerunner re-runs one spec file alone and reports which of its tests failed.
// `timeoutMs` of 0 leaves the configured per-test timeout alone. Injected so the whole
// classification is testable without launching a Tauri app.
//
// An error means the re-run could not be run or could not be believed, never that a
// test failed: those two must not be confusable, or a broken harness would buy a red
// run a warn.
type E2ERerunner func(target e2eFailingFile, timeoutMs int) (failed map[string]bool, err error)

// E2ESpecResult is one failing spec plus what the re-runs concluded about it.
type E2ESpecResult struct {
	Key       string
	File      string
	ShardName string
	Verdict   ContentionVerdict
}

// MaybeClassifyE2EContention runs the two-stage classification unless the failing-file
// count is past MaxE2ERerunFiles, in which case it reports `skipped` so the caller can
// disclose the cap instead of silently examining a subset.
func MaybeClassifyE2EContention(files []e2eFailingFile, rerun E2ERerunner, load LoadSampler) (results []E2ESpecResult, skipped bool) {
	if len(files) > MaxE2ERerunFiles {
		return nil, true
	}
	return ClassifyE2EContention(files, rerun, load), false
}

// ClassifyE2EContention re-runs each failing spec file alone and SEQUENTIALLY, first at
// the unchanged timeout and then (only for what still fails) with headroom.
//
// Sequential is the whole point: two re-runs at once would recreate the contention the
// first stage exists to remove.
//
// Only the tests that failed in the original run are judged. A test the re-run newly
// breaks is a different run's evidence, and the original run already stands as the
// record of what went red.
func ClassifyE2EContention(files []e2eFailingFile, rerun E2ERerunner, load LoadSampler) []E2ESpecResult {
	results := []E2ESpecResult{}
	for _, f := range files {
		verdicts := make(map[string]ContentionVerdict, len(f.keys))
		for _, key := range f.keys {
			verdicts[key] = VerdictContention // upgraded below if it keeps failing
		}

		failedAlone, err := rerun(f, 0)
		if err != nil {
			// A harness-level problem is not evidence about any spec. Leave every verdict
			// real rather than inventing an excuse for a red run.
			markAllKeys(verdicts, f.keys, VerdictReal)
			results = append(results, specResults(f, verdicts)...)
			continue
		}

		stillFailing := keysIn(f.keys, failedAlone)
		if len(stillFailing) > 0 {
			// Load is sampled around the escalation only: that verdict is the only one
			// whose meaning depends on the machine being quiet.
			headroom := VerdictTooSlow
			if load() > BusyLoadPerCore {
				headroom = VerdictInconclusive
			}
			markAllKeys(verdicts, stillFailing, headroom)

			failedWithHeadroom, err := rerun(f, playwrightBaseTimeoutMs*e2eHeadroomFactor)
			if err != nil {
				markAllKeys(verdicts, stillFailing, VerdictReal)
			} else {
				markAllKeys(verdicts, keysIn(stillFailing, failedWithHeadroom), VerdictReal)
			}
		}
		results = append(results, specResults(f, verdicts)...)
	}
	return results
}

func markAllKeys(verdicts map[string]ContentionVerdict, keys []string, v ContentionVerdict) {
	for _, key := range keys {
		verdicts[key] = v
	}
}

// keysIn keeps the input order, so a report never reshuffles a spec file's tests.
func keysIn(keys []string, set map[string]bool) []string {
	out := make([]string, 0, len(keys))
	for _, key := range keys {
		if set[key] {
			out = append(out, key)
		}
	}
	return out
}

func specResults(f e2eFailingFile, verdicts map[string]ContentionVerdict) []E2ESpecResult {
	out := make([]E2ESpecResult, 0, len(f.keys))
	for _, key := range f.keys {
		out = append(out, E2ESpecResult{Key: key, File: f.file, ShardName: f.shardName, Verdict: verdicts[key]})
	}
	return out
}

// E2EVerdicts projects the results onto the shared vocabulary `WarnOnly` judges.
func E2EVerdicts(results []E2ESpecResult) []ContentionVerdict {
	verdicts := make([]ContentionVerdict, 0, len(results))
	for _, r := range results {
		verdicts = append(verdicts, r.Verdict)
	}
	return verdicts
}

// collectE2EFailures reads each shard's JSON report and groups the run's failing tests
// by spec FILE, which is the unit the re-run works in.
//
// It reuses `parsePlaywrightRecords` rather than walking the report a fourth time: a
// `status: "unexpected"` is exactly a `TestFailed` record, and the key it builds is the
// one the duration allowlist, the flake warning, and the per-test log all already use.
//
// `runStart` guards against a report older than this run: paths are pid-scoped, so this
// is the second lock on the same door, covering a recycled pid landing on a leftover the
// sweep hasn't collected. Judging this run on another run's failures would be worse than
// not judging it at all.
func collectE2EFailures(shards []shardSpec, runStart time.Time) []e2eFailingFile {
	byFile := map[string]*e2eFailingFile{}
	var order []string

	for _, s := range shards {
		info, err := os.Stat(s.jsonReport)
		if err != nil || info.ModTime().Before(runStart) {
			continue
		}
		records, err := parsePlaywrightRecords(s.jsonReport)
		if err != nil {
			continue
		}
		for _, rec := range records {
			if rec.Outcome != TestFailed {
				continue
			}
			file := e2eKeyFile(rec.ID)
			entry, seen := byFile[file]
			if !seen {
				entry = &e2eFailingFile{file: file, shard: s, shardName: s.name}
				byFile[file] = entry
				order = append(order, file)
			}
			entry.keys = append(entry.keys, rec.ID)
		}
	}

	sort.Strings(order)
	files := make([]e2eFailingFile, 0, len(order))
	for _, file := range order {
		if entry, ok := byFile[file]; ok && entry != nil {
			files = append(files, *entry)
		}
	}
	return files
}

// e2eKeyFile takes the spec file back out of a `<file>::<describe chain>::<title>` key.
func e2eKeyFile(key string) string {
	file, _, found := strings.Cut(key, "::")
	if !found {
		return key
	}
	return file
}

// resolveE2EFailure turns a red Playwright run into a verdict. Every failing spec file
// is re-run alone once the suite is over, so starvation by the rest of the lane is told
// apart from real slowness or a real defect. Only an all-contention (or all-unsettled)
// outcome softens the result, and even then to a WARN, never a pass: the re-run must not
// become a silent absorber, which is exactly how the retry budget rotted before it was
// surfaced.
//
// `rerun` and `load` are injected for the same reason the Rust lane injects them: the
// verdict logic must stay ONE implementation, testable without an app.
func resolveE2EFailure(
	runErr error,
	files []e2eFailingFile,
	rerun E2ERerunner,
	load LoadSampler,
	history map[string]TestHistory,
	historyErr error,
) (CheckResult, error) {
	// Nothing classifiable: a shard that died before the test phase, a build break, a
	// socket that never appeared. Report it exactly as it arrived.
	if len(files) == 0 {
		return CheckResult{}, runErr
	}

	failingTests := 0
	for _, f := range files {
		failingTests += len(f.keys)
	}

	results, skipped := MaybeClassifyE2EContention(files, rerun, load)
	if skipped {
		return CheckResult{}, fmt.Errorf("%s\n%w",
			E2ERerunSkippedNote(len(files)), runErr)
	}

	loadAvg := LoadAverage()
	summary := E2EContentionSummary(results, history, loadAvg)
	if historyErr != nil {
		summary += fmt.Sprintf("  (spec history unavailable: %v)\n", historyErr)
	}

	if WarnOnly(E2EVerdicts(results)) {
		return CheckResult{
			Code:    ResultWarning,
			Message: e2eContentionWarnMessage(results) + "\n" + summary,
			Total:   -1,
			Issues:  len(results),
			Changes: -1,
		}, nil
	}
	// The headline leads, because a check's first error line is also the message that
	// lands in `check-log.csv` and the one an agent reads before anything else. The
	// shard transcripts follow it, so the picture of the failure is still right there.
	return CheckResult{}, fmt.Errorf("%s\n%s%w",
		e2eRedHeadline(results, loadAvg), summary, runErr)
}

// e2eRedHeadline is the single line that has to do the whole job on its own: how many of
// the failures are the reader's problem, how many were the lane starving itself.
func e2eRedHeadline(results []E2ESpecResult, loadAvg float64) string {
	yours, notYours := 0, 0
	for _, r := range results {
		if r.Verdict == VerdictContention || r.Verdict == VerdictInconclusive {
			notYours++
		} else {
			yours++
		}
	}
	if notYours == 0 {
		return fmt.Sprintf("playwright E2E: %d failing %s survived an isolated re-run, so %s real (load was %.1f)",
			yours, Pluralize(yours, "spec", "specs"), Pluralize(yours, "it's", "they're"), loadAvg)
	}
	return fmt.Sprintf("playwright E2E: %d of %d failing specs survived an isolated re-run and %s real; the other %d %s the lane starving itself (load was %.1f)",
		yours, len(results), Pluralize(yours, "is", "are"), notYours, Pluralize(notYours, "was", "were"), loadAvg)
}

// e2eContentionWarnMessage is the one line that lands in the check summary and in
// `check-log.csv`, so it has to carry the whole story on its own. The listing under it
// names the specs, so this counts rather than repeating them.
func e2eContentionWarnMessage(results []E2ESpecResult) string {
	starved, murky := 0, 0
	for _, r := range results {
		if r.Verdict == VerdictContention {
			starved++
		} else {
			murky++
		}
	}
	if murky == 0 {
		return fmt.Sprintf("playwright E2E: all %d failing %s passed when re-run alone, so the lane was starving itself; none confirmed a defect",
			starved, Pluralize(starved, "spec", "specs"))
	}
	parts := make([]string, 0, 2)
	if starved > 0 {
		parts = append(parts, fmt.Sprintf("%d passed when re-run alone", starved))
	}
	parts = append(parts, fmt.Sprintf("%d needed more time while the machine was still busy, so the cause is unsettled", murky))
	return fmt.Sprintf("playwright E2E: %d failing %s, none confirmed a defect: %s",
		len(results), Pluralize(len(results), "spec", "specs"), strings.Join(parts, "; "))
}

// E2EContentionSummary renders the verdicts for whoever reads the check output. An agent
// reading it should need zero judgement: which specs are the reader's problem, which are
// the machine's, and what the machine was doing at the time.
func E2EContentionSummary(results []E2ESpecResult, history map[string]TestHistory, loadAvg float64) string {
	var contention, tooSlow, inconclusive, real []E2ESpecResult
	for _, r := range results {
		switch r.Verdict {
		case VerdictContention:
			contention = append(contention, r)
		case VerdictTooSlow:
			tooSlow = append(tooSlow, r)
		case VerdictInconclusive:
			inconclusive = append(inconclusive, r)
		default:
			real = append(real, r)
		}
	}

	var b strings.Builder
	fmt.Fprintf(&b, "%d failing %s re-run alone after every shard finished (load was %.1f):\n",
		len(results), Pluralize(len(results), "spec", "specs"), loadAvg)
	section := func(heading string, specs []E2ESpecResult) {
		if len(specs) == 0 {
			return
		}
		b.WriteString("  • " + heading + "\n")
		for _, s := range specs {
			h, found := history[s.Key]
			fmt.Fprintf(&b, "      - %s  [shard %s, %s]\n",
				formatE2ETestKey(s.Key), s.ShardName, renderTestHistory(h, found))
		}
	}
	section(fmt.Sprintf("%d passed alone at the same timeout, so the lane was starving %s: contention, NOT your problem",
		len(contention), Pluralize(len(contention), "it", "them")), contention)
	section(fmt.Sprintf("Needed up to %dx the timeout even alone on a quiet machine, so this is real slowness: speed the spec up or give it a justified `test.setTimeout`",
		e2eHeadroomFactor), tooSlow)
	section("Needed extra time, but the machine was still busy during the re-run, so starvation and real slowness can't be told apart. Re-run on a quiet machine to settle it",
		inconclusive)
	section(fmt.Sprintf("Still failing alone with %dx the timeout: a genuine failure, YOUR problem", e2eHeadroomFactor), real)

	b.WriteString("  " + testHistoryCaveat + "\n")
	return b.String()
}

// E2ERerunSkippedNote explains why no re-run happened, disclosing the cap so a reader
// never mistakes a bounded look for a full one.
func E2ERerunSkippedNote(failingFiles int) string {
	return fmt.Sprintf(
		"%d spec files failed, past the %d-file isolation re-run cap, so no isolated re-run was attempted. "+
			"That many red files at once usually means the machine was too loaded for the run to mean anything; "+
			"re-run the lane on a quieter machine before reading any of it as a defect.",
		failingFiles, MaxE2ERerunFiles)
}

// playwrightRerunner re-runs one spec file against its shard's still-running app.
//
// Every path it writes is scoped to the run AND to the attempt, so a re-run can never
// overwrite the original run's report, recordings, or error contexts: that evidence is
// the only picture of what the failure looked like, and someone is usually reading it.
// Same reasoning as `planShards`.
func playwrightRerunner(desktopDir string, pid int, baseWaitScale float64) E2ERerunner {
	attempt := 0
	return func(target e2eFailingFile, timeoutMs int) (map[string]bool, error) {
		attempt++
		report := fmt.Sprintf("/tmp/cmdr-e2e-rerun-report-%d-%d.json", pid, attempt)
		outputDir := fmt.Sprintf("/tmp/cmdr-e2e-rerun-results-%d-%d", pid, attempt)

		args := []string{
			"exec", "playwright", "test",
			"--config", "test/e2e-playwright/playwright.config.ts",
			"--project", "tauri",
			// ❌ Never let the re-run retry. A retry inside the probe would answer a
			// different question than the one being asked ("does it pass alone, once?").
			"--retries=0",
		}
		if timeoutMs > 0 {
			args = append(args, "--timeout="+strconv.Itoa(timeoutMs))
		}
		// Playwright reads a positional arg as a regex over the test file's full path, so
		// an anchored, escaped filename selects exactly this spec.
		args = append(args, regexp.QuoteMeta(target.file)+"$")

		// Stage 1 reproduces the original run's budgets exactly, or a pass would mean
		// "given more time" instead of "starved". Stage 2 is the headroom stage, and
		// `--timeout` alone can't deliver it: Playwright's runtime `test.setTimeout`
		// BEATS the CLI flag, so a spec that sets its own ceiling would sit at its
		// original budget and be called real. Handing the scale through the env widens
		// those specs too, because `wait-budget.ts` wraps every `setTimeout` call.
		waitScale := baseWaitScale
		if timeoutMs > 0 {
			waitScale = e2eHeadroomFactor
		}

		cmd := niceCommand("pnpm", args...)
		cmd.Dir = desktopDir
		// The one run whose video is worth its CPU. `fixtures.ts` stops the recorder for
		// every ordinary test (three shards filming at 15 fps on a machine somebody is
		// using, for footage of passing tests that nobody watches); a re-run is alone on
		// the machine and is the attempt somebody will actually look at.
		cmd.Env = append(shardPlaywrightEnv(target.shard, report, outputDir, waitScale), "CMDR_E2E_KEEP_RECORDING=1")
		output, _ := RunCommand(cmd, true)

		// A non-zero exit is EXPECTED here: failing specs are the whole point. The report
		// is the evidence, so its absence (not the exit code) is what makes a re-run
		// unbelievable.
		records, err := parsePlaywrightRecords(report)
		if err != nil {
			return nil, fmt.Errorf("the isolated re-run of %s left no readable report (%v); playwright said:\n%s",
				target.file, err, indentOutput(extractE2ETestOutput(output)))
		}
		// The trap the Rust lane hit with `--run-ignored only`: a filter that selects
		// nothing reports zero failures, which reads as "everything passed alone" and
		// turns every real failure into a contention warn.
		if !rerunCoveredTargets(records, target.keys) {
			return nil, fmt.Errorf("the isolated re-run of %s ran none of its failing tests, so it is not evidence; playwright said:\n%s",
				target.file, indentOutput(extractE2ETestOutput(output)))
		}

		failed := map[string]bool{}
		for _, rec := range records {
			if rec.Outcome == TestFailed {
				failed[rec.ID] = true
			}
		}
		return failed, nil
	}
}

// rerunCoveredTargets reports whether the re-run actually executed at least one of the
// tests it was supposed to judge, whatever the outcome.
func rerunCoveredTargets(records []TestRecord, keys []string) bool {
	ran := make(map[string]bool, len(records))
	for _, rec := range records {
		ran[rec.ID] = true
	}
	for _, key := range keys {
		if ran[key] {
			return true
		}
	}
	return false
}

// e2eNiceIncrement is how far the lane steps out of the user's way.
//
// `nice` only yields when the CPU is CONTENDED, so an idle machine runs the suite at
// full speed and a busy one gives its owner their machine back. ❌ Never macOS
// `taskpolicy -b` / background QoS: that also throttles disk I/O, which on a suite
// built out of file operations turns every spec into a timeout.
//
// This deliberately trades suite wall-clock for the user's responsiveness. What makes
// it safe is the isolation re-run above: a spec starved by the trade now gets re-run
// alone and reported as contention rather than as a defect.
const e2eNiceIncrement = 5

// niceCommand builds a command that yields CPU to whoever else wants it.
func niceCommand(name string, args ...string) *exec.Cmd {
	execName, execArgs := niceArgv(CommandExists("nice"), name, args)
	return exec.Command(execName, execArgs...)
}

// niceArgv decides what to exec: `nice -n N <name> <args…>`, or the command untouched
// when `nice` isn't on PATH. `nice` execs in place, so the wrapped process keeps the pid
// the caller waits on and kills. Degrading rather than failing matters because a missing
// `nice` would otherwise take the whole lane down over a scheduling preference.
//
// It returns the executable and its arguments separately rather than one argv, so no
// caller has to index a slice to split them apart again.
func niceArgv(niceAvailable bool, name string, args []string) (execName string, execArgs []string) {
	if !niceAvailable {
		return name, args
	}
	return "nice", append([]string{"-n", strconv.Itoa(e2eNiceIncrement), name}, args...)
}
