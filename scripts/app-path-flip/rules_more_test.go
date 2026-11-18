package main

import (
	"strings"
	"testing"
)

// The rules the worked example (`cmdr-archive`) and `cmdr-fs` added to the first set.

func TestAnAsyncHelperCallMovesIntoItsFuture(t *testing.T) {
	text := implHead + "    fn scan_for_copy<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n        let path_std = path.as_std_legacy();\n        let path: &Path = &path_std;\n        Box::pin(self.scan_subtree(path, MacOnlyFiles::Include))\n    }\n}\nimpl Demo {\n    async fn scan_subtree(&self, path: &Path, m: MacOnlyFiles) {}\n}\n"
	got := run(t, text, diag("E0515", "cannot return value referencing local variable `path_std`",
		spanOf(t, text, "Box::pin(self.scan_subtree(path, MacOnlyFiles::Include))", "", "returns a value referencing data owned by the current function", true)))
	want := "{ Box::pin(async move { let path_std = path.as_std_legacy(); let path: &Path = &path_std; self.scan_subtree(path, MacOnlyFiles::Include).await }) }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("not moved into the future:\n%s", got)
	}
}

func TestANonAsyncHelperStaysResidue(t *testing.T) {
	text := implHead + "    fn open_read_stream<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n        let path_std = path.as_std_legacy();\n        let path: &Path = &path_std;\n        Box::pin(self.open_stream(path, 0))\n    }\n}\nimpl Demo {\n    fn open_stream<'a>(&'a self, path: &'a Path, offset: u64) -> Fut<'a> { todo!() }\n}\n"
	got := run(t, text, diag("E0515", "cannot return value referencing local variable `path_std`",
		spanOf(t, text, "Box::pin(self.open_stream(path, 0))", "", "", true)))
	if got != text {
		t.Fatalf("a helper with work before its future must stay residue:\n%s", got)
	}
}

func TestAnAssertComparingATypedValueDropsItToStd(t *testing.T) {
	text := "fn t() {\n    assert_eq!(echo.stripped, Path::new(\"/photos\"));\n}\n"
	start := strings.Index(text, "assert_eq!")
	end := strings.Index(text, ";\n}")
	d := diag("E0277", "can't compare `app_path::AppPathBuf` with `&std::path::Path`",
		diagSpan{FileName: fixtureFile, ByteStart: start, ByteEnd: end, IsPrimary: true, Macro: "assert_eq"})
	got := run(t, text, d)
	if !strings.Contains(got, "assert_eq!(&*echo.stripped.as_std_legacy(), Path::new(\"/photos\"))") {
		t.Fatalf("typed operand not dropped:\n%s", got)
	}
}

func TestAComparisonAgainstABridgedFieldReadsItsPath(t *testing.T) {
	text := "fn t() {\n    assert_eq!(root.app_root(), Path::new(\"sftp://h/\"));\n}\n"
	start := strings.Index(text, "assert_eq!")
	end := strings.Index(text, ";\n}")
	d := diag("E0369", "binary operation `==` cannot be applied to type `&bridge::BridgedPath`",
		diagSpan{FileName: fixtureFile, ByteStart: start, ByteEnd: end, IsPrimary: true, Macro: "assert_eq"})
	other := "/rustc/x/library/core/src/macros/mod.rs"
	l1, l2 := "&bridge::BridgedPath", "&std::path::Path"
	d.Spans = append(d.Spans, diagSpan{FileName: other, Label: &l1}, diagSpan{FileName: other, Label: &l2})
	got := run(t, text, d)
	if !strings.Contains(got, "assert_eq!(root.app_root().as_path(), Path::new(\"sftp://h/\"))") {
		t.Fatalf("bridged operand not dropped:\n%s", got)
	}
}

func TestAMacroSiteGetsOnlyTheComparisonRule(t *testing.T) {
	text := "fn t() {\n    assert!(matches!(x, y));\n}\n"
	d := diag("E0308", "mismatched types", diagSpan{FileName: fixtureFile, ByteStart: 13, ByteEnd: 35, IsPrimary: true, Macro: "matches"})
	label := "expected `&AppPath`, found `&Path`"
	d.Spans[0].Label = &label
	if got := run(t, text, d); got != text {
		t.Fatalf("a lifted macro span must not be rewritten whole:\n%s", got)
	}
}

func TestAnImportWithoutARustcSuggestionGoesAtTheModuleHead(t *testing.T) {
	text := "//! Docs.\n\nuse std::sync::Arc;\n\nfn t() { AppPath::legacy_unchecked(p); }\n\n#[cfg(test)]\nmod tests {\n    use super::Thing;\n    fn u() { AppPathBuf::new(); }\n}\n"
	got := run(t, text, diag("E0433", "failed to resolve: use of undeclared type `AppPath`", spanOf(t, text, "AppPath", "fn t() { ", "", true)))
	if !strings.Contains(got, "\n\nuse cmdr_fs::app_path::AppPath;\nuse std::sync::Arc;") {
		t.Fatalf("file-level import not placed:\n%s", got)
	}
	got = run(t, got, diag("E0433", "failed to resolve: use of undeclared type `AppPathBuf`", spanOf(t, got, "AppPathBuf", "fn u() { ", "", true)))
	if !strings.Contains(squash(got), "mod tests { use cmdr_fs::app_path::AppPathBuf; use super::Thing;") {
		t.Fatalf("inline-module import not placed:\n%s", got)
	}
	again := run(t, got, diag("E0433", "failed to resolve: use of undeclared type `AppPathBuf`", spanOf(t, got, "AppPathBuf::new", "", "", true)))
	if again != got {
		t.Fatalf("a re-run must never stack a duplicate import:\n%s", again)
	}
}

func TestABorrowedReturnThroughAnAccessorIsNoField(t *testing.T) {
	text := "pub struct Demo {\n    home: Option<PathBuf>,\n}\n" + implHead + "    fn stands_in_for_own_disk(&self) -> Option<&AppPath> {\n        self.home()\n    }\n}\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "self.home()", "", "expected `Option<&AppPath>`, found `Option<&Path>`", true)))
	if got != text {
		t.Fatalf("an accessor call must not retype the field of its name:\n%s", got)
	}
}

func TestABodyIsLeftAloneWhileItsSignatureIsRetyped(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n        self.inner.get_metadata(path)\n    }\n}\n"
	r := fixture(map[string]string{fixtureFile: text})
	diags := []diagnostic{
		diag("E0053", "m", spanOf(t, text, "&'a Path", "", "expected `AppPath`, found `Path`", true)),
		diag("E0308", "mismatched types", spanOf(t, text, "path", "get_metadata(", "expected `&AppPath`, found `&Path`", true)),
	}
	retyping, err := methodsBeingRetyped(r, diags)
	if err != nil || len(retyping) != 1 {
		t.Fatalf("retyping = %v, err = %v", retyping, err)
	}
}

func TestNormalizeType(t *testing.T) {
	for in, want := range map[string]string{
		"&'a std::path::Path":                "&Path",
		"std::option::Option<&'a Path>":      "Option<&Path>",
		"&Cow<'_, Path>":                     "&Cow<Path>",
		"&'a [std::path::PathBuf]":           "&[PathBuf]",
		"&cmdr_fs::app_path::AppPath":        "&AppPath",
		"std::vec::Vec<(PathBuf, CopyScan)>": "Vec<(PathBuf,CopyScan)>",
	} {
		if got := normalizeType(in); got != want {
			t.Errorf("normalizeType(%q) = %q, want %q", in, got, want)
		}
	}
}

func TestAnAssertWhoseTypedOperandCameFirstDropsItToStd(t *testing.T) {
	text := "fn t() {\n    assert_eq!(volume.stands_in_for_own_disk(), Some(Path::new(\"/Users/you\")));\n}\n"
	got := run(t, text, diag("E0308", "mismatched types",
		spanOf(t, text, "Some(Path::new(\"/Users/you\"))", "", "expected `Option<&AppPath>`, found `Option<&Path>`", true)))
	want := "assert_eq!(volume.stands_in_for_own_disk().map(AppPath::as_std_legacy).as_deref(), Some(Path::new(\"/Users/you\")));"
	if !strings.Contains(got, want) {
		t.Fatalf("typed operand not dropped, or the expected value touched:\n%s", got)
	}
	call := "fn t() {\n    check(volume.stands_in_for_own_disk(), Some(Path::new(\"/u\")));\n}\n"
	if got := run(t, call, diag("E0308", "mismatched types",
		spanOf(t, call, "Some(Path::new(\"/u\"))", "", "expected `Option<&AppPath>`, found `Option<&Path>`", true))); got != call {
		t.Fatalf("an argument of a plain call is no comparison:\n%s", got)
	}
}

// squash collapses every whitespace run to one space: rustfmt owns the layout.
func squash(s string) string {
	return strings.Join(strings.Fields(s), " ")
}
