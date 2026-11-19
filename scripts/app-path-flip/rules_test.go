package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// Each rule runs on a small fixture with the diagnostic rustc gives for it, shaped as the
// real ones the example crates produced (spans, labels, the "defined here" children).

const fixtureFile = "crates/demo/src/lib.rs"

// fixture builds a rewriter over in-memory files.
func fixture(files map[string]string) *rewriter {
	var names []string
	for name := range files {
		names = append(names, name)
	}
	return &rewriter{crate: "demo", crateRust: names, load: func(file string) (*source, error) {
		text, ok := files[file]
		if !ok {
			return nil, os.ErrNotExist
		}
		return newSource(file, []byte(text)), nil
	}}
}

// spanOf is the span over the first occurrence of needle past the end of `after` (when given).
func spanOf(t *testing.T, text, needle, after string, label string, primary bool) diagSpan {
	t.Helper()
	from := 0
	if after != "" {
		from = strings.Index(text, after)
		if from < 0 {
			t.Fatalf("no %q in fixture", after)
		}
		from += len(after)
	}
	at := strings.Index(text[from:], needle)
	if at < 0 {
		t.Fatalf("no %q in fixture", needle)
	}
	var l *string
	if label != "" {
		l = &label
	}
	return diagSpan{FileName: fixtureFile, ByteStart: from + at, ByteEnd: from + at + len(needle), IsPrimary: primary, Label: l}
}

func run(t *testing.T, text string, d diagnostic) string {
	t.Helper()
	r := fixture(map[string]string{fixtureFile: text})
	edits, err := r.editsFor(&d)
	if err != nil {
		t.Fatal(err)
	}
	return string(applyToText([]byte(text), chooseEdits(edits)))
}

func diag(code, message string, spans ...diagSpan) diagnostic {
	return diagnostic{Message: message, Code: &diagCode{Code: code}, Level: "error", Spans: spans}
}

func definedIn(file string) diagnostic {
	return diagnostic{Message: "method defined here", Spans: []diagSpan{{FileName: file}}}
}

const implHead = "impl Volume for Demo {\n"

func TestE0053RetypesTheParam(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n        self.inner.get_metadata(path)\n    }\n}\n"
	got := run(t, text, diag("E0053", "method `get_metadata` has an incompatible type for trait",
		spanOf(t, text, "&'a Path", "", "expected `AppPath`, found `Path`", true)))
	if !strings.Contains(got, "path: &'a AppPath)") {
		t.Fatalf("param not retyped:\n%s", got)
	}
}

func TestE0053RetypesABatchAndAReturn(t *testing.T) {
	text := implHead + "    fn delete_files<'a>(&'a self, paths: &'a [PathBuf]) -> Fut<'a> { todo!() }\n    fn local_path(&self) -> Option<PathBuf> { None }\n}\n"
	got := run(t, text, diag("E0053", "m", spanOf(t, text, "&'a [PathBuf]", "", "expected `AppPathBuf`, found `PathBuf`", true)))
	got = run(t, got, diag("E0053", "m", spanOf(t, got, "Option<PathBuf>", "", "expected `AppPathBuf`, found `PathBuf`", true)))
	if !strings.Contains(got, "paths: &'a [AppPathBuf]") || !strings.Contains(got, "-> Option<AppPathBuf>") {
		t.Fatalf("not retyped:\n%s", got)
	}
}

func TestE0053RetypesAFullyQualifiedStdPathInAMacroBody(t *testing.T) {
	text := "macro_rules! fwd {\n    () => {\n        fn exists<'a>(&'a self, path: &'a ::std::path::Path) -> Fut<'a> { todo!() }\n        fn local_path(&self) -> Option<::std::path::PathBuf> { None }\n    };\n}\n"
	got := run(t, text, diag("E0053", "m", spanOf(t, text, "&'a ::std::path::Path", "", "expected `AppPath`, found `Path`", true)))
	got = run(t, got, diag("E0053", "m", spanOf(t, got, "Option<::std::path::PathBuf>", "", "expected `AppPathBuf`, found `PathBuf`", true)))
	if !strings.Contains(got, "path: &'a ::cmdr_fs::app_path::AppPath)") || !strings.Contains(got, "-> Option<::cmdr_fs::app_path::AppPathBuf>") {
		t.Fatalf("not retyped by its full path:\n%s", got)
	}
}

func TestE0053LeavesAReverseMismatchAlone(t *testing.T) {
	text := implHead + "    fn create_symlink<'a>(&'a self, path: &'a AppPath, target: &'a AppPath) {}\n}\n"
	got := run(t, text, diag("E0053", "m", spanOf(t, text, "&'a AppPath)", "", "expected `Path`, found `AppPath`", true)))
	if got != text {
		t.Fatalf("a link target must keep `Path`:\n%s", got)
	}
}

func TestPreludeGoesAtTheTopWhenTheBodyUsesTheParamOutsideAFuture(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n        let inner = self.inner_path(path);\n        Box::pin(async move { self.index(inner).await })\n    }\n}\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "path", "inner_path(", "expected `&Path`, found `&AppPath`", true)))
	want := "-> Fut<'a> {\nlet path_std = path.as_std_legacy();\nlet path: &Path = &path_std;\n        let inner"
	if !strings.Contains(got, want) {
		t.Fatalf("prelude not at the top:\n%s", got)
	}
}

func TestPreludeGoesInsideTheFutureWhenEveryUseIsThere(t *testing.T) {
	text := implHead + "    fn exists<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n        Box::pin(async move { path.exists() })\n    }\n}\n"
	got := run(t, text, diag("E0599", "no method named `exists` found for reference `&'a AppPath` in the current scope",
		spanOf(t, text, "exists", "path.", "method not found in `&'a AppPath`", true)))
	if !strings.Contains(got, "async move {\nlet path_std = path.as_std_legacy();\nlet path: &Path = &path_std; path.exists()") {
		t.Fatalf("prelude not inside the future:\n%s", got)
	}
}

func TestPreludeForABatch(t *testing.T) {
	text := implHead + "    fn delete_files<'a>(&'a self, paths: &'a [AppPathBuf]) -> Fut<'a> {\n        let n = count(paths);\n        todo!()\n    }\n}\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "paths", "count(", "expected `&[PathBuf]`, found `&[AppPathBuf]`", true)))
	if !strings.Contains(got, "let paths_std: Vec<PathBuf> = paths.iter().map(|p| p.as_std_legacy().into_owned()).collect();\nlet paths: &[PathBuf] = &paths_std;") {
		t.Fatalf("no batch prelude:\n%s", got)
	}
}

func TestNoPreludeOutsideAVolumeImpl(t *testing.T) {
	text := "fn helper(path: &AppPath) {\n    native(path);\n}\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "path", "native(", "expected `&Path`, found `&AppPath`", true)))
	if strings.Contains(got, "path_std") {
		t.Fatalf("a non-impl fn got a prelude:\n%s", got)
	}
	if !strings.Contains(got, "native(&path.as_std_legacy())") {
		t.Fatalf("the typed value should drop to `as_std_legacy`:\n%s", got)
	}
}

func TestATypedParamHandedBackUsesTheOriginal(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\nlet path_std = path.as_std_legacy();\nlet path: &Path = &path_std;\n        if path == self.fail { return err(); }\n        self.inner.get_metadata(path)\n    }\n}\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "path", "get_metadata(", "expected `&AppPath`, found `&Path`", true)))
	if !strings.Contains(got, "let path_app = path;\nlet path_std") || !strings.Contains(got, "self.inner.get_metadata(path_app)") {
		t.Fatalf("not handed back:\n%s", got)
	}
}

func TestACanonicalLiteralBecomesTheMacro(t *testing.T) {
	text := "fn t() { volume.get_metadata(Path::new(\"/a/b\")); volume.get_metadata(Path::new(\"/a/\")); }\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "Path::new(\"/a/b\")", "", "expected `&AppPath`, found `&Path`", true)))
	got = run(t, got, diag("E0308", "mismatched types", spanOf(t, got, "Path::new(\"/a/\")", "", "expected `&AppPath`, found `&Path`", true)))
	if !strings.Contains(got, "get_metadata(app_path!(\"/a/b\"))") {
		t.Fatalf("canonical literal not converted:\n%s", got)
	}
	if !strings.Contains(got, "get_metadata(&AppPath::legacy_unchecked(Path::new(\"/a/\")))") {
		t.Fatalf("a non-canonical literal must stay on the bridge:\n%s", got)
	}
}

func TestARuntimeValueGoesThroughTheBridge(t *testing.T) {
	text := "fn t() { volume.list_directory(&dir, None); volume.get_metadata(&p); }\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "&dir", "", "expected `&AppPath`, found `&PathBuf`", true)))
	got = run(t, got, diag("E0308", "mismatched types", spanOf(t, got, "&p", "", "expected `&AppPath`, found `&&Path`", true)))
	if !strings.Contains(got, "list_directory(&AppPath::legacy_unchecked(&dir), None)") {
		t.Fatalf("not bridged:\n%s", got)
	}
	if !strings.Contains(got, "get_metadata(&AppPath::legacy_unchecked(p))") {
		t.Fatalf("an extra borrow should go:\n%s", got)
	}
}

func TestSeveralMismatchedArgumentsOfOneCall(t *testing.T) {
	text := "fn t() { volume.rename(&from, &to, false); }\n"
	d := diag("E0308", "arguments to this method are incorrect",
		spanOf(t, text, "&from", "", "expected `&AppPath`, found `&PathBuf`", false),
		spanOf(t, text, "&to", "", "expected `&AppPath`, found `&PathBuf`", false),
		spanOf(t, text, "rename", "", "", true))
	got := run(t, text, d)
	if !strings.Contains(got, "rename(&AppPath::legacy_unchecked(&from), &AppPath::legacy_unchecked(&to), false)") {
		t.Fatalf("both args not bridged:\n%s", got)
	}
}

func TestABatchGoesThroughTheBridge(t *testing.T) {
	text := "fn t() { volume.delete_files(&paths); }\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "&paths", "", "expected `&[AppPathBuf]`, found `&Vec<PathBuf>`", true)))
	if !strings.Contains(got, "delete_files(&paths.iter().map(|p| AppPath::legacy_unchecked(p).into_owned()).collect::<Vec<_>>())") {
		t.Fatalf("batch not bridged:\n%s", got)
	}
}

func TestAReturnedTypedValueUsedAsAPath(t *testing.T) {
	text := "fn t() { native(volume.root()); let p: PathBuf = volume.listing_path(x); }\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "volume.root()", "", "expected `&Path`, found `&AppPath`", true)))
	got = run(t, got, diag("E0308", "mismatched types", spanOf(t, got, "volume.listing_path(x)", "", "expected `PathBuf`, found `AppPathBuf`", true)))
	if !strings.Contains(got, "native(&volume.root().as_std_legacy())") || !strings.Contains(got, "volume.listing_path(x).as_std_legacy().into_owned()") {
		t.Fatalf("not dropped to std:\n%s", got)
	}
}

func TestAnOwnedValueIntoATypedField(t *testing.T) {
	text := "fn t() { let r = Resolved { path, routed: None }; let s = Echo { stripped: a.join(b) }; }\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "path", "", "expected `AppPathBuf`, found `PathBuf`", true)))
	got = run(t, got, diag("E0308", "mismatched types", spanOf(t, got, "a.join(b)", "", "expected `AppPathBuf`, found `PathBuf`", true)))
	if !strings.Contains(got, "Resolved { path: AppPath::legacy_unchecked(&path).into_owned(), routed") {
		t.Fatalf("shorthand field not spelled out:\n%s", got)
	}
	if !strings.Contains(got, "stripped: AppPath::legacy_unchecked(&a.join(b)).into_owned()") {
		t.Fatalf("not bridged:\n%s", got)
	}
}

// Rule 3b: a same-named method on a typed receiver drops the RECEIVER, never lifts the argument.
func TestASameNamedMethodDropsTheReceiver(t *testing.T) {
	text := "fn t() { let r = volume.root(); r.starts_with(&pathbuf); }\n"
	d := diag("E0308", "mismatched types",
		spanOf(t, text, "&pathbuf", "", "expected `&AppPath`, found `&PathBuf`", true),
		spanOf(t, text, "starts_with", "", "arguments to this method are incorrect", false))
	d.Children = []diagnostic{definedIn("crates/cmdr-fs/src/app_path/mod.rs")}
	got := run(t, text, d)
	if !strings.Contains(got, "r.as_std_legacy().starts_with(&pathbuf)") {
		t.Fatalf("receiver not dropped:\n%s", got)
	}
}

func TestAPathOnlyMethodOnATypedReceiver(t *testing.T) {
	text := "fn t() { log(volume.root().display()); }\n"
	got := run(t, text, diag("E0599", "no method named `display` found for reference `&AppPath` in the current scope",
		spanOf(t, text, "display", "", "method not found in `&AppPath`", true)))
	if !strings.Contains(got, "volume.root().as_std_legacy().display()") {
		t.Fatalf("receiver not dropped:\n%s", got)
	}
}

// Rule 3a: a borrowed return retypes the field to `BridgedPath`.
func TestABorrowedReturnRetypesTheField(t *testing.T) {
	text := "pub struct Demo {\n    name: String,\n    archive_path: PathBuf,\n    home: Option<PathBuf>,\n}\n" + implHead +
		"    fn root(&self) -> &AppPath {\n        &self.archive_path\n    }\n" +
		"    fn stands_in_for_own_disk(&self) -> Option<&AppPath> {\n        self.home.as_deref()\n    }\n}\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "&self.archive_path", "fn root", "expected `&AppPath`, found `&PathBuf`", true)))
	got = run(t, got, diag("E0308", "mismatched types", spanOf(t, got, "self.home.as_deref()", "", "expected `Option<&AppPath>`, found `Option<&Path>`", true)))
	for _, want := range []string{"archive_path: BridgedPath,", "home: Option<BridgedPath>,", "self.archive_path.app()", "self.home.as_ref().map(BridgedPath::app)"} {
		if !strings.Contains(got, want) {
			t.Fatalf("missing %q:\n%s", want, got)
		}
	}
}

func TestALiteralRootIsAStaticAppPath(t *testing.T) {
	text := implHead + "    fn root(&self) -> &AppPath {\n        Path::new(\"/\")\n    }\n}\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "Path::new(\"/\")", "", "expected `&AppPath`, found `&Path`", true)))
	if !strings.Contains(got, "AppPath::from_normalized(\"/\").expect(\"a canonical literal\")") {
		t.Fatalf("literal root not converted:\n%s", got)
	}
}

func TestBridgedPathConstructorsAndUses(t *testing.T) {
	text := "fn t() { let v = Demo { archive_path: path }; native_owned(v.archive_path.clone()); parent.space(&self.archive_path); }\n"
	got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, "path", "archive_path: ", "expected `BridgedPath`, found `PathBuf`", true)))
	got = run(t, got, diag("E0308", "mismatched types", spanOf(t, got, "v.archive_path.clone()", "", "expected `PathBuf`, found `BridgedPath`", true)))
	got = run(t, got, diag("E0308", "mismatched types", spanOf(t, got, "&self.archive_path", "", "expected `&AppPath`, found `&BridgedPath`", true)))
	for _, want := range []string{"archive_path: BridgedPath::new(path)", "v.archive_path.clone().into_path_buf()", "parent.space(self.archive_path.app())"} {
		if !strings.Contains(got, want) {
			t.Fatalf("missing %q:\n%s", want, got)
		}
	}
}

func TestImportsGoWhereRustcSuggests(t *testing.T) {
	text := "use std::sync::Arc;\nfn t() { AppPath::legacy_unchecked(p); }\n"
	replacement := "use crate::volume::AppPath;\n"
	d := diag("E0433", "failed to resolve: use of undeclared type `AppPath`", spanOf(t, text, "AppPath", "fn t", "", true))
	d.Children = []diagnostic{{Message: "consider importing this struct", Spans: []diagSpan{{FileName: fixtureFile, ByteStart: 0, ByteEnd: 0, SuggestedReplacement: &replacement}}}}
	got := run(t, text, d)
	if !strings.HasPrefix(got, "use cmdr_fs::app_path::AppPath;\nuse std::sync::Arc;") {
		t.Fatalf("import not added with the canonical path:\n%s", got)
	}
}

func TestUnusedPathImportsGo(t *testing.T) {
	text := "use std::path::{Path, PathBuf};\n"
	empty, applicable := "", "MachineApplicable"
	d := diagnostic{Message: "unused import: `PathBuf`", Code: &diagCode{Code: "unused_imports"}, Level: "warning",
		Spans:    []diagSpan{spanOf(t, text, "PathBuf", "", "", true)},
		Children: []diagnostic{{Message: "remove the unused import", Spans: []diagSpan{{FileName: fixtureFile, ByteStart: 20, ByteEnd: 29, SuggestedReplacement: &empty, SuggestionApplicability: &applicable}}}}}
	got := run(t, text, d)
	if got != "use std::path::{Path};\n" {
		t.Fatalf("unused import not removed: %q", got)
	}
}

func TestCanonicalLiterals(t *testing.T) {
	for text, want := range map[string]bool{
		"/": true, "/a/b": true, "/a b/c.txt": true, "sftp://u@h:22/x": true, "mtp://dev/store/DCIM": true,
		"/a/": false, "/a/../b": false, "/a/./b": false, "": false, ".": false, "photos": false, "//srv/share/x": false,
		"C:/x": false, "sftp://h": false, "sftp://h/": false, "c://x": false, "/a\\b": false,
	} {
		if got := isCanonicalLiteral(text); got != want {
			t.Errorf("isCanonicalLiteral(%q) = %v, want %v", text, got, want)
		}
	}
}

func TestMaskBlanksStringsAndCommentsButNotLifetimes(t *testing.T) {
	text := "fn f<'a>(x: &'a str) { let s = \"{ path }\"; // path {\n let c = '{'; }"
	masked := string(mask([]byte(text)))
	if strings.Contains(masked, "path") || strings.Count(masked, "{") != 1 || !strings.Contains(masked, "'a") {
		t.Fatalf("bad mask: %q", masked)
	}
}

func TestReceiverStartWalksTheChain(t *testing.T) {
	text := "let x = self.inner.root().display();"
	dot := strings.Index(text, ".display")
	if got := text[receiverStart([]byte(text), dot):dot]; got != "self.inner.root()" {
		t.Fatalf("receiver = %q", got)
	}
}

// The whole loop on a fixture "crate" isn't run here (it needs cargo); `chooseEdits` keeps
// the order and overlap rules every round relies on.
func TestChooseEditsDropsOverlapsAndKeepsTwoInsertsAtOnePoint(t *testing.T) {
	edits := chooseEdits([]edit{
		{file: "a", start: 10, end: 20, text: "x"},
		{file: "a", start: 12, end: 14, text: "y"},
		{file: "a", start: 30, end: 30, text: "b"},
		{file: "a", start: 30, end: 30, text: "a"},
		{file: "a", start: 10, end: 20, text: "x"},
	})
	if len(edits) != 3 || edits[1].text != "a" || edits[2].text != "b" {
		t.Fatalf("edits = %+v", edits)
	}
}

func TestParseDiagnosticsDedupesAndFilters(t *testing.T) {
	line := `{"reason":"compiler-message","message":{"message":"m","code":{"code":"E0308"},"level":"error","spans":[{"file_name":"crates/demo/src/lib.rs","byte_start":1,"byte_end":2,"line_start":1,"is_primary":true,"label":"l"}],"children":[]}}`
	other := strings.Replace(line, "crates/demo", "crates/other", 1)
	stream := []byte(line + "\n" + line + "\n" + other + "\n" + `{"reason":"build-finished","success":false}` + "\n")
	diags, err := parseDiagnostics(stream, func(f string) bool { return strings.HasPrefix(f, "crates/demo/") })
	if err != nil || len(diags) != 1 {
		t.Fatalf("diags = %d, err = %v", len(diags), err)
	}
}

func TestApplyEditsWritesBackToFront(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "f.rs"), []byte("abcdef"), 0o644); err != nil {
		t.Fatal(err)
	}
	err := applyEdits(dir, []edit{{file: "f.rs", start: 1, end: 2, text: "XX"}, {file: "f.rs", start: 4, end: 4, text: "_"}})
	if err != nil {
		t.Fatal(err)
	}
	got, _ := os.ReadFile(filepath.Join(dir, "f.rs"))
	if string(got) != "aXXcd_ef" {
		t.Fatalf("got %q", got)
	}
}
