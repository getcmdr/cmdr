package main

import (
	"bytes"
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
