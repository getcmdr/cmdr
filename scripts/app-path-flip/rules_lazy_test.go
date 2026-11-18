package main

import (
	"strings"
	"testing"
)

// The lazy-helper rule (a prelude'd body tail-calling a future that does nothing before its
// first poll), the round-trip collapse, and the import placement under attributes.

const prelude1 = "        let path_std = path.as_std_legacy();\n        let path: &Path = &path_std;\n"

func lazyDiag(t *testing.T, text, needle string) diagnostic {
	return diag("E0515", "cannot return value referencing local variable `path_std`", spanOf(t, text, needle, "", "", true))
}

func runWith(t *testing.T, r *rewriter, text string, d diagnostic) string {
	t.Helper()
	edits, err := r.editsFor(&d)
	if err != nil {
		t.Fatal(err)
	}
	return string(applyToText([]byte(text), chooseEdits(edits)))
}

func TestAnUnboxedTailCallToABodyOnlyBoxPinHelperMovesIntoTheFuture(t *testing.T) {
	text := implHead + "    fn scan_for_copy<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 + "        self.scan_for_copy_impl(path)\n    }\n}\n" +
		"impl Demo {\n    fn scan_for_copy_impl<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n        // A comment before the box is no work.\n        Box::pin(async move {\n            let p = self.to_smb_path(path)?;\n            Ok(p)\n        })\n    }\n}\n"
	got := run(t, text, lazyDiag(t, text, "self.scan_for_copy_impl(path)"))
	want := "{ Box::pin(async move { let path_std = path.as_std_legacy(); let path: &Path = &path_std; self.scan_for_copy_impl(path).await }) }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("not moved into the future:\n%s", got)
	}
}

func TestAHelperDoingWorkBeforeItsBoxStaysResidue(t *testing.T) {
	text := implHead + "    fn open_read_stream<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 + "        self.open_read_stream_impl(path)\n    }\n}\n" +
		"impl Demo {\n    fn open_read_stream_impl<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n        let abs_path = self.resolve(path);\n        Box::pin(async move { open(abs_path).await })\n    }\n}\n"
	if got := run(t, text, lazyDiag(t, text, "self.open_read_stream_impl(path)")); got != text {
		t.Fatalf("a helper with work before its future must stay residue:\n%s", got)
	}
}

func TestAHelperWithWorkAfterItsBoxStaysResidue(t *testing.T) {
	text := implHead + "    fn f<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 + "        self.f_impl(path)\n    }\n}\n" +
		"impl Demo {\n    fn f_impl<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n        Box::pin(async move { 1 }).also(self.count())\n    }\n}\n"
	if got := run(t, text, lazyDiag(t, text, "self.f_impl(path)")); got != text {
		t.Fatalf("a body that is more than the box must stay residue:\n%s", got)
	}
}

func TestTheBatchTwinMovesIntoTheFuture(t *testing.T) {
	text := implHead + "    fn scan_for_copy_batch<'a>(&'a self, paths: &'a [AppPathBuf], b: &'a ScanBoundary<'a>) -> Fut<'a> {\n" +
		"        let paths_std: Vec<PathBuf> = paths.iter().map(|p| p.as_std_legacy().into_owned()).collect();\n        let paths: &[PathBuf] = &paths_std;\n" +
		"        self.batch_impl(paths, b)\n    }\n}\nimpl Demo {\n    fn batch_impl<'a>(&'a self, paths: &'a [PathBuf], b: &'a ScanBoundary<'a>) -> Fut<'a> {\n        Box::pin(async move { todo!() })\n    }\n}\n"
	d := diag("E0515", "cannot return value referencing local variable `paths_std`", spanOf(t, text, "self.batch_impl(paths, b)", "", "", true))
	got := run(t, text, d)
	want := "{ Box::pin(async move { let paths_std: Vec<PathBuf> = paths.iter().map(|p| p.as_std_legacy().into_owned()).collect(); let paths: &[PathBuf] = &paths_std; self.batch_impl(paths, b).await }) }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("batch not moved:\n%s", got)
	}
}

const notingHelpers = "impl Demo {\n    pub(super) async fn noting<T>(&self, work: impl Future<Output = T> + Send) -> T {\n        let ran_on = self.generation();\n        work.await\n    }\n" +
	"    pub(super) async fn list_impl(&self, path: &Path, on_progress: Option<&P>, cancel: Option<&C>) -> R {\n        todo!()\n    }\n" +
	"    fn eager_impl<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n        let x = self.resolve(path);\n        Box::pin(async move { x })\n    }\n}\n"

func TestANestedCallTreeOfAsyncFnsMovesIntoTheFuture(t *testing.T) {
	text := implHead + "    fn list_directory<'a>(&'a self, path: &'a AppPath, on_progress: Option<&'a P>) -> Fut<'a> {\n" + prelude1 +
		"        Box::pin(self.noting(self.list_impl(path, on_progress, None)))\n    }\n}\n" + notingHelpers
	got := run(t, text, lazyDiag(t, text, "Box::pin(self.noting(self.list_impl(path, on_progress, None)))"))
	want := "{ Box::pin(async move { let path_std = path.as_std_legacy(); let path: &Path = &path_std; self.noting(self.list_impl(path, on_progress, None)).await }) }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("nested tree not moved:\n%s", got)
	}
}

func TestANestedTreeWithOneEagerCallStaysResidue(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 +
		"        Box::pin(self.noting(self.eager_impl(path)))\n    }\n}\n" + notingHelpers
	if got := run(t, text, lazyDiag(t, text, "Box::pin(self.noting(self.eager_impl(path)))")); got != text {
		t.Fatalf("an eager call anywhere in the tree must keep it residue:\n%s", got)
	}
}

func TestAnArgumentThatRunsCodeStaysResidue(t *testing.T) {
	for _, arg := range []string{"self.resolve(path)", "Path::new(x)", "path.await", "counter.next()?", "&items[0]", "|p| p", "vec![1]", "\"lit\""} {
		text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 +
			"        Box::pin(self.noting(self.list_impl(path, " + arg + ", None)))\n    }\n}\n" + notingHelpers
		call := "Box::pin(self.noting(self.list_impl(path, " + arg + ", None)))"
		if got := run(t, text, lazyDiag(t, text, call)); got != text {
			t.Fatalf("argument %q evaluates later once moved, so it must stay residue:\n%s", arg, got)
		}
	}
}

func TestConstructorsAndPathsArePlainArguments(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 +
		"        Box::pin(self.noting(self.list_impl(&path, Some(Opening::Fresh(self.mode)), MacOnlyFiles::Include)))\n    }\n}\n" + notingHelpers
	got := run(t, text, lazyDiag(t, text, "Box::pin(self.noting(self.list_impl(&path, Some(Opening::Fresh(self.mode)), MacOnlyFiles::Include)))"))
	if !strings.Contains(squash(got), "Box::pin(async move { let path_std") {
		t.Fatalf("constructor args not accepted:\n%s", got)
	}
}

func TestACalleeInAnotherCrateIsFoundInTheWorkspace(t *testing.T) {
	text := implHead + "    fn scan_for_copy<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 +
		"        Box::pin(self.noting(scan_walk::scan_one(self, path)))\n    }\n}\n" + notingHelpers
	const walk = "crates/cmdr-fs/src/volume/scan_walk.rs"
	files := map[string]string{fixtureFile: text, walk: "pub fn scan_one<'a>(source: &'a dyn ScanSource, path: &'a Path) -> Walking<'a, R> {\n    Box::pin(async move {\n        let boundary = ScanBoundary::silent();\n        scan_tree(source, path, &boundary).await\n    })\n}\n"}
	r := fixture(files)
	r.crateRust, r.workspaceRust = []string{fixtureFile}, []string{walk}
	got := runWith(t, r, text, lazyDiag(t, text, "Box::pin(self.noting(scan_walk::scan_one(self, path)))"))
	if !strings.Contains(squash(got), "self.noting(scan_walk::scan_one(self, path)).await }) }") {
		t.Fatalf("workspace callee not found lazy:\n%s", got)
	}
	r.workspaceRust, r.lazyMemo = nil, nil
	if got := runWith(t, r, text, lazyDiag(t, text, "Box::pin(self.noting(scan_walk::scan_one(self, path)))")); got != text {
		t.Fatalf("a callee defined nowhere in reach is not lazy:\n%s", got)
	}
}

func TestOneEagerDefinitionOfANameKeepsItResidue(t *testing.T) {
	text := implHead + "    fn patch<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 +
		"        Box::pin(self.noting(self.list_impl(path, None, None)))\n    }\n}\n" + notingHelpers +
		"impl Other {\n    fn list_impl<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n        self.count();\n        Box::pin(async move { 1 })\n    }\n}\n"
	if got := run(t, text, lazyDiag(t, text, "Box::pin(self.noting(self.list_impl(path, None, None)))")); got != text {
		t.Fatalf("an ambiguous name with an eager definition must stay residue:\n%s", got)
	}
}

func TestAFutureBoundToALetMovesIntoTheBlock(t *testing.T) {
	text := implHead + "    fn write_from_stream<'a>(&'a self, path: &'a AppPath, s: S) -> Fut<'a> {\n" + prelude1 +
		"        let write = self.list_impl(path, s, None);\n        Box::pin(async move { self.noting(write).await })\n    }\n}\n" + notingHelpers
	got := run(t, text, lazyDiag(t, text, "Box::pin(async move { self.noting(write).await })"))
	want := "{ Box::pin(async move { let path_std = path.as_std_legacy(); let path: &Path = &path_std; let write = self.list_impl(path, s, None); self.noting(write).await }) }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("let-bound future not moved:\n%s", got)
	}
	eager := strings.Replace(text, "let write = self.list_impl(path, s, None);", "let write = self.eager_impl(path);", 1)
	if got := run(t, eager, lazyDiag(t, eager, "Box::pin(async move { self.noting(write).await })")); got != eager {
		t.Fatalf("a let binding an eager call must stay residue:\n%s", got)
	}
}

func TestARoundTripCollapsesToTheTypedParam(t *testing.T) {
	text := "macro_rules! forward {\n    (@ get_metadata) => {\n        fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n            self.inner.get_metadata(&AppPath::legacy_unchecked(&path.as_std_legacy()))\n        }\n    };\n" +
		"    (@ scan) => {\n        fn scan<'a>(&'a self, paths: &'a [AppPathBuf]) -> Fut<'a> {\n            self.inner.scan(&paths.iter().map(|p| AppPath::legacy_unchecked(&p.as_std_legacy()).into_owned()).collect::<Vec<_>>())\n        }\n    };\n}\n"
	got := run(t, text, diag("E0515", "cannot return value referencing temporary value", spanOf(t, text, "self.inner.get_metadata(&AppPath::legacy_unchecked(&path.as_std_legacy()))", "", "", true)))
	if !strings.Contains(got, "self.inner.get_metadata(path)\n") {
		t.Fatalf("one round trip not collapsed:\n%s", got)
	}
	call := "self.inner.scan(&paths.iter().map(|p| AppPath::legacy_unchecked(&p.as_std_legacy()).into_owned()).collect::<Vec<_>>())"
	got = run(t, text, diag("E0515", "cannot return value referencing temporary value", spanOf(t, text, call, "", "", true)))
	if !strings.Contains(got, "self.inner.scan(paths)\n") {
		t.Fatalf("batch round trip not collapsed:\n%s", got)
	}
}

func TestARoundTripOfANonParamIsLeftAlone(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n        let owned = path.to_owned();\n        self.inner.get_metadata(&AppPath::legacy_unchecked(&owned.as_std_legacy()))\n    }\n}\n"
	if got := run(t, text, diag("E0515", "cannot return value referencing temporary value", spanOf(t, text, "self.inner.get_metadata(&AppPath::legacy_unchecked(&owned.as_std_legacy()))", "", "", true))); got != text {
		t.Fatalf("a value the text can't type must not collapse:\n%s", got)
	}
}

func TestAMacroBodysErrorsWaitForItsSignature(t *testing.T) {
	text := "macro_rules! forward {\n    (@ get_metadata) => {\n        fn get_metadata<'a>(&'a self, path: &'a Path) -> Fut<'a> {\n            self.inner.get_metadata(path)\n        }\n    };\n}\n"
	r := fixture(map[string]string{fixtureFile: text})
	diags := []diagnostic{
		diag("E0053", "m", spanOf(t, text, "&'a Path", "", "expected `AppPath`, found `Path`", true)),
		diag("E0308", "mismatched types", spanOf(t, text, "path", "get_metadata(", "expected `&AppPath`, found `&Path`", true)),
	}
	retyping, err := methodsBeingRetyped(r, diags)
	if err != nil || len(retyping) != 1 {
		t.Fatalf("retyping = %v, err = %v", retyping, err)
	}
	src, _ := r.load(fixtureFile)
	if m := src.fnAt(diags[1].primary().ByteStart); m == nil || !retyping[methodKey(fixtureFile, m)] {
		t.Fatalf("the body's stale error isn't skipped, so it gets bridged the same round as the retype")
	}
}

func TestAnImportNeverLandsUnderAnItemsAttribute(t *testing.T) {
	text := "mod streams;\n\n#[cfg(any(test, feature = \"testing\"))]\npub use streams::chunk_stream;\nuse std::sync::Arc;\n\nfn t() { BridgedPath::new(p); }\n"
	got := run(t, text, diag("E0433", "failed to resolve: use of undeclared type `BridgedPath`", spanOf(t, text, "BridgedPath", "fn t() { ", "", true)))
	if !strings.Contains(got, "\nuse cmdr_fs::app_path::BridgedPath;\n#[cfg(any(test, feature = \"testing\"))]\npub use streams::chunk_stream;") {
		t.Fatalf("import not placed above the attribute:\n%s", got)
	}
	multi := "/// Docs.\n#[cfg(any(\n    test,\n    feature = \"testing\"\n))]\npub use streams::chunk_stream;\n\nfn t() { BridgedPath::new(p); }\n"
	at := strings.Index(multi, "pub use")
	insert := "use x;\n"
	d := diag("E0433", "failed to resolve: use of undeclared type `BridgedPath`", spanOf(t, multi, "BridgedPath", "fn t() { ", "", true))
	d.Children = []diagnostic{{Message: "consider importing", Spans: []diagSpan{{FileName: fixtureFile, ByteStart: at, ByteEnd: at, SuggestedReplacement: &insert}}}}
	got = run(t, multi, d)
	if !strings.HasPrefix(got, "use cmdr_fs::app_path::BridgedPath;\n/// Docs.\n#[cfg(any(") {
		t.Fatalf("rustc's point under a multi-line attribute and its doc not lifted:\n%s", got)
	}
}

func TestATypedOperandTakesTheOtherSidesShape(t *testing.T) {
	for _, c := range []struct{ expr, typ, other, want string }{
		{"path", "&AppPathBuf", "&PathBuf", "&*path.as_std_legacy()"},
		{"path", "AppPathBuf", "PathBuf", "*path.as_std_legacy()"},
		{"path", "&AppPath", "&&Path", "&&*path.as_std_legacy()"},
		{"path.as_std_legacy()", "Cow<Path>", "&PathBuf", "&*path.as_std_legacy()"},
		{"root", "&BridgedPath", "&Path", "root.as_path()"},
		{"root", "BridgedPath", "PathBuf", "*root.as_path()"},
		{"found", "Option<AppPathBuf>", "Option<PathBuf>", "found.as_ref().map(|p| p.as_std_legacy().into_owned())"},
		{"paths", "&Vec<AppPathBuf>", "&Vec<PathBuf>", "&paths.iter().map(|p| p.as_std_legacy().into_owned()).collect::<Vec<_>>()"},
	} {
		if got, ok := stdOperand(c.expr, c.typ, c.other); !ok || got != c.want {
			t.Errorf("stdOperand(%q, %q, %q) = %q, %v; want %q", c.expr, c.typ, c.other, got, ok, c.want)
		}
	}
	if _, ok := stdOperand("x", "Option<&AppPath>", "Option<&Path>"); ok {
		t.Error("a borrowed option has no owned twin to compare: residue")
	}
}

func TestAnAssertAgainstABorrowedPathBufCompiles(t *testing.T) {
	text := "fn t() {\n    assert_eq!(path, &paths[i]);\n}\n"
	start, end := strings.Index(text, "assert_eq!"), strings.Index(text, ";\n}")
	d := diag("E0277", "can't compare `&app_path::AppPathBuf` with `&std::path::PathBuf`",
		diagSpan{FileName: fixtureFile, ByteStart: start, ByteEnd: end, IsPrimary: true, Macro: "assert_eq"})
	if got := run(t, text, d); !strings.Contains(got, "assert_eq!(&*path.as_std_legacy(), &paths[i]);") {
		t.Fatalf("not reshaped:\n%s", got)
	}
}

func TestABorrowedTypedOperandTakesItsDepthFromRustcsNote(t *testing.T) {
	text := "fn t() {\n    assert_eq!(volume.root(), Path::new(&params.app_root()));\n}\n"
	start, end := strings.Index(text, "assert_eq!"), strings.Index(text, ";\n}")
	d := diag("E0277", "can't compare `app_path::AppPath` with `std::path::Path`",
		diagSpan{FileName: fixtureFile, ByteStart: start, ByteEnd: end, IsPrimary: true, Macro: "assert_eq"})
	d.Children = []diagnostic{
		{Message: "the trait `PartialEq<std::path::Path>` is not implemented for `app_path::AppPath`"},
		{Message: "required for `&cmdr_fs::app_path::AppPath` to implement `PartialEq<&std::path::Path>`"},
	}
	if got := run(t, text, d); !strings.Contains(got, "assert_eq!(&*volume.root().as_std_legacy(), Path::new(&params.app_root()));") {
		t.Fatalf("depth not read from the note:\n%s", got)
	}
	// The line an earlier pass left (`Path == &Path`) mends the same way.
	broken := "fn t() {\n    assert_eq!(*volume.root().as_std_legacy(), Path::new(&params.app_root()));\n}\n"
	start, end = strings.Index(broken, "assert_eq!"), strings.Index(broken, ";\n}")
	d = diag("E0277", "can't compare `std::path::Path` with `&std::path::Path`",
		diagSpan{FileName: fixtureFile, ByteStart: start, ByteEnd: end, IsPrimary: true, Macro: "assert_eq"})
	if got := run(t, broken, d); !strings.Contains(got, "assert_eq!(&*volume.root().as_std_legacy(), Path::new(&params.app_root()));") {
		t.Fatalf("earlier pass's operand not mended:\n%s", got)
	}
}

func TestATestDirIntoATypedSlotCrossesTheBridge(t *testing.T) {
	for _, c := range []struct{ expr, found, want string }{
		{"dir", "&cmdr_fs::testing::TestDir", "volume.list_directory(&AppPath::legacy_unchecked(dir), None)"},
		{"&dir", "&TestDir", "volume.list_directory(&AppPath::legacy_unchecked(&dir), None)"},
		{"&fixture", "&&MetadataFixture", "volume.list_directory(&AppPath::legacy_unchecked(fixture), None)"},
	} {
		text := "fn t() {\n    volume.list_directory(" + c.expr + ", None);\n}\n"
		got := run(t, text, diag("E0308", "mismatched types", spanOf(t, text, c.expr, "list_directory(", "expected `&AppPath`, found `"+c.found+"`", true)))
		if !strings.Contains(got, c.want) {
			t.Errorf("%s: got\n%s", c.found, got)
		}
	}
}

func TestTheAuditFindsAnImportUnderAnAttribute(t *testing.T) {
	imports := map[string]bool{"use cmdr_fs::app_path::BridgedPath;": true}
	hits := importsUnderAttributes("#[cfg(test)]\nuse cmdr_fs::app_path::BridgedPath;\npub use a::b;\n#![allow(x)]\nuse cmdr_fs::app_path::BridgedPath;\n", imports)
	if len(hits) != 1 || hits[0].line != 2 {
		t.Fatalf("hits = %v", hits)
	}
}
