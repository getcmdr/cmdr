package main

import (
	"strings"
	"testing"
)

// The lib phase's fixtures are the `cmdr-operations` golden scenarios' shape: code behind a
// feature that doesn't enable `cmdr-fs/testing`, so a plain build can't find `app_path!`.

func runLib(t *testing.T, text string, d diagnostic) string {
	t.Helper()
	r := fixture(map[string]string{fixtureFile: text})
	edits, err := r.libPhaseEdits(&d)
	if err != nil {
		t.Fatal(err)
	}
	return string(applyToText([]byte(text), chooseEdits(edits)))
}

func missingMacro(t *testing.T, text, after string) diagnostic {
	d := diag("", "cannot find macro `app_path` in this scope", spanOf(t, text, "app_path", after, "", true))
	d.Code = nil
	return d
}

func TestAMacroAPlainBuildLacksCrossesTheBridge(t *testing.T) {
	text := "use cmdr_fs::app_path;\n\nasync fn seed(v: &V) {\n    v.create_file(app_path!(\"/docs/note.md\"), b\"# note\").await.expect(\"note\");\n}\n"
	got := runLib(t, text, missingMacro(t, text, "create_file("))
	want := "v.create_file(&AppPath::legacy_unchecked(Path::new(\"/docs/note.md\")), b\"# note\")"
	if !strings.Contains(got, want) {
		t.Fatalf("not bridged:\n%s", got)
	}
}

func TestTheLibPhaseImportsWhatTheBridgeFormNames(t *testing.T) {
	text := "use cmdr_fs::app_path;\n\nfn t() { AppPath::legacy_unchecked(Path::new(\"/a\")); }\n"
	got := runLib(t, text, diag("E0433", "failed to resolve: use of undeclared type `Path`", spanOf(t, text, "Path", "unchecked(", "", true)))
	if !strings.Contains(got, "use std::path::Path;\n") {
		t.Fatalf("Path not imported:\n%s", got)
	}
}

func TestTheLibPhaseLeavesEverythingElseAsResidue(t *testing.T) {
	text := "fn t() { v.create_file(other!(\"/a\")); x.y(); }\n"
	if got := runLib(t, text, diag("E0308", "mismatched types", spanOf(t, text, "x.y()", "", "expected `&AppPath`, found `&Path`", true))); got != text {
		t.Fatalf("the lib phase rewrote a mismatch:\n%s", got)
	}
	d := diag("", "cannot find macro `app_path` in this scope", spanOf(t, text, "other", "", "", true))
	if got := runLib(t, text, d); got != text {
		t.Fatalf("the lib phase rewrote another macro:\n%s", got)
	}
}

func TestAnAppPathImportTheLibPhaseEmptiedGoes(t *testing.T) {
	text := "use cmdr_fs::app_path;\nuse cmdr_fs::app_path::AppPath;\n"
	empty, applicable := "", "MachineApplicable"
	d := diagnostic{Message: "unused import: `cmdr_fs::app_path`", Code: &diagCode{Code: "unused_imports"}, Level: "warning",
		Spans:    []diagSpan{spanOf(t, text, "cmdr_fs::app_path", "", "", true)},
		Children: []diagnostic{{Message: "remove the whole `use` item", Spans: []diagSpan{{FileName: fixtureFile, ByteStart: 0, ByteEnd: 23, SuggestedReplacement: &empty, SuggestionApplicability: &applicable}}}}}
	if got := run(t, text, d); got != "use cmdr_fs::app_path::AppPath;\n" {
		t.Fatalf("unused app_path import not removed: %q", got)
	}
	d.Level = "error" // a crate that denies the lint
	if got := runLib(t, text, d); got != "use cmdr_fs::app_path::AppPath;\n" {
		t.Fatalf("the lib phase kept the emptied import: %q", got)
	}
}
