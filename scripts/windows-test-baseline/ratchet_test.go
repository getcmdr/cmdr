package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func entry(test string, status listStatus) listEntry {
	return listEntry{ID: testID{Crate: "cmdr-fs", Test: test}, Status: status, Reason: "a reason"}
}

func runOf(passed []string, failed []string, selfSkipped []string) report {
	r := report{Ran: map[testID]bool{}}
	for _, name := range passed {
		r.Ran[testID{"cmdr-fs", name}] = true
	}
	for _, name := range failed {
		id := testID{"cmdr-fs", name}
		r.Ran[id] = true
		r.Failures = append(r.Failures, failure{ID: id, Text: "panicked at src/a.rs:1:2:\nboom"})
	}
	for _, name := range selfSkipped {
		id := testID{"cmdr-fs", name}
		r.Ran[id] = true
		r.SelfSkipped = append(r.SelfSkipped, id)
	}
	return r
}

func TestARunMatchingItsListIsClean(t *testing.T) {
	list := []listEntry{entry("a", statusFails), entry("b", statusFlaky), entry("c", statusBuilder)}
	if v := judge(list, runOf([]string{"b", "ok"}, []string{"a"}, nil)); !v.clean() {
		t.Fatalf("got %+v", v)
	}
	// A flaky entry may fail too.
	if v := judge(list, runOf([]string{"ok"}, []string{"a", "b"}, nil)); !v.clean() {
		t.Fatalf("got %+v", v)
	}
}

func TestEachMismatchIsNamed(t *testing.T) {
	list := []listEntry{entry("fixed", statusFails), entry("renamed", statusFails), entry("still", statusFails)}
	v := judge(list, runOf([]string{"fixed"}, []string{"still", "new"}, []string{"linkless"}))
	if len(v.Regressions) != 1 || v.Regressions[0].ID.Test != "new" {
		t.Errorf("regressions: %+v", v.Regressions)
	}
	if len(v.NowPassing) != 1 || v.NowPassing[0].Test != "fixed" {
		t.Errorf("now passing: %+v", v.NowPassing)
	}
	if len(v.Stale) != 1 || v.Stale[0].Test != "renamed" {
		t.Errorf("stale: %+v", v.Stale)
	}
	if len(v.SelfSkipped) != 1 || v.SelfSkipped[0].Test != "linkless" {
		t.Errorf("self-skipped: %+v", v.SelfSkipped)
	}
}

func TestTighteningIsFreeAndLooseningTakesAReason(t *testing.T) {
	list := []listEntry{entry("fixed", statusFails), entry("still", statusFails)}
	r := runOf([]string{"fixed"}, []string{"still", "new"}, nil)
	tight := tighten(list, judge(list, r), "")
	if len(tight) != 1 || tight[0].ID.Test != "still" {
		t.Fatalf("tightened without a reason: %+v", tight)
	}
	if v := judge(tight, r); v.clean() {
		t.Fatal("the new failure must still show until it's listed with a reason")
	}
	loose := tighten(list, judge(list, r), "#1225 app paths")
	if v := judge(loose, r); !v.clean() {
		t.Fatalf("got %+v", v)
	}
	for _, e := range loose {
		if e.ID.Test == "new" && (e.Status != statusFails || e.Reason != "#1225 app paths") {
			t.Fatalf("new entry: %+v", e)
		}
	}
}

func TestTheListRoundTripsWithItsPreamble(t *testing.T) {
	data := "# what this is\n" + listHeader + "\ncmdr-fs\tb\tfails\twhy b\ncmdr\ta\tbuilder\tno second drive\n"
	entries, err := readList(strings.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	var out bytes.Buffer
	if err := writeList(&out, readPreamble(data), entries); err != nil {
		t.Fatal(err)
	}
	want := "# what this is\n" + listHeader + "\ncmdr\ta\tbuilder\tno second drive\ncmdr-fs\tb\tfails\twhy b\n"
	if out.String() != want {
		t.Fatalf("got:\n%s\nwant:\n%s", out.String(), want)
	}
}

func TestAnEntryWithoutAReasonIsRefused(t *testing.T) {
	if _, err := readList(strings.NewReader("cmdr-fs\ta\tfails\t \n")); err == nil {
		t.Fatal("want an error")
	}
	if _, err := readList(strings.NewReader("cmdr-fs\ta\tbroken\twhy\n")); err == nil {
		t.Fatal("want an error for an unknown status")
	}
}

func TestTheBuilderFilterLeavesOnlyBuilderEntriesOut(t *testing.T) {
	got := builderFilter([]listEntry{entry("a", statusFails), entry("x::y", statusBuilder)})
	if got != "not ((package(cmdr-fs) & test(=x::y)))" {
		t.Fatalf("got %q", got)
	}
	if builderFilter([]listEntry{entry("a", statusFails)}) != "" {
		t.Fatal("no builder entry, no filter")
	}
}

// nextest's JUnit for a run with retries: a pass after a failed try carries `flakyFailure`, a failure on every try
// carries `failure` plus a `rerunFailure` per retry.
const retriedJUnit = `<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="1" errors="0">
  <testsuite name="cmdr-index" tests="3" failures="1">
    <testcase name="store::tests::flips" classname="cmdr-index"><flakyFailure message="timeout" type="test timeout"/><system-out>running 1 test</system-out></testcase>
    <testcase name="store::tests::always" classname="cmdr-index"><failure message="boom" type="test failure"/><rerunFailure message="boom" type="test failure"/><rerunFailure message="boom" type="test failure"/></testcase>
    <testcase name="store::tests::fine" classname="cmdr-index"/>
  </testsuite>
</testsuites>`

func writeJUnit(t *testing.T, xml string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "junit.xml")
	if err := os.WriteFile(path, []byte(xml), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestARetriedRunReadsPassesOnARetryApartFromFailures(t *testing.T) {
	r, err := readReport(writeJUnit(t, retriedJUnit))
	if err != nil {
		t.Fatal(err)
	}
	if len(r.Failures) != 1 || r.Failures[0].ID.Test != "store::tests::always" {
		t.Fatalf("failures: %+v", r.Failures)
	}
	if len(r.Flaky) != 1 || r.Flaky[0].Test != "store::tests::flips" {
		t.Fatalf("flaky: %+v", r.Flaky)
	}
	if len(r.Ran) != 3 {
		t.Fatalf("ran: %v", r.Ran)
	}
}

func TestAListedFailureThatPassesOnARetryStillFails(t *testing.T) {
	list := []listEntry{entry("a", statusFails)}
	r := runOf(nil, nil, nil)
	r.Ran[testID{"cmdr-fs", "a"}] = true
	r.Flaky = []testID{{"cmdr-fs", "a"}}
	if v := judge(list, r); !v.clean() {
		t.Fatalf("a listed failure that failed once and then passed isn't fixed: %+v", v)
	}
}

func TestAnUnlistedPassOnARetryIsNamedLoudlyButStaysGreen(t *testing.T) {
	r := runOf([]string{"b"}, nil, nil)
	r.Flaky = []testID{{"cmdr-fs", "b"}}
	v := judge(nil, r)
	if !v.clean() {
		t.Fatalf("a pass on a retry must not block a landing: %+v", v)
	}
	if len(v.Flaky) != 1 || v.Flaky[0].Test != "b" {
		t.Fatalf("flaky: %+v", v.Flaky)
	}
	var out bytes.Buffer
	writeVerdict(&out, nil, v)
	if !strings.Contains(out.String(), "Passed on a retry (1)") || !strings.Contains(out.String(), "cmdr-fs `b`") {
		t.Fatalf("verdict:\n%s", out.String())
	}
	// A listed flaky entry that flipped is what the entry says, so it's not named again.
	if v := judge([]listEntry{entry("b", statusFlaky)}, r); len(v.Flaky) != 0 {
		t.Fatalf("flaky: %+v", v.Flaky)
	}
}

func TestATestNamedTwiceInTheReportIsRefused(t *testing.T) {
	twice := `<testsuites><testsuite name="cmdr-fs"><testcase name="a"/></testsuite><testsuite name="cmdr-fs::integration"><testcase name="a"/></testsuite></testsuites>`
	if _, err := readReport(writeJUnit(t, twice)); err == nil {
		t.Fatal("two binaries of one crate with one test name can't be told apart on the list: want an error")
	}
}

func TestAMismatchNamesTheExactCommandForThisRun(t *testing.T) {
	t.Setenv("GITHUB_RUN_ID", "44941795618")
	v := judge([]listEntry{entry("fixed", statusFails)}, runOf([]string{"fixed"}, nil, nil))
	var out bytes.Buffer
	writeVerdict(&out, []listEntry{entry("fixed", statusFails)}, v)
	if !strings.Contains(out.String(), "go -C scripts/windows-test-baseline run . ratchet -run 44941795618 -write") {
		t.Fatalf("verdict:\n%s", out.String())
	}
}

func TestAnEmptyOrMissingReportFails(t *testing.T) {
	if _, err := loadRun(writeJUnit(t, `<testsuites name="nextest-run" tests="0"></testsuites>`), ""); err == nil {
		t.Fatal("a report where nothing ran must fail")
	}
	if _, err := loadRun(filepath.Join(t.TempDir(), "absent.xml"), ""); err == nil {
		t.Fatal("a missing report must fail")
	}
	if _, err := loadRun(writeJUnit(t, `<testsuites><testsuite name="cmdr"><testcase name="a">`), ""); err == nil {
		t.Fatal("a truncated report must fail")
	}
}
