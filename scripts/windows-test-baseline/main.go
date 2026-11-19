// Windows test baseline: turns a nextest JUnit report from the Windows builder into the failing-test list of
// docs/notes/windows-port/app-path-baseline.md, each failure tagged with its cause cluster and the effort that owns it,
// and diffs a later run against that list by test name.
//
// Usage (from anywhere in the repo):
//
//	go -C scripts/windows-test-baseline run . render -junit FILE -commit SHA [-date YYYY-MM-DD] > OUT.md
//	go -C scripts/windows-test-baseline run . diff -baseline docs/notes/windows-port/app-path-baseline.md -junit FILE
//	go -C scripts/windows-test-baseline run . explain -junit FILE [-crate NAME]   # each failure, its cluster, and its panic lines
//	go -C scripts/windows-test-baseline run . tsv -junit FILE > OUT.tsv           # one line per failure, machine-readable
//
// Why JUnit and not the console: PowerShell wraps console output at its window width and some logs come out UTF-16,
// so test names and panic messages had to be stitched back together from the earlier logs. The JUnit report holds each
// test whole, with its captured output, which is what the cause is read from.
//
// The clusters and owners mirror docs/specs/app-path-plan.md § "Windows failures, mapped"; `classify` is the one place
// they're decided.
package main

import (
	"bufio"
	"encoding/xml"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"sort"
	"strings"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: windows-test-baseline render|diff|explain|tsv [flags]")
		os.Exit(2)
	}
	var err error
	switch os.Args[1] {
	case "render":
		err = runRender(os.Args[2:], os.Stdout)
	case "diff":
		err = runDiff(os.Args[2:], os.Stdout)
	case "explain":
		err = runExplain(os.Args[2:], os.Stdout)
	case "tsv":
		err = runTSV(os.Args[2:], os.Stdout)
	default:
		err = fmt.Errorf("unknown command %q (want render, diff, explain, or tsv)", os.Args[1])
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func runRender(args []string, out io.Writer) error {
	fs := flag.NewFlagSet("render", flag.ContinueOnError)
	junit := fs.String("junit", "", "nextest JUnit report")
	commit := fs.String("commit", "", "the commit the run tested")
	date := fs.String("date", "", "the run's date, YYYY-MM-DD")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *junit == "" || *commit == "" {
		return errors.New("render needs -junit and -commit")
	}
	report, err := readReport(*junit)
	if err != nil {
		return err
	}
	return render(out, report, *commit, *date)
}

func runDiff(args []string, out io.Writer) error {
	fs := flag.NewFlagSet("diff", flag.ContinueOnError)
	baseline := fs.String("baseline", "", "the baseline Markdown")
	junit := fs.String("junit", "", "the new run's nextest JUnit report")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *baseline == "" || *junit == "" {
		return errors.New("diff needs -baseline and -junit")
	}
	f, err := os.Open(*baseline)
	if err != nil {
		return err
	}
	defer f.Close()
	before, err := parseBaseline(f)
	if err != nil {
		return err
	}
	report, err := readReport(*junit)
	if err != nil {
		return err
	}
	writeDiff(out, before, report)
	return nil
}

// runExplain prints each failure of one crate with its cluster and the panic lines its cause was read from, for
// checking (and refining) `classify` against a real run.
func runExplain(args []string, out io.Writer) error {
	fs := flag.NewFlagSet("explain", flag.ContinueOnError)
	junit := fs.String("junit", "", "nextest JUnit report")
	crate := fs.String("crate", "", "only this crate's failures")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *junit == "" {
		return errors.New("explain needs -junit")
	}
	report, err := readReport(*junit)
	if err != nil {
		return err
	}
	tags := classifyAll(report.Failures)
	for i, f := range report.Failures {
		if *crate != "" && f.ID.Crate != *crate {
			continue
		}
		unread := ""
		if tags[i].Unread {
			unread = " (cause unread)"
		}
		fmt.Fprintf(out, "== %s %s: %s%s\n", f.ID.Crate, f.ID.Test, tags[i].Cluster.Key, unread)
		lines := strings.Split(f.Text, "\n")
		for j, line := range lines {
			// The panic message is the line after `panicked at <file>:<line>:<col>:`.
			if strings.Contains(line, "panicked at") && strings.HasSuffix(line, ":") && j+1 < len(lines) {
				fmt.Fprintf(out, "   %s\n", strings.TrimSpace(lines[j+1]))
			}
			if strings.Contains(line, "left:") || strings.Contains(line, "right:") {
				fmt.Fprintf(out, "   %s\n", strings.TrimSpace(line))
			}
		}
	}
	return nil
}

// runTSV prints one line per failure, `crate`, `test`, `cluster`, `read` (`read` or `unread`), and the first panic
// line (cut at 200 bytes), with a header: the machine-readable record of a run, small enough to keep in the repo.
func runTSV(args []string, out io.Writer) error {
	fs := flag.NewFlagSet("tsv", flag.ContinueOnError)
	junit := fs.String("junit", "", "nextest JUnit report")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *junit == "" {
		return errors.New("tsv needs -junit")
	}
	report, err := readReport(*junit)
	if err != nil {
		return err
	}
	tags := classifyAll(report.Failures)
	order := make([]int, len(tags))
	for i := range order {
		order[i] = i
	}
	sort.Slice(order, func(a, b int) bool {
		ia, ib := tags[order[a]].ID, tags[order[b]].ID
		if ia.Crate != ib.Crate {
			return ia.Crate < ib.Crate
		}
		return ia.Test < ib.Test
	})
	w := bufio.NewWriter(out)
	fmt.Fprintln(w, "crate\ttest\tcluster\tread\tpanic")
	for _, i := range order {
		read := "read"
		if tags[i].Unread {
			read = "unread"
		}
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", tags[i].ID.Crate, tags[i].ID.Test, tags[i].Cluster.Key, read, firstPanic(report.Failures[i].Text))
	}
	return w.Flush()
}

// firstPanic is the first panic message in a failure's text, one line, tabs flattened, at most 200 bytes.
func firstPanic(text string) string {
	lines := strings.Split(text, "\n")
	for j, line := range lines {
		if strings.Contains(line, "panicked at") && strings.HasSuffix(line, ":") && j+1 < len(lines) {
			msg := strings.ReplaceAll(strings.TrimSpace(lines[j+1]), "\t", " ")
			if len(msg) > 200 {
				msg = strings.ToValidUTF8(msg[:200], "")
			}
			return msg
		}
	}
	return ""
}

// ── The JUnit report ──────────────────────────────────────────────────────

type junitSuites struct {
	Suites []junitSuite `xml:"testsuite"`
}

type junitSuite struct {
	Name  string      `xml:"name,attr"`
	Cases []junitCase `xml:"testcase"`
}

type junitProblem struct {
	Message string `xml:"message,attr"`
	Type    string `xml:"type,attr"`
	Text    string `xml:",chardata"`
}

type junitCase struct {
	Name      string         `xml:"name,attr"`
	Failures  []junitProblem `xml:"failure"`
	Errors    []junitProblem `xml:"error"`
	Skipped   *struct{}      `xml:"skipped"`
	SystemOut string         `xml:"system-out"`
	SystemErr string         `xml:"system-err"`
}

// testID names one test the way nextest does: its crate, and its path inside the binary.
type testID struct {
	Crate string
	Test  string
}

// failure is one failing test and the text its cause is read from.
type failure struct {
	ID   testID
	Text string
}

// report is one run: every test that ran, the ones that failed, and the passes that skipped themselves.
type report struct {
	Ran      map[testID]bool
	Failures []failure
	// SelfSkipped is every test that passed by returning early, read from the skip marker it printed
	// (`cmdr_fs::testing::SKIP_MARKER`). nextest has no runtime skip, so only the marker tells such a pass from a
	// real one, and the report holds a pass's output only under the `windows-baseline` profile.
	SelfSkipped []testID
}

// skipMarker is `cmdr_fs::testing::SKIP_MARKER`: a line a cell prints when it skips itself.
const skipMarker = "skipped: "

// skippedItself reports whether a passing test's output holds the skip marker at a line start.
func skippedItself(output string) bool {
	for _, line := range strings.Split(output, "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), skipMarker) {
			return true
		}
	}
	return false
}

func readReport(path string) (report, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return report{}, err
	}
	var suites junitSuites
	if err := xml.Unmarshal(data, &suites); err != nil {
		return report{}, fmt.Errorf("%s: %w", path, err)
	}
	r := report{Ran: map[testID]bool{}}
	for _, suite := range suites.Suites {
		crate := crateOf(suite.Name)
		for _, c := range suite.Cases {
			if c.Skipped != nil {
				continue
			}
			id := testID{Crate: crate, Test: c.Name}
			r.Ran[id] = true
			problems := append(append([]junitProblem{}, c.Failures...), c.Errors...)
			if len(problems) == 0 {
				if skippedItself(c.SystemOut + "\n" + c.SystemErr) {
					r.SelfSkipped = append(r.SelfSkipped, id)
				}
				continue
			}
			var text strings.Builder
			for _, p := range problems {
				fmt.Fprintf(&text, "%s\n%s\n%s\n", p.Type, p.Message, p.Text)
			}
			text.WriteString(c.SystemOut)
			text.WriteString("\n")
			text.WriteString(c.SystemErr)
			r.Failures = append(r.Failures, failure{ID: id, Text: text.String()})
		}
	}
	return r, nil
}

// crateOf turns nextest's suite name (`cmdr-fs`, `cmdr-fs::lib`, `cmdr::bin/Cmdr`) into the crate.
func crateOf(suite string) string {
	crate, _, _ := strings.Cut(suite, "::")
	return crate
}

// ── Rendering and diffing ─────────────────────────────────────────────────

func render(out io.Writer, r report, commit, date string) error {
	tagged := classifyAll(r.Failures)
	byCrate := map[string][]tag{}
	for _, t := range tagged {
		byCrate[t.ID.Crate] = append(byCrate[t.ID.Crate], t)
	}
	crates := make([]string, 0, len(byCrate))
	for crate := range byCrate {
		crates = append(crates, crate)
	}
	sort.Strings(crates)

	w := bufio.NewWriter(out)
	fmt.Fprintln(w, "# App path Windows baseline")
	fmt.Fprintln(w)
	when := ""
	if date != "" {
		when = ", " + date
	}
	fmt.Fprintf(w, "Every test that failed on the Windows builder at `%s`%s: %s of the %s that ran under `cargo nextest run --workspace --exclude cmdr-fsevent-stream --no-fail-fast` (the macOS-only FSEvents crate has no Windows build). ", commit, when, thousands(len(r.Failures)), thousands(len(r.Ran)))
	fmt.Fprintln(w, "Made by `scripts/windows-test-baseline` from the run's nextest JUnit report; each later app path milestone diffs its run against this list with the same tool, so \"turned green\" and \"newly red\" are names, not counts. Grouped by crate, then by cause cluster and the effort that owns it (`docs/specs/app-path-plan.md` § \"Windows failures, mapped\"). \"Cause unread\" marks a failure whose output names no cause; it sits in its nearest sibling's cluster.")
	for _, crate := range crates {
		fmt.Fprintln(w)
		fmt.Fprintf(w, "## %s (%d)\n", crate, len(byCrate[crate]))
		groups := map[string][]tag{}
		for _, t := range byCrate[crate] {
			key := t.Cluster.Label + " | " + t.Owner
			groups[key] = append(groups[key], t)
		}
		keys := make([]string, 0, len(groups))
		for key := range groups {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			list := groups[key]
			label, owner, _ := strings.Cut(key, " | ")
			fmt.Fprintln(w)
			fmt.Fprintf(w, "### %s: %s (%d)\n", label, owner, len(list))
			fmt.Fprintln(w)
			sort.Slice(list, func(i, j int) bool { return list[i].ID.Test < list[j].ID.Test })
			for _, t := range list {
				note := ""
				if t.Unread {
					note = " (cause unread)"
				}
				fmt.Fprintf(w, "- `%s`%s\n", t.ID.Test, note)
			}
		}
	}
	writeSelfSkipped(w, r.SelfSkipped)
	return w.Flush()
}

// writeSelfSkipped lists the passes that skipped themselves, so a run without a capability (Windows' symlink
// privilege) can't quietly drop those cells from coverage. Listed under a `#` heading of its own, which
// `parseBaseline` never reads as a crate.
func writeSelfSkipped(w io.Writer, skipped []testID) {
	sortIDs(skipped)
	fmt.Fprintln(w)
	fmt.Fprintf(w, "# Skipped themselves (%d)\n", len(skipped))
	fmt.Fprintln(w)
	fmt.Fprintln(w, "Passes that returned early and printed the skip marker (`cmdr_fs::testing::SKIP_MARKER`), counted only when the run used `--profile windows-baseline`, which keeps a pass's output in the report. A full run has none: give the builder the symlink privilege (Developer Mode or an elevated shell).")
	if len(skipped) > 0 {
		fmt.Fprintln(w)
	}
	for _, id := range skipped {
		fmt.Fprintf(w, "- %s `%s`\n", id.Crate, id.Test)
	}
}

func sortIDs(ids []testID) {
	sort.Slice(ids, func(i, j int) bool {
		if ids[i].Crate != ids[j].Crate {
			return ids[i].Crate < ids[j].Crate
		}
		return ids[i].Test < ids[j].Test
	})
}

// thousands spells n with `,` between each group of three digits.
func thousands(n int) string {
	digits := fmt.Sprint(n)
	var out strings.Builder
	for i, d := range digits {
		if i > 0 && (len(digits)-i)%3 == 0 {
			out.WriteByte(',')
		}
		out.WriteRune(d)
	}
	return out.String()
}

// parseBaseline reads back the failing tests a rendered baseline lists.
func parseBaseline(r io.Reader) (map[testID]bool, error) {
	out := map[testID]bool{}
	crate := ""
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	for scanner.Scan() {
		line := scanner.Text()
		if rest, ok := strings.CutPrefix(line, "## "); ok {
			crate, _, _ = strings.Cut(rest, " ")
			continue
		}
		// A top-level heading (the self-skipped list) ends the failures.
		if strings.HasPrefix(line, "# ") {
			crate = ""
			continue
		}
		if rest, ok := strings.CutPrefix(line, "- `"); ok && crate != "" {
			name, _, _ := strings.Cut(rest, "`")
			out[testID{Crate: crate, Test: name}] = true
		}
	}
	return out, scanner.Err()
}

func writeDiff(out io.Writer, before map[testID]bool, after report) {
	nowFailing := map[testID]bool{}
	for _, f := range after.Failures {
		nowFailing[f.ID] = true
	}
	var green, red, gone []testID
	for id := range before {
		switch {
		case !after.Ran[id]:
			gone = append(gone, id)
		case !nowFailing[id]:
			green = append(green, id)
		}
	}
	for id := range nowFailing {
		if !before[id] {
			red = append(red, id)
		}
	}
	section := func(title string, ids []testID) {
		sortIDs(ids)
		fmt.Fprintf(out, "%s (%d)\n", title, len(ids))
		for _, id := range ids {
			fmt.Fprintf(out, "- %s `%s`\n", id.Crate, id.Test)
		}
	}
	section("Turned green", green)
	section("Newly red", red)
	section("Listed but didn't run (renamed, removed, or gated off)", gone)
	section("Passed by skipping themselves (no symlink privilege, or another missing capability)", append([]testID{}, after.SelfSkipped...))
}
