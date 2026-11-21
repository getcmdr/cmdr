package main

// The CI ratchet: the Windows test job's known-failures list (`ci-known-failures.tsv` beside this file), checked
// against the job's nextest JUnit report, and rewritten from one. The list may only shrink on its own: a listed test
// that passes fails the job until the list drops it, which `ratchet -write` does for free. A new failure fails the job
// too, and joining the list takes a stated reason (`-write -add REASON`). `docs/tooling/ci.md` § Windows lane.

import (
	"bufio"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

// listStatus is what the list says about one test.
type listStatus string

const (
	// statusFails: the test fails on the hosted runner today. Passing turns the job red until the entry goes.
	statusFails listStatus = "fails"
	// statusFlaky: the test passes and fails across runs on the same tree. Neither outcome turns the job red.
	statusFlaky listStatus = "flaky"
	// statusBuilder: the hosted runner can't give the test what it needs (a real second drive, a folder mount, a
	// privilege), so CI leaves it out of the run and the Windows builder covers it.
	statusBuilder listStatus = "builder"
)

// listEntry is one line of the known-failures list.
type listEntry struct {
	ID     testID
	Status listStatus
	Reason string
}

const listHeader = "crate\ttest\tstatus\treason"

// readList parses the list: a header, then one tab-separated entry per line. `#` lines are comments.
func readList(r io.Reader) ([]listEntry, error) {
	var entries []listEntry
	seen := map[testID]bool{}
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	lineNo := 0
	for scanner.Scan() {
		lineNo++
		line := scanner.Text()
		if line == "" || strings.HasPrefix(line, "#") || line == listHeader {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) != 4 {
			return nil, fmt.Errorf("line %d: want 4 tab-separated fields (%s), got %d", lineNo, listHeader, len(fields))
		}
		e := listEntry{ID: testID{Crate: fields[0], Test: fields[1]}, Status: listStatus(fields[2]), Reason: fields[3]}
		switch e.Status {
		case statusFails, statusFlaky, statusBuilder:
		default:
			return nil, fmt.Errorf("line %d: status %q isn't fails, flaky, or builder", lineNo, e.Status)
		}
		if strings.TrimSpace(e.Reason) == "" {
			return nil, fmt.Errorf("line %d: %s `%s` has no reason", lineNo, e.ID.Crate, e.ID.Test)
		}
		if seen[e.ID] {
			return nil, fmt.Errorf("line %d: %s `%s` is listed twice", lineNo, e.ID.Crate, e.ID.Test)
		}
		seen[e.ID] = true
		entries = append(entries, e)
	}
	return entries, scanner.Err()
}

// writeList writes the list sorted by crate and test, after the comment block that heads the committed file.
func writeList(w io.Writer, preamble []string, entries []listEntry) error {
	sort.Slice(entries, func(i, j int) bool {
		if entries[i].ID.Crate != entries[j].ID.Crate {
			return entries[i].ID.Crate < entries[j].ID.Crate
		}
		return entries[i].ID.Test < entries[j].ID.Test
	})
	bw := bufio.NewWriter(w)
	for _, line := range preamble {
		fmt.Fprintln(bw, line)
	}
	fmt.Fprintln(bw, listHeader)
	for _, e := range entries {
		fmt.Fprintf(bw, "%s\t%s\t%s\t%s\n", e.ID.Crate, e.ID.Test, e.Status, e.Reason)
	}
	return bw.Flush()
}

// readPreamble keeps the file's leading `#` lines, so a rewrite doesn't drop the explanation at its top.
func readPreamble(data string) []string {
	var out []string
	for _, line := range strings.Split(data, "\n") {
		if !strings.HasPrefix(line, "#") {
			break
		}
		out = append(out, line)
	}
	return out
}

// verdict is one run held against the list.
type verdict struct {
	// Regressions failed and aren't listed: the job's red.
	Regressions []failure
	// NowPassing are `fails` entries that ran and passed: the list must drop them.
	NowPassing []testID
	// Stale are `fails` and `flaky` entries that didn't run (renamed, removed, or gated off): the list must drop them.
	Stale []testID
	// Unlisted self-skips passed by skipping themselves (no symlink privilege, say), so CI doesn't cover them: the
	// list must name them `builder`.
	SelfSkipped []testID
	// Ran and Failed count the run, for the summary.
	Ran, Failed int
}

func (v verdict) clean() bool {
	return len(v.Regressions) == 0 && len(v.NowPassing) == 0 && len(v.Stale) == 0 && len(v.SelfSkipped) == 0
}

func judge(entries []listEntry, r report) verdict {
	listed := map[testID]listEntry{}
	for _, e := range entries {
		listed[e.ID] = e
	}
	failing := map[testID]bool{}
	v := verdict{Ran: len(r.Ran), Failed: len(r.Failures)}
	for _, f := range r.Failures {
		failing[f.ID] = true
		if _, ok := listed[f.ID]; !ok {
			v.Regressions = append(v.Regressions, f)
		}
	}
	for _, e := range entries {
		if e.Status == statusBuilder {
			continue
		}
		switch {
		case !r.Ran[e.ID]:
			v.Stale = append(v.Stale, e.ID)
		case e.Status == statusFails && !failing[e.ID]:
			v.NowPassing = append(v.NowPassing, e.ID)
		}
	}
	for _, id := range r.SelfSkipped {
		if e, ok := listed[id]; !ok || e.Status != statusBuilder {
			v.SelfSkipped = append(v.SelfSkipped, id)
		}
	}
	sort.Slice(v.Regressions, func(i, j int) bool {
		a, b := v.Regressions[i].ID, v.Regressions[j].ID
		if a.Crate != b.Crate {
			return a.Crate < b.Crate
		}
		return a.Test < b.Test
	})
	sortIDs(v.NowPassing)
	sortIDs(v.Stale)
	sortIDs(v.SelfSkipped)
	return v
}

// tighten applies a verdict to the list: drops what passes now or no longer runs, and, given a reason, adds the
// regressions as `fails` and the unlisted self-skips as `builder`.
func tighten(entries []listEntry, v verdict, addReason string) []listEntry {
	drop := map[testID]bool{}
	for _, id := range v.NowPassing {
		drop[id] = true
	}
	for _, id := range v.Stale {
		drop[id] = true
	}
	out := make([]listEntry, 0, len(entries))
	for _, e := range entries {
		if !drop[e.ID] {
			out = append(out, e)
		}
	}
	if addReason == "" {
		return out
	}
	for _, f := range v.Regressions {
		out = append(out, listEntry{ID: f.ID, Status: statusFails, Reason: addReason})
	}
	index := map[testID]int{}
	for i, e := range out {
		index[e.ID] = i
	}
	for _, id := range v.SelfSkipped {
		if i, ok := index[id]; ok {
			out[i].Status, out[i].Reason = statusBuilder, addReason
			continue
		}
		out = append(out, listEntry{ID: id, Status: statusBuilder, Reason: addReason})
	}
	return out
}

// builderFilter is the nextest filterset that leaves every `builder` entry out of the run, or "" when there's none.
func builderFilter(entries []listEntry) string {
	var clauses []string
	for _, e := range entries {
		if e.Status == statusBuilder {
			clauses = append(clauses, fmt.Sprintf("(package(%s) & test(=%s))", e.ID.Crate, e.ID.Test))
		}
	}
	if len(clauses) == 0 {
		return ""
	}
	return "not (" + strings.Join(clauses, " | ") + ")"
}

// writeVerdict prints what a run found against the list, as Markdown (the job summary renders it).
func writeVerdict(w io.Writer, entries []listEntry, v verdict) {
	counts := map[listStatus]int{}
	for _, e := range entries {
		counts[e.Status]++
	}
	fmt.Fprintf(w, "**Windows tests**: %s ran, %s failed. The list holds %d `fails`, %d `flaky`, and %d `builder` entries.\n\n",
		thousands(v.Ran), thousands(v.Failed), counts[statusFails], counts[statusFlaky], counts[statusBuilder])
	if v.clean() {
		fmt.Fprintln(w, "Every failure is on the list, and every listed failure still fails.")
		return
	}
	section := func(title, hint string, ids []testID) {
		if len(ids) == 0 {
			return
		}
		fmt.Fprintf(w, "### %s (%d)\n\n%s\n\n", title, len(ids), hint)
		for _, id := range ids {
			fmt.Fprintf(w, "- %s `%s`\n", id.Crate, id.Test)
		}
		fmt.Fprintln(w)
	}
	if len(v.Regressions) > 0 {
		fmt.Fprintf(w, "### Newly failing (%d)\n\nFix them, or list them with a reason (`ratchet -write -add REASON`).\n\n", len(v.Regressions))
		for _, f := range v.Regressions {
			line := firstPanic(f.Text)
			if line == "" && strings.Contains(f.Text, "timeout") {
				line = "timed out"
			}
			if line != "" {
				line = ": " + line
			}
			fmt.Fprintf(w, "- %s `%s`%s\n", f.ID.Crate, f.ID.Test, line)
		}
		fmt.Fprintln(w)
	}
	section("Listed as failing, passing now", "Tighten the list: `ratchet -write` drops them.", v.NowPassing)
	section("Listed but didn't run", "Renamed, removed, or gated off: `ratchet -write` drops them.", v.Stale)
	section("Passed by skipping themselves", "CI doesn't cover them; list them as `builder` with the missing capability as the reason.", v.SelfSkipped)
	fmt.Fprintln(w, "Rewrite the list from this run: `go -C scripts/windows-test-baseline run . ratchet -run <run-id> -write` (`docs/tooling/ci.md` § Windows lane).")
}

// runRatchet is `ratchet`: check a run against the list, or with `-write` rewrite the list from it; `-filter` prints
// the nextest filterset that leaves the `builder` entries out.
func runRatchet(args []string, out io.Writer) error {
	fs := flag.NewFlagSet("ratchet", flag.ContinueOnError)
	list := fs.String("list", defaultListPath(), "the known-failures list")
	junit := fs.String("junit", "", "the run's nextest JUnit report")
	runID := fs.String("run", "", "a CI run ID: download its `windows-test-report` artifact with gh and read that")
	write := fs.Bool("write", false, "rewrite the list from the run instead of checking it")
	add := fs.String("add", "", "with -write: list the run's new failures (as fails) and unlisted self-skips (as builder) with this reason")
	filter := fs.Bool("filter", false, "print the nextest filterset that leaves the builder entries out, and exit")
	summary := fs.String("summary", "", "also append the verdict as Markdown to this file (the job summary)")
	if err := fs.Parse(args); err != nil {
		return err
	}
	data, err := os.ReadFile(*list)
	if err != nil {
		return err
	}
	entries, err := readList(strings.NewReader(string(data)))
	if err != nil {
		return fmt.Errorf("%s: %w", *list, err)
	}
	if *filter {
		fmt.Fprintln(out, builderFilter(entries))
		return nil
	}
	if *add != "" && !*write {
		return errors.New("-add only goes with -write")
	}
	r, err := loadRun(*junit, *runID)
	if err != nil {
		return err
	}
	if *write {
		return rewriteList(out, *list, string(data), entries, r, *add)
	}
	v := judge(entries, r)
	writeVerdict(out, entries, v)
	if *summary != "" {
		if err := appendVerdict(*summary, entries, v); err != nil {
			return err
		}
	}
	if !v.clean() {
		return errors.New("the Windows test run doesn't match its known-failures list")
	}
	return nil
}

// loadRun reads the run's report, from a file or from a CI run's artifact.
func loadRun(junit, runID string) (report, error) {
	path := junit
	if runID != "" {
		var err error
		if path, err = downloadReport(runID); err != nil {
			return report{}, err
		}
	}
	if path == "" {
		return report{}, errors.New("ratchet needs -junit or -run")
	}
	r, err := readReport(path)
	if err != nil {
		return report{}, err
	}
	if len(r.Ran) == 0 {
		return report{}, fmt.Errorf("%s: no test ran, so there's nothing to hold against the list", path)
	}
	return r, nil
}

// rewriteList tightens the list from the run (and, given a reason, loosens it), writes it back, and says what still
// doesn't match.
func rewriteList(out io.Writer, listPath, data string, entries []listEntry, r report, addReason string) error {
	tightened := tighten(entries, judge(entries, r), addReason)
	f, err := os.Create(listPath)
	if err != nil {
		return err
	}
	if err := writeList(f, readPreamble(data), tightened); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	fmt.Fprintf(out, "Rewrote %s: %d entries (was %d).\n", listPath, len(tightened), len(entries))
	if after := judge(tightened, r); !after.clean() {
		writeVerdict(out, tightened, after)
		return errors.New("the list still doesn't match the run: the failures above need a fix or `-add REASON`")
	}
	return nil
}

func appendVerdict(path string, entries []listEntry, v verdict) error {
	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return err
	}
	writeVerdict(f, entries, v)
	return f.Close()
}

// defaultListPath is `ci-known-failures.tsv` beside this tool, wherever `go run` is called from.
func defaultListPath() string {
	if _, err := os.Stat("ci-known-failures.tsv"); err == nil {
		return "ci-known-failures.tsv"
	}
	return filepath.Join("scripts", "windows-test-baseline", "ci-known-failures.tsv")
}

// downloadReport fetches a CI run's `windows-test-report` artifact into a temp dir and returns its JUnit path.
func downloadReport(runID string) (string, error) {
	dir, err := os.MkdirTemp("", "windows-test-report-")
	if err != nil {
		return "", err
	}
	cmd := exec.Command("gh", "run", "download", runID, "--repo", "getcmdr/cmdr", "-n", "windows-test-report", "-D", dir)
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("gh run download %s: %w", runID, err)
	}
	return filepath.Join(dir, "junit.xml"), nil
}
