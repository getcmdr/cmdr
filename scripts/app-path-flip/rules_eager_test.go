package main

import (
	"strings"
	"testing"
)

// The eager-helper rule (a prelude'd body tail-calling a helper that does work before its
// future starts) and the `from_ref` row. The fixtures are `cmdr-local`'s helpers as they
// stood before its hand-made residue commit (ee6965cf20), and each positive test checks the
// rule makes what that commit made by hand.

const fut = "Pin<Box<dyn Future<Output = R> + Send + 'a>>"

func volumeMethod(sig, call string) string {
	return implHead + "    fn " + sig + " -> " + fut + " {\n" + prelude1 + "        " + call + "\n    }\n}\n"
}

func eagerDiag(t *testing.T, text, call string) diagnostic {
	return lazyDiag(t, text, call)
}

const openReadStreamHelper = "impl Demo {\n    pub(super) fn open_read_stream_impl<'a>(\n        &'a self,\n        path: &'a Path,\n    ) -> " + fut + " {\n" +
	"        let abs_path = self.resolve(path);\n        Box::pin(async move {\n            spawn_blocking(move || open(&abs_path)).await\n        })\n    }\n}\n"

func TestAnEagerHelperTakesTheTypedPathAndThePrelude(t *testing.T) {
	text := volumeMethod("open_read_stream<'a>(&'a self, path: &'a AppPath)", "self.open_read_stream_impl(path)") + openReadStreamHelper
	got := squash(run(t, text, eagerDiag(t, text, "self.open_read_stream_impl(path)")))
	for _, want := range []string{
		"fn open_read_stream<'a>(&'a self, path: &'a AppPath) -> " + fut + " { self.open_read_stream_impl(path) }",
		"path: &'a AppPath, ) -> " + fut + " { let path_std = path.as_std_legacy(); let path: &Path = &path_std; let abs_path = self.resolve(path);",
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("missing %q in:\n%s", want, got)
		}
	}
}

// `scan_subtree`'s shape: closures around a `.path()` call are no rebinding of `path`.
func TestMethodCallsBetweenClosuresAreNoRebinding(t *testing.T) {
	helper := "impl Demo {\n    fn open_read_stream_impl<'a>(&'a self, path: &'a Path) -> " + fut + " {\n        let abs = self.resolve(path);\n" +
		"        Box::pin(async move { walk(abs).map(|e| e.path().to_owned()).filter(|e| keep(e)).await })\n    }\n}\n"
	text := volumeMethod("open_read_stream<'a>(&'a self, path: &'a AppPath)", "self.open_read_stream_impl(path)") + helper
	if got := squash(run(t, text, eagerDiag(t, text, "self.open_read_stream_impl(path)"))); !strings.Contains(got, "{ let path_std = path.as_std_legacy(); let path: &Path = &path_std; let abs") {
		t.Fatalf("not moved:\n%s", got)
	}
}

func TestAnEagerHelperWhoseFutureBridgesTheParamBackKeepsTheTypedValue(t *testing.T) {
	helper := "impl Demo {\n    pub(super) fn list_directory_as_source_impl<'a>(\n        &'a self,\n        path: &'a Path,\n    ) -> " + fut + " {\n" +
		"        let abs_dir = self.resolve(path);\n        let sidecars = SidecarRule::with_probe(self.sidecar_probe);\n        Box::pin(async move {\n" +
		"            let entries = self.list_directory(&AppPath::legacy_unchecked(path), None).await?;\n            Ok(merge(entries, abs_dir, sidecars))\n        })\n    }\n}\n"
	text := volumeMethod("list_directory_as_source<'a>(&'a self, path: &'a AppPath)", "self.list_directory_as_source_impl(path)") + helper
	got := squash(run(t, text, eagerDiag(t, text, "self.list_directory_as_source_impl(path)")))
	want := "{ let path_app = path; let path_std = path.as_std_legacy(); let path: &Path = &path_std; let abs_dir = self.resolve(path);"
	if !strings.Contains(got, want) || !strings.Contains(got, "self.list_directory(path_app, None).await?") {
		t.Fatalf("typed value not kept:\n%s", got)
	}
}

func TestAForwardingChainRetypesEachLinkAndPreludesTheEagerEnd(t *testing.T) {
	helpers := "impl Demo {\n    pub(super) fn write_from_stream_impl<'a>(\n        &'a self,\n        dest: &'a Path,\n        mode: WriteMode,\n        mut stream: S,\n    ) -> " + fut + " {\n" +
		"        self.stream_into(dest, Opening::Fresh(mode), stream)\n    }\n\n" +
		"    fn stream_into<'a>(\n        &'a self,\n        dest: &'a Path,\n        opening: Opening,\n        mut stream: S,\n    ) -> " + fut + " {\n" +
		"        let dest_abs = self.resolve(dest);\n        Box::pin(async move {\n            write(dest_abs, opening, stream).await\n        })\n    }\n}\n"
	caller := implHead + "    fn write_from_stream<'a>(&'a self, dest: &'a AppPath, mode: WriteMode, stream: S) -> " + fut + " {\n" +
		"        let dest_std = dest.as_std_legacy();\n        let dest: &Path = &dest_std;\n        self.write_from_stream_impl(dest, mode, stream)\n    }\n}\n"
	text := caller + helpers
	d := diag("E0515", "cannot return value referencing local variable `dest_std`", spanOf(t, text, "self.write_from_stream_impl(dest, mode, stream)", "", "", true))
	got := squash(run(t, text, d))
	for _, want := range []string{
		"{ self.write_from_stream_impl(dest, mode, stream) }",
		"fn write_from_stream_impl<'a>( &'a self, dest: &'a AppPath, mode: WriteMode, mut stream: S, ) -> " + fut + " { self.stream_into(dest, Opening::Fresh(mode), stream) }",
		"fn stream_into<'a>( &'a self, dest: &'a AppPath, opening: Opening, mut stream: S, ) -> " + fut + " { let dest_std = dest.as_std_legacy(); let dest: &Path = &dest_std; let dest_abs = self.resolve(dest);",
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("missing %q in:\n%s", want, got)
		}
	}
}

func TestTheTypedParamCanSitAnywhereInTheArguments(t *testing.T) {
	helper := "impl Demo {\n    pub(super) fn scan_for_conflicts_impl<'a>(\n        &'a self,\n        source_items: &'a [SourceItemInfo],\n        dest_path: &'a Path,\n    ) -> " + fut + " {\n" +
		"        let dest_abs = self.resolve(dest_path);\n        let source_items: Vec<SourceItemInfo> = source_items.to_vec();\n        Box::pin(async move { conflicts(source_items, dest_abs).await })\n    }\n}\n"
	caller := implHead + "    fn scan_for_conflicts<'a>(&'a self, source_items: &'a [SourceItemInfo], dest_path: &'a AppPath) -> " + fut + " {\n" +
		"        let dest_path_std = dest_path.as_std_legacy();\n        let dest_path: &Path = &dest_path_std;\n        self.scan_for_conflicts_impl(source_items, dest_path)\n    }\n}\n"
	text := caller + helper
	d := diag("E0515", "cannot return value referencing local variable `dest_path_std`", spanOf(t, text, "self.scan_for_conflicts_impl(source_items, dest_path)", "", "", true))
	got := squash(run(t, text, d))
	if !strings.Contains(got, "dest_path: &'a AppPath, ) -> "+fut+" { let dest_path_std = dest_path.as_std_legacy(); let dest_path: &Path = &dest_path_std; let dest_abs") {
		t.Fatalf("second param not moved:\n%s", got)
	}
}

func TestTheBatchTwinGoesToAnEagerHelper(t *testing.T) {
	helper := "impl Demo {\n    fn batch_impl<'a>(&'a self, paths: &'a [PathBuf], b: &'a B) -> " + fut + " {\n        let owned = paths.to_vec();\n        Box::pin(async move { scan(owned, b).await })\n    }\n}\n"
	caller := implHead + "    fn scan_for_copy_batch<'a>(&'a self, paths: &'a [AppPathBuf], b: &'a B) -> " + fut + " {\n" +
		"        let paths_std: Vec<PathBuf> = paths.iter().map(|p| p.as_std_legacy().into_owned()).collect();\n        let paths: &[PathBuf] = &paths_std;\n        self.batch_impl(paths, b)\n    }\n}\n"
	text := caller + helper
	d := diag("E0515", "cannot return value referencing local variable `paths_std`", spanOf(t, text, "self.batch_impl(paths, b)", "", "", true))
	got := squash(run(t, text, d))
	if !strings.Contains(got, "fn batch_impl<'a>(&'a self, paths: &'a [AppPathBuf], b: &'a B) -> "+fut+" { let paths_std: Vec<PathBuf> = paths.iter().map(|p| p.as_std_legacy().into_owned()).collect(); let paths: &[PathBuf] = &paths_std; let owned") ||
		!strings.Contains(got, "{ self.batch_impl(paths, b) }") {
		t.Fatalf("batch not moved:\n%s", got)
	}
}

func TestAnEagerHelperInAnotherFileOfTheCrateIsEdited(t *testing.T) {
	const other = "crates/demo/src/streams.rs"
	text := volumeMethod("open_read_stream<'a>(&'a self, path: &'a AppPath)", "self.open_read_stream_impl(path)")
	files := map[string]string{fixtureFile: text, other: openReadStreamHelper}
	r := fixture(files)
	edits, err := r.editsFor(ptr(eagerDiag(t, text, "self.open_read_stream_impl(path)")))
	if err != nil {
		t.Fatal(err)
	}
	byFile := map[string][]edit{}
	for _, e := range chooseEdits(edits) {
		byFile[e.file] = append(byFile[e.file], e)
	}
	if got := squash(string(applyToText([]byte(files[other]), byFile[other]))); !strings.Contains(got, "path: &'a AppPath, ) -> "+fut+" { let path_std = path.as_std_legacy();") {
		t.Fatalf("helper file not edited:\n%s", got)
	}
	if got := squash(string(applyToText([]byte(text), byFile[fixtureFile]))); !strings.Contains(got, "{ self.open_read_stream_impl(path) }") {
		t.Fatalf("caller not edited:\n%s", got)
	}
}

func ptr(d diagnostic) *diagnostic { return &d }

func TestARerunOverTheEagerHelpersOutputIsANoOp(t *testing.T) {
	text := volumeMethod("open_read_stream<'a>(&'a self, path: &'a AppPath)", "self.open_read_stream_impl(path)") + openReadStreamHelper
	once := run(t, text, eagerDiag(t, text, "self.open_read_stream_impl(path)"))
	// The flipped tree compiles, so rustc raises no E0515 to re-run on; even handed one, the
	// rule finds no prelude in the caller and the helper's param already typed.
	r := fixture(map[string]string{fixtureFile: once})
	src, _ := r.load(fixtureFile)
	d := eagerDiag(t, once, "self.open_read_stream_impl(path)")
	if e := r.eagerHelperEdits(&d, src, d.primary()); len(e) > 0 {
		t.Fatalf("the rule fired on its own output: %v", e)
	}
	if _, ok := r.eagerChain("open_read_stream_impl", 0, "one", 0, map[string]bool{}); ok {
		t.Fatal("a helper already typed and preluded must not qualify again")
	}
}

// Each case keeps the E0515 residue: the move might not be the identity, or the text can't
// show that it is.
func TestTheEagerHelperRuleMustNotFire(t *testing.T) {
	plainCaller := volumeMethod("open_read_stream<'a>(&'a self, path: &'a AppPath)", "self.open_read_stream_impl(path)")
	helperWith := func(params, body string) string {
		return "impl Demo {\n    fn open_read_stream_impl<'a>(&'a self, " + params + ") -> " + fut + " {\n" + body + "\n    }\n}\n"
	}
	for _, c := range []struct{ name, text, call string }{
		{"the future borrows the param", plainCaller + helperWith("path: &'a Path", "        let abs = self.resolve(path);\n        Box::pin(async move { read(abs, path).await })"), ""},
		{"a second definition", plainCaller + openReadStreamHelper + strings.Replace(openReadStreamHelper, "impl Demo", "impl Other", 1), ""},
		{"a trait declares the name", plainCaller + openReadStreamHelper + "trait T {\n    fn open_read_stream_impl<'a>(&'a self, path: &'a Path) -> " + fut + ";\n}\n", ""},
		{"a statement beside the prelude", implHead + "    fn open_read_stream<'a>(&'a self, path: &'a AppPath) -> " + fut + " {\n" + prelude1 + "        self.count();\n        self.open_read_stream_impl(path)\n    }\n}\n" + openReadStreamHelper, ""},
		{"the name in another argument", volumeMethod("open_read_stream<'a>(&'a self, path: &'a AppPath)", "self.open_read_stream_impl(path, Some(path))") + helperWith("path: &'a Path, again: Option<&'a Path>", "        let abs = self.resolve(path);\n        Box::pin(async move { read(abs).await })"), "self.open_read_stream_impl(path, Some(path))"},
		{"a helper that isn't a self method", volumeMethod("open_read_stream<'a>(&'a self, path: &'a AppPath)", "open_read_stream_impl(path)") + "fn open_read_stream_impl<'a>(path: &'a Path) -> " + fut + " {\n    let abs = resolve(path);\n    Box::pin(async move { read(abs).await })\n}\n", "open_read_stream_impl(path)"},
		{"a param type the prelude doesn't make", plainCaller + helperWith("path: &'a PathBuf", "        let abs = self.resolve(path);\n        Box::pin(async move { read(abs).await })"), ""},
		{"a final expression that isn't the box", plainCaller + helperWith("path: &'a Path", "        let abs = self.resolve(path);\n        read(abs).boxed()"), ""},
		{"a closure param of the same name", plainCaller + helperWith("path: &'a Path", "        let abs = self.resolve(path);\n        let f = |path: &Path| path.exists();\n        Box::pin(async move { read(abs, f).await })"), ""},
		{"a body that rebinds the param", plainCaller + helperWith("path: &'a Path", "        let path = self.resolve(path);\n        Box::pin(async move { read(path).await })"), ""},
		{"an async helper (the lazy rule's)", plainCaller + "impl Demo {\n    async fn open_read_stream_impl(&self, path: &Path) -> R {\n        read(path).await\n    }\n}\n", ""},
		{"a forwarder passing the param twice", plainCaller + helperWith("path: &'a Path", "        self.inner_impl(path, path)") + "impl Demo {\n    fn inner_impl<'a>(&'a self, a: &'a Path, b: &'a Path) -> " + fut + " {\n        let x = self.resolve(a);\n        Box::pin(async move { read(x).await })\n    }\n}\n", ""},
		{"a forwarding cycle", plainCaller + helperWith("path: &'a Path", "        self.open_read_stream_impl(path)"), ""},
	} {
		call := c.call
		if call == "" {
			call = "self.open_read_stream_impl(path)"
		}
		got := run(t, c.text, eagerDiag(t, c.text, call))
		if c.name == "an async helper (the lazy rule's)" {
			if strings.Contains(got, "path: &AppPath) -> R") {
				t.Errorf("%s: retyped an async helper:\n%s", c.name, got)
			}
			continue
		}
		if got != c.text {
			t.Errorf("%s: must stay residue, got:\n%s", c.name, got)
		}
	}
}

// ---- from_ref ----

func fromRefDiag(t *testing.T, text, arg, found, definedAt string) diagnostic {
	d := diag("E0308", "mismatched types", spanOf(t, text, arg, "from_ref(", "expected `&AppPathBuf`, found `"+found+"`", true))
	name := "from_ref"
	if strings.Contains(text, "std::slice::from_ref") {
		name = "std::slice::from_ref"
	}
	callee := spanOf(t, text, name, "", "arguments to this function are incorrect", false)
	d.Spans = append(d.Spans, callee)
	d.Children = []diagnostic{{Message: "function defined here", Spans: []diagSpan{{FileName: definedAt}}}}
	return d
}

const coreSlice = "/rustc/abc/library/core/src/slice/raw.rs"

func TestAFromRefOfAPathBufIntoATypedBatchBridgesTheElement(t *testing.T) {
	text := "fn t() {\n    volume.scan_for_copy_batch(std::slice::from_ref(&path), &boundary);\n}\n"
	got := run(t, text, fromRefDiag(t, text, "&path", "&PathBuf", coreSlice))
	if !strings.Contains(got, "volume.scan_for_copy_batch(std::slice::from_ref(&AppPath::legacy_unchecked(&path).into_owned()), &boundary);") {
		t.Fatalf("element not bridged:\n%s", got)
	}
	borrowed := "fn t() {\n    volume.scan_for_copy_batch(from_ref(&p), &boundary);\n}\n"
	got = run(t, borrowed, fromRefDiag(t, borrowed, "&p", "&&Path", coreSlice))
	if !strings.Contains(got, "from_ref(&AppPath::legacy_unchecked(p).into_owned())") {
		t.Fatalf("an extra borrow not dropped:\n%s", got)
	}
}

func TestTheFromRefRowMustNotFire(t *testing.T) {
	text := "fn t() {\n    volume.scan_for_copy_batch(std::slice::from_ref(&path), &boundary);\n}\n"
	if got := run(t, text, fromRefDiag(t, text, "&path", "&PathBuf", "crates/demo/src/lib.rs")); got != text {
		t.Fatalf("a crate's own from_ref is not the slice one:\n%s", got)
	}
	d := fromRefDiag(t, text, "&path", "&PathBuf", coreSlice)
	d.Spans[0].Label = ptrString("expected `&AppPathBuf`, found `&String`")
	if got := run(t, text, d); got != text {
		t.Fatalf("a non-path element must stay residue:\n%s", got)
	}
	// The re-run: a bridged element raises nothing, and a typed element is no mismatch.
	done := "fn t() {\n    volume.scan_for_copy_batch(std::slice::from_ref(&AppPath::legacy_unchecked(&path).into_owned()), &boundary);\n}\n"
	if got := run(t, done, fromRefDiag(t, done, "&AppPath::legacy_unchecked(&path).into_owned()", "&AppPathBuf", coreSlice)); got != done {
		t.Fatalf("an already-typed element changed:\n%s", got)
	}
}

func ptrString(s string) *string { return &s }
