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
