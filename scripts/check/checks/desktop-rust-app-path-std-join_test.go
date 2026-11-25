package checks

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestWrapperCallCount(t *testing.T) {
	for _, tc := range []struct {
		name, line string
		want       int
	}{
		{"bare call", "let p = std_of(&path);", 1},
		{"each wrapper", "f(on_volume(a), seam_path(b), engine_path(c), into_std(d), std_of(e));", 5},
		{"module-qualified call", "let p = listed_name::on_volume(&path);", 1},
		{"crate-qualified call", "let p = crate::wire_paths::seam_path(&path);", 1},
		{"method call is unrelated", "let s = images.on_volume(path, id, closed);", 0},
		{"wrapped method call", "    .on_volume(path, id, closed)", 0},
		{"definition", "pub(crate) fn std_of(path: &AppPath) -> Cow<'_, Path> {", 0},
		{"type's associated fn", "ListingPath::on_volume(self.volumes.as_ref(), id, path)", 0},
		{"Self's associated fn", "Self::on_volume(volume)", 0},
		{"longer name", "fn on_volume_arrival(&self) { engine_path_of(p); engine_paths(ps); }", 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			for _, relPath := range []string{"crates/cmdr-operations/src/x.rs", "crates/cmdr-fs/src/volume/x.rs"} {
				if got := wrapperCallCount(tc.line, relPath); got != tc.want {
					t.Errorf("wrapperCallCount(%q) in %s = %d, want %d", tc.line, relPath, got, tc.want)
				}
			}
			// The wrappers are crate-private: outside their crates a same-named call is another fn.
			if got := wrapperCallCount(tc.line, "apps/desktop/src-tauri/src/commands/x.rs"); got != 0 {
				t.Errorf("wrapperCallCount(%q) outside the wrappers' crates = %d, want 0", tc.line, got)
			}
		})
	}
}

func TestScanStdJoins(t *testing.T) {
	for _, tc := range []struct {
		name, body string
		want       int
	}{
		{"untyped join", "let p = Path::new(\"/share\").join(name);", 1},
		{"two on a line", "let (a, b) = (dir.join(x), dir.join(y));", 2},
		{"with_file_name", "let p = path.with_file_name(new_name);", 1},
		{"typed ?", "let p = folder.join(name)?;", 0},
		{"typed .ok()", "let p = folder.join(&name).ok();", 0},
		{"typed .map_err", "let p = folder.join(name).map_err(Error::from)?;", 0},
		{"typed .expect", "let p = folder.join(name).expect(\"valid\");", 0},
		{"typed unwrap_or", "let p = folder.join(name).unwrap_or_default();", 0},
		{"typed .is_ok", "assert!(folder.join(name).is_ok());", 0},
		{"typed, wrapped by rustfmt", "let p = folder\n    .join(&format!(\"{}.tmp\", name))\n    .map_err(Error::from)?;", 0},
		{"untyped, wrapped arg", "let p = dir.join(\n    name,\n);", 1},
		{"untyped, then another call", "let p = dir.join(name).canonicalize()?;", 1},
		{"string join of a slice", "let s = parts.join(\"/\"); let t = names.join(\", \");", 0},
		{"a file name literal counts", "let db = dir.join(\"index.sqlite\");", 1},
		{"empty arg", "let s = parts.join();", 0},
		{"ListedFolder line", "let p = ListedFolder::of(&dir).join(name);", 0},
		{"comment line", "// dir.join(name) would split on Windows", 0},
		{"trailing comment", "let x = 1; // dir.join(name)", 0},
		{"directive on the line", "let p = tmp.join(name); // allowed-std-join: a local tempdir", 0},
		{"directive above", "// allowed-std-join: the in-flight ledger is local\nlet p = ledger_dir.join(name);", 0},
		{"after cfg(all(test, ...))", "fn f() {}\n#[cfg(all(test, unix))]\nmod tests { fn t() { let p = dir.join(name); } }", 0},
		{"cfg(any(test, ...)) is production", "#[cfg(any(test, feature = \"x\"))]\nfn f() { let p = dir.join(name); }", 1},
		{"after cfg(test)", "fn f() {}\n#[cfg(test)]\nmod tests { fn t() { let p = dir.join(name); } }", 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			sites, orphans := scanStdJoins("crates/cmdr-operations/src/x.rs", strings.Split(tc.body, "\n"))
			if len(sites) != tc.want {
				t.Errorf("got %d sites (%v), want %d", len(sites), sites, tc.want)
			}
			if len(orphans) != 0 {
				t.Errorf("unexpected orphan directives: %v", orphans)
			}
		})
	}
}

func TestScanStdJoins_AnUnusedDirectiveIsStale(t *testing.T) {
	_, orphans := scanStdJoins("crates/cmdr-operations/src/x.rs",
		[]string{"// allowed-std-join: nothing to excuse", "let p = folder.join(name)?;"})
	if len(orphans) != 1 || orphans[0].line != 1 {
		t.Fatalf("expected the directive reported stale, got %v", orphans)
	}
}

func TestAppPathStdBridge_StdJoinIsScopedToTheWriteEngineAndBudgeted(t *testing.T) {
	body := "fn f(dir: &Path, name: &str) -> PathBuf { dir.join(name) }\n"
	_, _, err := runAppPathStdBridgeOn(t, map[string]string{"operations_host/x.rs": body}, nil)
	if err == nil || !strings.Contains(err.Error(), "wire_paths::seam_child") {
		t.Fatalf("an unbudgeted join under operations_host must fail naming the doors, got %v", err)
	}
	if _, _, err := runAppPathStdBridgeOn(t, map[string]string{"volume/x.rs": body}, nil); err != nil {
		t.Fatalf("a join outside the write engine isn't budgeted, got %v", err)
	}
	if _, _, err := runAppPathStdBridgeOn(t, map[string]string{"operations_host/x_tests.rs": body}, nil); err != nil {
		t.Fatalf("a test file's join isn't budgeted, got %v", err)
	}
	unused := "// allowed-std-join: nothing here\nfn f() {}\n"
	_, _, err = runAppPathStdBridgeOn(t, map[string]string{"operations_host/x.rs": unused}, nil)
	if err == nil || !strings.Contains(err.Error(), "unused `// allowed-std-join:`") {
		t.Fatalf("an unused directive must fail, got %v", err)
	}
}

// The wrappers are crate-private to cmdr-operations and cmdr-fs, so a same-named call in the
// app tree (the desktop's own `wire_path::on_volume`) isn't one; the fixture workspace scans
// only the app tree, so the in-scope count is covered by TestWrapperCallCount.
func TestAppPathStdBridge_NewBudgetsRatchetAndReportTheTotals(t *testing.T) {
	files := map[string]string{
		"operations_host/x.rs": "fn f(dir: &Path, name: &str) { let p = dir.join(name); let q = on_volume(&a); }\n",
	}
	x := "apps/desktop/src-tauri/src/operations_host/x.rs"
	list := &appPathStdBridgeAllowlist{WrapperCalls: map[string]int{x: 3}, StdJoin: map[string]int{x: 4}}
	res, written, err := runAppPathStdBridgeWith(t, files, list)
	if err != nil {
		t.Fatalf("budgeted sites must pass, got %v", err)
	}
	if !strings.Contains(res.Message, "+ 0 through a wrapper") || !strings.Contains(res.Message, "1 std path join") {
		t.Errorf("expected the totals in the message, got %q", res.Message)
	}
	var got appPathStdBridgeAllowlist
	if err := json.Unmarshal([]byte(written), &got); err != nil {
		t.Fatalf("the rewritten allowlist parses: %v", err)
	}
	if _, ok := got.WrapperCalls[x]; ok {
		t.Errorf("an app-tree `on_volume` isn't a wrapper call, so its budget drops off")
	}
	if got.StdJoin[x] != 1 {
		t.Errorf("the join budget ratchets to the count, got %d", got.StdJoin[x])
	}
	files["operations_host/x.rs"] += "fn g(dir: &Path) { let r = dir.with_file_name(n); }\n"
	_, _, err = runAppPathStdBridgeWith(t, files, &appPathStdBridgeAllowlist{StdJoin: map[string]int{x: 1}})
	if err == nil || !strings.Contains(err.Error(), "of a name onto a folder over the file's budget") {
		t.Fatalf("a join over budget must fail, got %v", err)
	}
}
