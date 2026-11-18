package checks

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// appPathStdBridgeAllowlistRelPath is the per-file budget of transition-bridge calls.
const appPathStdBridgeAllowlistRelPath = "scripts/check/checks/app-path-std-bridge-allowlist.json"

// appPathModuleDir is where the bridge is defined and tested; its calls there aren't uses.
const appPathModuleDir = "crates/cmdr-fs/src/app_path/"

// appPathStdBridgePattern matches a call of either bridge function: the one door from
// `std::path` into an app path that skips validation (`AppPath::legacy_unchecked`), and the
// one way back out for `std::path` arithmetic (`.as_std_legacy()`).
var appPathStdBridgePattern = gateRegexp(regexp.MustCompile(`\b(legacy_unchecked|as_std_legacy)\s*\(`))

type appPathStdBridgeAllowlist struct {
	Comment string         `json:"$comment"`
	Files   map[string]int `json:"files"`
}

type appPathStdBridgeSite struct {
	relPath string
	line    int
	text    string
}

// RunAppPathStdBridge counts the app path's transition-bridge calls per file
// (`AppPath::legacy_unchecked`, `AppPath::as_std_legacy`; `crates/cmdr-fs/DETAILS.md`
// § "App paths"), against a budget that only shrinks. It's the progress meter while the
// crates move from `std::path` onto the app path: each crate's conversion drives its
// files' counts to zero, and once the whole tree is at zero both functions go.
func RunAppPathStdBridge(ctx *CheckContext) (CheckResult, error) {
	roots, err := ScannerRoots(ctx.RootDir, "desktop-rust-app-path-std-bridge")
	if err != nil {
		return CheckResult{}, err
	}
	var sites []appPathStdBridgeSite
	scanned := 0
	for _, root := range roots {
		rootSites, rootScanned, scanErr := scanForAppPathStdBridge(ctx.RootDir, root)
		if scanErr != nil {
			return CheckResult{}, fmt.Errorf("failed to scan Rust files: %w", scanErr)
		}
		sites = append(sites, rootSites...)
		scanned += rootScanned
	}

	allowlist := loadAppPathStdBridgeAllowlist(ctx.RootDir)
	counts := map[string]int{}
	for _, s := range sites {
		counts[s.relPath]++
	}
	staleChanges := shrinkwrapCountAllowlist(allowlist.Files, counts, "no bridge calls left")
	if len(staleChanges) > 0 && !ctx.CI {
		if err := writeJSONAllowlist(filepath.Join(ctx.RootDir, appPathStdBridgeAllowlistRelPath), allowlist); err != nil {
			return CheckResult{}, err
		}
		reformatWithOxfmt(ctx.RootDir, appPathStdBridgeAllowlistRelPath)
	}
	staleMsg := formatLockPoisonStaleness(staleChanges, ctx.CI)

	var over []appPathStdBridgeSite
	for _, s := range sites {
		if counts[s.relPath] > allowlist.Files[s.relPath] {
			over = append(over, s)
		}
	}
	if len(over) > 0 {
		sort.Slice(over, func(i, j int) bool {
			if over[i].relPath == over[j].relPath {
				return over[i].line < over[j].line
			}
			return over[i].relPath < over[j].relPath
		})
		var sb strings.Builder
		for _, s := range over {
			sb.WriteString(fmt.Sprintf("  %s:%d: %s\n", s.relPath, s.line, s.text))
		}
		msg := fmt.Sprintf(
			"found %d app-path bridge %s over the file's budget in %s (speak `AppPath` arithmetic instead: "+
				"`join`, `parent`, `strip_prefix`, and the native crossings `to_native` / `from_native`; "+
				"the bridge only shrinks, crates/cmdr-fs/DETAILS.md § \"App paths\"):\n%s",
			len(over), Pluralize(len(over), "call", "calls"), appPathStdBridgeAllowlistRelPath,
			strings.TrimRight(sb.String(), "\n"),
		)
		return CheckResult{}, fmt.Errorf("%s", strings.Join(appendNonEmpty([]string{msg}, staleMsg), "\n"))
	}

	total := 0
	for _, n := range allowlist.Files {
		total += n
	}
	okMsg := fmt.Sprintf("%d Rust %s scanned, %d app-path bridge %s left in %d %s",
		scanned, Pluralize(scanned, "file", "files"),
		total, Pluralize(total, "call", "calls"),
		len(allowlist.Files), Pluralize(len(allowlist.Files), "file", "files"))
	if staleMsg != "" {
		return SuccessWithChanges(okMsg + "; " + staleMsg), nil
	}
	return Success(okMsg), nil
}

// shrinkwrapCountAllowlist drops entries whose file has no sites left and ratchets the
// rest down to the current count, reporting each change.
func shrinkwrapCountAllowlist(files map[string]int, counts map[string]int, goneReason string) []string {
	var changes []string
	for _, path := range sortedKeys(files) {
		allowed := files[path]
		current := counts[path]
		switch {
		case current == 0:
			delete(files, path)
			changes = append(changes, fmt.Sprintf("removed %s (%s)", path, goneReason))
		case current < allowed:
			files[path] = current
			changes = append(changes, fmt.Sprintf("ratcheted %s: %d → %d", path, allowed, current))
		}
	}
	return changes
}

// loadAppPathStdBridgeAllowlist reads the budget. A missing or unparsable file yields an
// empty one, so every call gets reported.
func loadAppPathStdBridgeAllowlist(rootDir string) appPathStdBridgeAllowlist {
	list := appPathStdBridgeAllowlist{Files: map[string]int{}}
	data, err := os.ReadFile(filepath.Join(rootDir, appPathStdBridgeAllowlistRelPath))
	if err != nil {
		return list
	}
	if err := json.Unmarshal(data, &list); err != nil || list.Files == nil {
		return appPathStdBridgeAllowlist{Comment: list.Comment, Files: map[string]int{}}
	}
	return list
}

// scanForAppPathStdBridge returns every bridge call under srcDir, tests included (a test
// that bridges is a test the conversion still has to visit), outside the app-path module.
func scanForAppPathStdBridge(rootDir, srcDir string) ([]appPathStdBridgeSite, int, error) {
	var sites []appPathStdBridgeSite
	scanned := 0
	err := corpus.WalkDir(srcDir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() || !strings.HasSuffix(d.Name(), ".rs") {
			return nil
		}
		scanned++
		relPath, relErr := filepath.Rel(rootDir, path)
		if relErr != nil {
			relPath = path
		}
		relPath = filepath.ToSlash(relPath)
		if strings.HasPrefix(relPath, appPathModuleDir) {
			return nil
		}
		lines, readErr := corpus.Lines(path)
		if readErr != nil {
			return readErr
		}
		for idx, line := range lines {
			if !appPathStdBridgePattern.MatchString(line) {
				continue
			}
			if strings.HasPrefix(strings.TrimLeft(line, " \t"), "//") {
				continue
			}
			sites = append(sites, appPathStdBridgeSite{relPath: relPath, line: idx + 1, text: strings.TrimSpace(line)})
		}
		return nil
	})
	return sites, scanned, err
}
