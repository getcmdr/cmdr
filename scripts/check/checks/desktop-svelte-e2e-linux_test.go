package checks

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

// The in-page menu group's report sits beside the suite's under the name `e2e-linux.sh` derives
// (`${report%.json}-in-page-menus.json`), so its results reach the per-test records too.
func TestLinuxE2EReportPathsIncludeTheInPageGroup(t *testing.T) {
	t.Parallel()
	got := linuxE2EReportPaths(4242)
	want := []string{"/tmp/cmdr-e2e-report-linux-4242.json", "/tmp/cmdr-e2e-report-linux-4242-in-page-menus.json"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("linuxE2EReportPaths(4242) = %v, want %v", got, want)
	}
}

// A pre-created report the run never wrote is no report: the duration and flake readers would
// fail on it.
func TestWrittenReportsSkipsAnEmptyOne(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	written := filepath.Join(dir, "written.json")
	empty := filepath.Join(dir, "empty.json")
	if err := os.WriteFile(written, []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(empty, nil, 0o644); err != nil {
		t.Fatal(err)
	}
	got := writtenReports([]string{written, empty, filepath.Join(dir, "missing.json")})
	if !reflect.DeepEqual(got, []string{written}) {
		t.Fatalf("writtenReports = %v, want only %s", got, written)
	}
}
