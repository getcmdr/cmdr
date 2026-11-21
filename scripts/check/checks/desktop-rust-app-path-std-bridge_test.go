package checks

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// runAppPathStdBridgeOn writes the supplied files (repo-root-relative, or under the app's
// `src/`) plus an optional allowlist into a temp repo and runs the check, as a local run.
func runAppPathStdBridgeOn(t *testing.T, files map[string]string, allowed map[string]int) (CheckResult, string, error) {
	t.Helper()
	var list *appPathStdBridgeAllowlist
	if allowed != nil {
		list = &appPathStdBridgeAllowlist{Comment: "test", Files: allowed}
	}
	return runAppPathStdBridgeWith(t, files, list)
}

// runAppPathStdBridgeWith is [runAppPathStdBridgeOn] with a whole allowlist (both budgets).
func runAppPathStdBridgeWith(t *testing.T, files map[string]string, list *appPathStdBridgeAllowlist) (CheckResult, string, error) {
	t.Helper()
	root := t.TempDir()
	seedAppFixtureWorkspace(t, root)
	srcDir := filepath.Join(root, "apps", "desktop", "src-tauri", "src")
	for rel, body := range files {
		full := filepath.Join(srcDir, rel)
		if strings.HasPrefix(rel, "crates/") {
			full = filepath.Join(root, rel)
		}
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatalf("mkdir: %v", err)
		}
		if err := os.WriteFile(full, []byte(body), 0o644); err != nil {
			t.Fatalf("write: %v", err)
		}
	}
	allowlistPath := filepath.Join(root, appPathStdBridgeAllowlistRelPath)
	if list != nil {
		if err := os.MkdirAll(filepath.Dir(allowlistPath), 0o755); err != nil {
			t.Fatalf("mkdir: %v", err)
		}
		data, _ := json.Marshal(list)
		if err := os.WriteFile(allowlistPath, data, 0o644); err != nil {
			t.Fatalf("write allowlist: %v", err)
		}
	}
	res, err := RunAppPathStdBridge(&CheckContext{RootDir: root})
	written, _ := os.ReadFile(allowlistPath)
	return res, string(written), err
}

func TestAppPathStdBridge_FlagsAnUnbudgetedCall(t *testing.T) {
	for name, body := range map[string]string{
		"in":  "fn f(p: &Path) { v.list(&AppPath::legacy_unchecked(p)); }\n",
		"out": "fn f(p: &AppPath) { let p = p.as_std_legacy(); }\n",
	} {
		t.Run(name, func(t *testing.T) {
			_, _, err := runAppPathStdBridgeOn(t, map[string]string{"volume/x.rs": body}, nil)
			if err == nil || !strings.Contains(err.Error(), "volume/x.rs:1") {
				t.Fatalf("expected the call flagged with its line, got %v", err)
			}
		})
	}
}

func TestAppPathStdBridge_SkipsCommentsAndTheModuleItself(t *testing.T) {
	res, _, err := runAppPathStdBridgeOn(t, map[string]string{
		"volume/x.rs":                           "// `AppPath::legacy_unchecked(p)` is the bridge\n",
		"crates/cmdr-fs/src/app_path/bridge.rs": "pub fn legacy_unchecked(path: &Path) {}\n",
	}, nil)
	if err != nil || res.Code != ResultSuccess {
		t.Fatalf("expected success, got %v / %+v", err, res)
	}
}

func TestAppPathStdBridge_ABudgetPassesAndShrinksToTheCount(t *testing.T) {
	body := "fn f(p: &Path) { let a = AppPath::legacy_unchecked(p); }\n"
	res, written, err := runAppPathStdBridgeOn(t, map[string]string{"volume/x.rs": body, "volume/gone.rs": "fn g() {}\n"},
		map[string]int{"apps/desktop/src-tauri/src/volume/x.rs": 3, "apps/desktop/src-tauri/src/volume/gone.rs": 2})
	if err != nil {
		t.Fatalf("a budgeted call must pass, got %v", err)
	}
	if !res.MadeChanges && !strings.Contains(res.Message, "ratcheted") {
		t.Errorf("expected the slack reported, got %+v", res)
	}
	var list appPathStdBridgeAllowlist
	if err := json.Unmarshal([]byte(written), &list); err != nil {
		t.Fatalf("the rewritten allowlist parses: %v", err)
	}
	if got := list.Files["apps/desktop/src-tauri/src/volume/x.rs"]; got != 1 {
		t.Errorf("x.rs ratchets to its count, got %d", got)
	}
	if _, ok := list.Files["apps/desktop/src-tauri/src/volume/gone.rs"]; ok {
		t.Error("a file with no calls left drops off")
	}
}

// Regression anchor for #1317: the meter counted bridge LINES, so a line holding two calls
// counted one, and rustfmt splitting it pushed the file over a budget taken before the split.
func TestAppPathStdBridge_CountsEveryCallOnALine(t *testing.T) {
	packed := "fn f(x: &Path, y: &Path) { let (a, b) = (AppPath::legacy_unchecked(x), AppPath::legacy_unchecked(y)); }\n"
	split := "fn f(x: &Path, y: &Path) {\n    let (a, b) = (\n        AppPath::legacy_unchecked(x),\n        AppPath::legacy_unchecked(y),\n    );\n}\n"
	for name, body := range map[string]string{"packed": packed, "split": split} {
		t.Run(name, func(t *testing.T) {
			_, written, err := runAppPathStdBridgeOn(t, map[string]string{"volume/x.rs": body},
				map[string]int{"apps/desktop/src-tauri/src/volume/x.rs": 5})
			if err != nil {
				t.Fatalf("a budgeted file must pass, got %v", err)
			}
			var list appPathStdBridgeAllowlist
			if err := json.Unmarshal([]byte(written), &list); err != nil {
				t.Fatalf("the rewritten allowlist parses: %v", err)
			}
			if got := list.Files["apps/desktop/src-tauri/src/volume/x.rs"]; got != 2 {
				t.Errorf("two calls count two however the line is wrapped, got %d", got)
			}
		})
	}
}

func TestAppPathStdBridge_FlagsAppTextTurnedIntoAStdPath(t *testing.T) {
	for name, body := range map[string]string{
		"path":       "fn f(entry: &FileEntry) { let p = Path::new(&entry.path); }\n",
		"name":       "fn f(e: &FileEntry) { let p = PathBuf::from(e.name.clone()); }\n",
		"as_str":     "fn f(p: &AppPath) { let q = Path::new(p.as_str()); }\n",
		"child_name": "fn f(entry: &FileEntry) { let n = ChildName::new(&entry.name)?; }\n",
	} {
		t.Run(name, func(t *testing.T) {
			_, _, err := runAppPathStdBridgeOn(t, map[string]string{"volume/x.rs": body}, nil)
			if err == nil || !strings.Contains(err.Error(), "turning app text into a std path") {
				t.Fatalf("expected the site flagged, got %v", err)
			}
		})
	}
}

func TestAppPathStdBridge_AnEscapeProducerWaitsForTheTextSitesToBeGone(t *testing.T) {
	files := map[string]string{
		"volume/reader.rs": "fn f(p: &Path) { let a = AppPathBuf::from_native(p)?; }\n",
		"volume/x.rs":      "fn g(entry: &FileEntry) { let p = Path::new(&entry.path); }\n",
	}
	budget := map[string]int{"apps/desktop/src-tauri/src/volume/x.rs": 1}
	_, _, err := runAppPathStdBridgeWith(t, files, &appPathStdBridgeAllowlist{Files: map[string]int{}, AppTextToPath: budget})
	if err == nil || !strings.Contains(err.Error(), "escape-producing") {
		t.Fatalf("a producer must fail while app text still becomes a std path, got %v", err)
	}
	delete(files, "volume/x.rs")
	res, _, err := runAppPathStdBridgeWith(t, files, &appPathStdBridgeAllowlist{Files: map[string]int{}, AppTextToPath: map[string]int{}})
	if err != nil || res.Code != ResultSuccess {
		t.Fatalf("with no text sites left a producer passes, got %v / %+v", err, res)
	}
	// A test file's producer (a scratch dir's app path) isn't the gate's business.
	testOnly := map[string]string{
		"volume/reader_test.rs": "fn t(p: &Path) { let a = AppPathBuf::from_native(p).unwrap(); }\n",
		"volume/x.rs":           "fn g(entry: &FileEntry) { let p = Path::new(&entry.path); }\n",
	}
	if _, _, err := runAppPathStdBridgeWith(t, testOnly, &appPathStdBridgeAllowlist{Files: map[string]int{}, AppTextToPath: budget}); err != nil {
		t.Fatalf("a test-only producer passes, got %v", err)
	}
}

// Review 2's N2: the dominant IPC shapes count too, on their own coarse budget.
func TestAppPathStdBridge_FlagsBarePathTextTurnedIntoAStdPath(t *testing.T) {
	for name, body := range map[string]string{
		"pathbuf_from": "fn rename(path: String) { let p = PathBuf::from(&path); }\n",
		"path_new":     "fn f(dest_dir: &str) { let p = Path::new(&dest_dir); }\n",
		"map":          "fn f(paths: Vec<String>) { let v: Vec<_> = paths.into_iter().map(PathBuf::from).collect(); }\n",
	} {
		t.Run(name, func(t *testing.T) {
			_, _, err := runAppPathStdBridgeOn(t, map[string]string{"commands/x.rs": body}, nil)
			if err == nil || !strings.Contains(err.Error(), "handing path text to a std path constructor") {
				t.Fatalf("expected the site flagged, got %v", err)
			}
		})
	}
}

// Review 2's N2: a producer is a producer however it's spelled; the non-escaping crossing isn't one.
func TestAppPathStdBridge_SeesEveryProducerSpellingAndNotTheUnicodeCrossing(t *testing.T) {
	budget := &appPathStdBridgeAllowlist{Files: map[string]int{}, PathFromBareText: map[string]int{"apps/desktop/src-tauri/src/volume/x.rs": 1}}
	text := "fn g(path: String) { let p = PathBuf::from(&path); }\n"
	for name, body := range map[string]string{
		"fn_pointer": "fn f(v: Vec<PathBuf>) { v.iter().map(|p| p.as_path()).map(AppPathBuf::from_native); }\n",
		"bare_use":   "use cmdr_fs::app_path::escape_bytes;\nfn f(b: &[u8]) { let s = escape_bytes(b); }\n",
		"name":       "fn f(n: &OsStr) { let a = AppNameBuf::from_native_os(n); }\n",
	} {
		t.Run(name, func(t *testing.T) {
			_, _, err := runAppPathStdBridgeWith(t, map[string]string{"volume/reader.rs": body, "volume/x.rs": text}, budget)
			if err == nil || !strings.Contains(err.Error(), "escape-producing") {
				t.Fatalf("expected the producer flagged, got %v", err)
			}
		})
	}
	unicode := "fn f(p: &Path) { let a = AppPathBuf::from_native_unicode(p)?; let n = AppNameBuf::from_native_os_unicode(o)?; }\n"
	if _, _, err := runAppPathStdBridgeWith(t, map[string]string{"volume/reader.rs": unicode, "volume/x.rs": text}, budget); err != nil {
		t.Fatalf("the non-escaping crossing passes, got %v", err)
	}
}

// Review 2's N5: OS text into an app-spelling door is refused outright.
func TestAppPathStdBridge_RefusesOsTextIntoAnAppSpellingDoor(t *testing.T) {
	for name, body := range map[string]string{
		"name":   "fn f(n: &OsStr) { let a = AppName::from_app_spelling(n.to_str()?)?; }\n",
		"path":   "fn f(p: &Path) { let a = AppPath::from_normalized(&p.to_string_lossy())?; }\n",
		"wire":   "fn f(p: &Path) { let a = AppPathBuf::from_wire(&p.display().to_string())?; }\n",
		"a_test": "fn t(p: &Path) { AppPathBuf::parse(p.to_str().unwrap(), PathFlavor::HOST).unwrap(); }\n",
	} {
		t.Run(name, func(t *testing.T) {
			_, _, err := runAppPathStdBridgeOn(t, map[string]string{"volume/x_test.rs": body}, nil)
			if err == nil || !strings.Contains(err.Error(), "hand OS text") {
				t.Fatalf("expected the hop refused, got %v", err)
			}
		})
	}
}
