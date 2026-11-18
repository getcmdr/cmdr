package main

import (
	"strings"
	"testing"
)

func TestASplitRoundTripDropsAnUnusedPrelude(t *testing.T) {
	text := "impl ScanSource for Demo {\n    fn scan_stat<'a>(&'a self, path: &'a AppPath) -> Walking<'a, FileEntry> {\n" + prelude1 +
		"        self.get_metadata(&AppPath::legacy_unchecked(path))\n    }\n}\n"
	got := run(t, text, diag("E0515", "cannot return value referencing local variable `path_std`", spanOf(t, text, "self.get_metadata(&AppPath::legacy_unchecked(path))", "", "", true)))
	want := "Walking<'a, FileEntry> { self.get_metadata(path) } }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("not collapsed:\n%s", got)
	}
}

func TestASplitRoundTripKeepsAPreludeOthersRead(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n" + prelude1 +
		"        log(path.display());\n        self.inner.get_metadata(&AppPath::legacy_unchecked(path))\n    }\n}\n"
	got := run(t, text, diag("E0515", "cannot return value referencing temporary value", spanOf(t, text, "self.inner.get_metadata(&AppPath::legacy_unchecked(path))", "", "", true)))
	want := "{ let path_app = path; let path_std = path.as_std_legacy(); let path: &Path = &path_std; log(path.display()); self.inner.get_metadata(path_app) }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("not handed the kept param:\n%s", got)
	}
}

func TestASplitRoundTripOfABatch(t *testing.T) {
	text := implHead + "    fn scan<'a>(&'a self, paths: &'a [AppPathBuf]) -> Fut<'a> {\n" +
		"        let paths_std: Vec<PathBuf> = paths.iter().map(|p| p.as_std_legacy().into_owned()).collect();\n        let paths: &[PathBuf] = &paths_std;\n" +
		"        self.inner.scan(&paths.iter().map(|p| AppPath::legacy_unchecked(p).into_owned()).collect::<Vec<_>>())\n    }\n}\n"
	call := "self.inner.scan(&paths.iter().map(|p| AppPath::legacy_unchecked(p).into_owned()).collect::<Vec<_>>())"
	got := run(t, text, diag("E0515", "cannot return value referencing temporary value", spanOf(t, text, call, "", "", true)))
	if !strings.Contains(squash(got), "Fut<'a> { self.inner.scan(paths) }") {
		t.Fatalf("batch not collapsed:\n%s", got)
	}
}

func TestABridgeOfANonParamOrBeforeThePreludeIsNoSplitRoundTrip(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath, other: &'a Path) -> Fut<'a> {\n" + prelude1 +
		"        self.inner.get_metadata(&AppPath::legacy_unchecked(other))\n    }\n}\n"
	if got := run(t, text, diag("E0515", "cannot return value referencing temporary value", spanOf(t, text, "self.inner.get_metadata(&AppPath::legacy_unchecked(other))", "", "", true))); got != text {
		t.Fatalf("a std param's bridge is no round trip:\n%s", got)
	}
}

func TestABridgeTemporaryAHeldFutureOutlivesIsHoisted(t *testing.T) {
	text := "async fn t() {\n    let dir = make();\n    let fut = volume.list_directory(&AppPath::legacy_unchecked(&dir), None);\n    fut.await;\n}\n"
	got := run(t, text, diag("E0716", "temporary value dropped while borrowed", spanOf(t, text, "AppPath::legacy_unchecked(&dir)", "", "creates a temporary value which is freed while still in use", true)))
	want := "let dir = make(); let dir_app = AppPath::legacy_unchecked(&dir); let fut = volume.list_directory(&dir_app, None); fut.await;"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("not hoisted:\n%s", got)
	}
	borrowSpan := run(t, text, diag("E0716", "temporary value dropped while borrowed", spanOf(t, text, "&AppPath::legacy_unchecked(&dir)", "", "", true)))
	if !strings.Contains(squash(borrowSpan), want) {
		t.Fatalf("a span over the borrow not hoisted:\n%s", borrowSpan)
	}
	join := "async fn t() {\n    let (a, b) = tokio::join!(\n        v.get_metadata(&AppPath::legacy_unchecked(&ctx.one)),\n        v.exists(x),\n    );\n}\n"
	got = run(t, join, diag("E0716", "temporary value dropped while borrowed", spanOf(t, join, "AppPath::legacy_unchecked(&ctx.one)", "", "", true)))
	if !strings.Contains(squash(got), "{ let one_app = AppPath::legacy_unchecked(&ctx.one); let (a, b) = tokio::join!( v.get_metadata(&one_app),") {
		t.Fatalf("join arm not hoisted before its statement:\n%s", got)
	}
}

func TestABridgeTemporaryInASelectArmIsHoistedBeforeTheSelect(t *testing.T) {
	text := "async fn t() {\n    let opened = tokio::select! {\n        biased;\n        () = abort.cancelled() => {\n            return Err(gone(path));\n        }\n        opened = v.open(&AppPath::legacy_unchecked(path), size) => opened,\n    };\n}\n"
	got := run(t, text, diag("E0716", "temporary value dropped while borrowed", spanOf(t, text, "AppPath::legacy_unchecked(path)", "", "", true)))
	want := "async fn t() { let path_app = AppPath::legacy_unchecked(path); let opened = tokio::select! { biased; () = abort.cancelled() => { return Err(gone(path)); } opened = v.open(&path_app, size) => opened, }; }"
	if !strings.Contains(squash(got), want) {
		t.Fatalf("not hoisted before the select:\n%s", got)
	}
	// Inside an arm's own block, the statement is the arm's.
	arm := "async fn t() {\n    tokio::select! {\n        () = a() => {\n            let d = make();\n            let fut = v.list(&AppPath::legacy_unchecked(&d));\n            fut.await;\n        }\n    };\n}\n"
	got = run(t, arm, diag("E0716", "temporary value dropped while borrowed", spanOf(t, arm, "AppPath::legacy_unchecked(&d)", "", "", true)))
	if !strings.Contains(squash(got), "let d = make(); let d_app = AppPath::legacy_unchecked(&d); let fut = v.list(&d_app);") {
		t.Fatalf("arm-local hoist left its block:\n%s", got)
	}
}

func TestAHoistThatWouldReorderOrShadowStaysResidue(t *testing.T) {
	for _, text := range []string{
		"async fn t() {\n    let fut = volume.list_directory(&AppPath::legacy_unchecked(&next_dir()), None);\n    fut.await;\n}\n",
		"async fn t() {\n    let dir_app = 1;\n    let fut = volume.list_directory(&AppPath::legacy_unchecked(&dir), None);\n    fut.await;\n}\n",
	} {
		at := strings.Index(text, "AppPath::legacy_unchecked(")
		end := strings.Index(text, "), None)") + 1
		d := diag("E0716", "temporary value dropped while borrowed", diagSpan{FileName: fixtureFile, ByteStart: at, ByteEnd: end, IsPrimary: true})
		if got := run(t, text, d); got != text {
			t.Fatalf("must stay residue:\n%s", got)
		}
	}
}

func TestStatementStartSkipsClosedBlocksAndCrossesParens(t *testing.T) {
	text := "{ let a = if x { 1 } else { 2 }; let f = g(h(&TARGET)); }"
	at := strings.Index(text, "TARGET")
	if got := text[statementStart([]byte(text), at, 0):at]; got != " let f = g(h(&" {
		t.Fatalf("statement starts at %q", got)
	}
	text = "{ if x {\n    run();\n}\nlet f = g(&TARGET); }"
	at = strings.Index(text, "TARGET")
	if got := text[statementStart([]byte(text), at, 0):at]; got != "\nlet f = g(&" {
		t.Fatalf("after a block statement: %q", got)
	}
}
