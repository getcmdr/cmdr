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

// A Rust test is OS-neutral by default: it runs on macOS, Linux, and Windows (`docs/testing.md` § "A test runs on
// every OS"). A whole test file or test module gated to some OSes is the exception, and it says why in a comment on
// the gate's line or the line right above it. This check finds every such gate whose reason is missing:
//
//   - an inner `#![cfg(<OS predicate>)]` in a test file (`isRustTestFile`), and
//   - an outer `#[cfg(<OS predicate>)]` on a `mod` whose name says it's tests (`mod foo_test;`,
//     `mod tests { … }`), in any file.
//
// Single test functions are out of scope: one cell's gate sits next to the code that explains it, and counting them
// would turn this into a wall of noise. The gates that predate the rule live in `os-test-gate-reason-allowlist.json`,
// which only shrinks: a local run drops an entry once its gate has a reason or is gone.

// osTestGateAllowlistRelPath is where the gates that predate the rule are listed.
const osTestGateAllowlistRelPath = "scripts/check/checks/os-test-gate-reason-allowlist.json"

// osTestGateAllowlist is keyed on the gate's identity: the file for an inner gate, `file#mod` for a module gate. A
// line number would move on every edit above it.
type osTestGateAllowlist struct {
	Comment string                       `json:"$comment,omitempty"`
	Gates   map[string]osTestGateAllowed `json:"gates"`
}

type osTestGateAllowed struct {
	Reason string `json:"reason"`
}

// osTestGate is one gate with no reason.
type osTestGate struct {
	key       string
	relPath   string
	line      int
	predicate string
}

// testModDecl matches the item after a module gate: `mod name;` or `mod name {`, `pub` and friends allowed.
var testModDecl = regexp.MustCompile(`^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z0-9_]+)\s*[;{]`)

// RunOSTestGateReason fails on an OS-gated test file or test module with no reason comment.
func RunOSTestGateReason(ctx *CheckContext) (CheckResult, error) {
	roots, err := ScannerRoots(ctx.RootDir, "desktop-rust-os-test-gate-reason")
	if err != nil {
		return CheckResult{}, err
	}
	gates, scanned, err := scanOSTestGates(ctx.RootDir, roots)
	if err != nil {
		return CheckResult{}, err
	}

	allowlist := loadOSTestGateAllowlist(ctx.RootDir)
	stale := shrinkOSTestGateAllowlist(&allowlist, gates)
	if len(stale) > 0 && !ctx.CI {
		if err := writeJSONAllowlist(filepath.Join(ctx.RootDir, osTestGateAllowlistRelPath), allowlist); err != nil {
			return CheckResult{}, err
		}
		reformatWithOxfmt(ctx.RootDir, osTestGateAllowlistRelPath)
	}
	staleMsg := formatOSTestGateStaleness(stale, ctx.CI)

	var missing []osTestGate
	for _, g := range gates {
		if _, ok := allowlist.Gates[g.key]; !ok {
			missing = append(missing, g)
		}
	}
	if len(missing) > 0 {
		msg := formatOSTestGatesMissing(missing)
		return CheckResult{}, fmt.Errorf("%s", strings.Join(appendNonEmpty([]string{msg}, staleMsg), "\n"))
	}

	okMsg := fmt.Sprintf("%d Rust %s scanned, every OS-gated test file and module says why (%d older %s allowlisted)",
		scanned, Pluralize(scanned, "file", "files"), len(allowlist.Gates), Pluralize(len(allowlist.Gates), "gate", "gates"))
	if staleMsg != "" {
		return SuccessWithChanges(okMsg + "; " + staleMsg), nil
	}
	return Success(okMsg), nil
}

// scanOSTestGates returns every gate with no reason under the roots, and how many files it read.
func scanOSTestGates(rootDir string, roots []string) ([]osTestGate, int, error) {
	var gates []osTestGate
	scanned := 0
	for _, root := range roots {
		walkErr := corpus.WalkDir(root, func(path string, d os.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if d.IsDir() || !strings.HasSuffix(d.Name(), ".rs") {
				return nil
			}
			source, readErr := corpus.ReadFile(path)
			if readErr != nil {
				return readErr
			}
			scanned++
			relPath, relErr := filepath.Rel(rootDir, path)
			if relErr != nil {
				relPath = path
			}
			gates = append(gates, osTestGatesIn(filepath.ToSlash(relPath), source, isRustTestFile(path))...)
			return nil
		})
		if walkErr != nil {
			return nil, 0, fmt.Errorf("failed to scan Rust files: %w", walkErr)
		}
	}
	return gates, scanned, nil
}

// shrinkOSTestGateAllowlist drops every entry whose gate has a reason now or is gone, and returns their keys.
func shrinkOSTestGateAllowlist(list *osTestGateAllowlist, gates []osTestGate) []string {
	found := map[string]bool{}
	for _, g := range gates {
		found[g.key] = true
	}
	var stale []string
	for _, key := range sortedKeys(list.Gates) {
		if !found[key] {
			delete(list.Gates, key)
			stale = append(stale, key)
		}
	}
	return stale
}

func formatOSTestGateStaleness(stale []string, ci bool) string {
	if len(stale) == 0 {
		return ""
	}
	verb := "Shrink-wrapped allowlist, these gates have a reason now or are gone"
	if ci {
		verb = "Stale allowlist entries (a local run drops them)"
	}
	return fmt.Sprintf("%s:\n  - %s", verb, strings.Join(stale, "\n  - "))
}

func formatOSTestGatesMissing(missing []osTestGate) string {
	sort.Slice(missing, func(i, j int) bool {
		if missing[i].relPath != missing[j].relPath {
			return missing[i].relPath < missing[j].relPath
		}
		return missing[i].line < missing[j].line
	})
	var sb strings.Builder
	for _, g := range missing {
		sb.WriteString(fmt.Sprintf("  - %s:%d cfg(%s) [%s]\n", g.relPath, g.line, g.predicate, g.key))
	}
	return fmt.Sprintf(
		"%d OS-gated test %s with no reason. A test runs on every OS unless it can't: say why in a comment on the "+
			"gate's line or right above it (`#![cfg(unix)] // POSIX modes`), or make the cells OS-neutral "+
			"(docs/testing.md § \"A test runs on every OS\"):\n%s",
		len(missing), Pluralize(len(missing), "gate", "gates"), strings.TrimRight(sb.String(), "\n"))
}

// osTestGatesIn returns one file's OS gates on test files and test modules that carry no reason comment.
func osTestGatesIn(relPath, source string, testFile bool) []osTestGate {
	if !strings.Contains(source, "cfg") {
		return nil
	}
	masked, _ := maskRustSource(source)
	lineStarts := lineStartOffsets(source)
	lines := strings.Split(source, "\n")
	var out []osTestGate
	for _, c := range parsedCfgsIn(masked, source) {
		if c.kind != "cfg" || !c.attr || !c.node.mentionsOS() {
			continue
		}
		line := lineForOffset(lineStarts, c.start)
		key := ""
		switch {
		case c.inner && testFile:
			key = relPath
		case !c.inner:
			name := gatedModName(lines, line)
			if name == "" || !strings.Contains(name, "test") {
				continue
			}
			key = relPath + "#" + name
		default:
			continue
		}
		if gateHasReason(lines, line) {
			continue
		}
		out = append(out, osTestGate{key: key, relPath: relPath, line: line, predicate: c.predicate})
	}
	return out
}

// gatedModName is the module an outer attribute on 1-based `line` gates: the first non-attribute line after it, when
// that's a `mod` item. "" otherwise.
func gatedModName(lines []string, line int) string {
	for i := line; i < len(lines); i++ {
		trimmed := strings.TrimSpace(lines[i])
		if trimmed == "" || strings.HasPrefix(trimmed, "#[") || strings.HasPrefix(trimmed, "//") {
			continue
		}
		if m := testModDecl.FindStringSubmatch(lines[i]); m != nil {
			return m[1]
		}
		return ""
	}
	return ""
}

// gateHasReason reports whether the gate on 1-based `line` has a comment after it on the same line, or a comment line
// directly above it (other attributes in between allowed).
func gateHasReason(lines []string, line int) bool {
	if line < 1 || line > len(lines) {
		return false
	}
	gateLine := lines[line-1]
	if close := strings.LastIndex(gateLine, "]"); close >= 0 && strings.Contains(gateLine[close:], "//") {
		return true
	}
	for i := line - 2; i >= 0; i-- {
		trimmed := strings.TrimSpace(lines[i])
		if strings.HasPrefix(trimmed, "#[") || strings.HasPrefix(trimmed, "#![") {
			continue
		}
		return strings.HasPrefix(trimmed, "//")
	}
	return false
}

// loadOSTestGateAllowlist reads the allowlist. A missing or unparsable file yields an empty one, so every gate with
// no reason gets reported.
func loadOSTestGateAllowlist(rootDir string) osTestGateAllowlist {
	list := osTestGateAllowlist{Gates: map[string]osTestGateAllowed{}}
	data, err := os.ReadFile(filepath.Join(rootDir, osTestGateAllowlistRelPath))
	if err != nil {
		return list
	}
	if err := json.Unmarshal(data, &list); err != nil || list.Gates == nil {
		return osTestGateAllowlist{Gates: map[string]osTestGateAllowed{}}
	}
	return list
}
