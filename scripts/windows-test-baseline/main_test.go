package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestClassifyTextReadsEachCauseFromItsOutput(t *testing.T) {
	cases := map[string]cluster{
		`panicked: not an absolute Windows app path: C:\Users\Admin\AppData\Local\Temp\.tmpX`: clusterNativeTempdir,
		`left: "/srv/data\\photos"`:                                    clusterStdArithmetic,
		`NotFound("sftp://test_user_1@h:22\\srv")`:                     clusterStdArithmetic,
		`rename_local_exclusive refused \\?\C:\x`:                      clusterFinalize,
		`Err(MountedVolumeInside)`:                                     clusterIdentity,
		`A folyamat nem fér hozzá a fájlhoz (os error 32)`:             clusterOpenFile,
		`"mountPoint": "<tmp>\\docs", "type": "mounted_volume_inside"`: clusterIdentity,
		`symlink: Os { code: 1314, kind: Uncategorized, message: "A required privilege is not held by the client." }`:                     clusterSymlink,
		`start the test ledger empty: Os { code: 5, kind: PermissionDenied, message: "Access is denied." }`:                               clusterAppendTruncate,
		`folders: Os { code: 123, kind: InvalidFilename, message: "The filename, directory name, or volume label syntax is incorrect." }`: clusterWindowsNames,
		`HOME is set: NotPresent`: clusterHomeEnv,
		`left: Directory { path: "C:/Users/Admin" } right: Directory { path: "C:\\Users\\Admin" }`: clusterNativeHome,
		`right: ["/Users/test_user_3/Documents"]`:                                                  clusterPosixFixture,
		`the transfer must complete`:                                                               clusterUnread,
		`C:\Users\Admin (os error 2)`:                                                              clusterUnread,
	}
	for text, want := range cases {
		if got := classifyText(text); got != want {
			t.Errorf("classifyText(%q) = %s, want %s", text, got.Key, want.Key)
		}
	}
}

func TestAZipModeCellIsNamedByItsTest(t *testing.T) {
	f := failure{ID: testID{"cmdr-archive", "volume::volume_test::a_unix_mode_in_the_external_attributes_reaches_the_entry"}, Text: "left: 438"}
	if got := classifyOne(f); got != clusterZipCreator {
		t.Fatalf("got %s", got.Key)
	}
}

func TestAnUnreadFailureNeverInheritsAnotherTestsOwnCause(t *testing.T) {
	tags := classifyAll([]failure{
		{ID: testID{"cmdr-operations", "in_flight_temps::tests::a"}, Text: `start the test ledger empty: Os { code: 5, kind: PermissionDenied }`},
		{ID: testID{"cmdr-operations", "in_flight_temps::tests::b"}, Text: "the remote edit should commit"},
	})
	if tags[1].Cluster != clusterUnread {
		t.Fatalf("got %+v", tags[1])
	}
}

func TestAnUnreadFailureTakesItsNearestSiblingsClusterMarkedUnread(t *testing.T) {
	tags := classifyAll([]failure{
		{ID: testID{"cmdr-operations", "transfer::copy::tests::a"}, Text: `left: "/x\\y"`},
		{ID: testID{"cmdr-operations", "transfer::copy::tests::b"}, Text: "the transfer must complete"},
		{ID: testID{"cmdr-operations", "delete::tests::c"}, Text: "MountedVolumeInside"},
	})
	if tags[1].Cluster != clusterStdArithmetic || !tags[1].Unread {
		t.Fatalf("got %+v", tags[1])
	}
	if tags[1].Owner != "this crate's app path conversion" {
		t.Fatalf("owner %q", tags[1].Owner)
	}
	if tags[2].Owner != "Windows plan task 35" {
		t.Fatalf("owner %q", tags[2].Owner)
	}
}

const sampleJUnit = `<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="2" errors="0">
  <testsuite name="cmdr-fs::lib" tests="2" failures="1">
    <testcase name="volume::a" classname="cmdr-fs::lib"/>
    <testcase name="volume::b" classname="cmdr-fs::lib"><failure type="test failure" message="code=101">thread panicked: left: "/a\\b"</failure></testcase>
  </testsuite>
  <testsuite name="cmdr" tests="1" failures="1">
    <testcase name="go_to_path::tests::c" classname="cmdr"><failure type="test failure">no cause here</failure><system-err>panicked: not an absolute Windows app path</system-err></testcase>
  </testsuite>
</testsuites>`

func TestARenderedBaselineReadsBackAndDiffsByName(t *testing.T) {
	dir := t.TempDir()
	junit := filepath.Join(dir, "junit.xml")
	if err := os.WriteFile(junit, []byte(sampleJUnit), 0o600); err != nil {
		t.Fatal(err)
	}
	r, err := readReport(junit)
	if err != nil {
		t.Fatal(err)
	}
	var md bytes.Buffer
	if err := render(&md, r, "abc123", "2026-11-18"); err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"## cmdr (1)", "## cmdr-fs (1)", "the app's file system and listings conversion", "- `volume::b`"} {
		if !strings.Contains(md.String(), want) {
			t.Fatalf("missing %q in:\n%s", want, md.String())
		}
	}
	listed, err := parseBaseline(&md)
	if err != nil {
		t.Fatal(err)
	}
	if len(listed) != 2 || !listed[testID{"cmdr-fs", "volume::b"}] || !listed[testID{"cmdr", "go_to_path::tests::c"}] {
		t.Fatalf("read back %v", listed)
	}

	listed[testID{"cmdr-fs", "volume::a"}] = true
	var diff bytes.Buffer
	writeDiff(&diff, listed, r)
	if !strings.Contains(diff.String(), "Turned green (1)\n- cmdr-fs `volume::a`") {
		t.Fatalf("diff:\n%s", diff.String())
	}
}
