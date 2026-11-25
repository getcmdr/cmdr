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

// appPathStdBridgePattern matches a call of a bridge function: the one door from `std::path`
// into an app path that skips validation (`AppPath::legacy_unchecked`), the one way back out
// for `std::path` arithmetic (`.as_std_legacy()`), and the field form for a borrowed return
// (`BridgedPath::new`, milestone 4's `root()` rule).
var appPathStdBridgePattern = gateRegexp(regexp.MustCompile(`\b(legacy_unchecked|as_std_legacy|BridgedPath::new)\s*\(`))

// appTextToPathPatterns match app TEXT becoming a `std::path` value: a `FileEntry.path` or
// `.name` (or any app path's `.as_str()`) handed to `Path::new`, `PathBuf::from`,
// `OsStr::new`, or `OsString::from`, or a listed `.name` handed to `ChildName::new` (a
// `std::path`-world name). Today that's the identity. Once a name holds an escape it isn't:
// the `Path` carries the escape's own UTF-8 bytes, so `std::fs` opens a file literally named
// with it, and the bridge reads it back as literal text.
//
// ❗ A TRIPWIRE, not a gate: no text scan can see app text arrive through a `let`, a closure,
// `.into()`, an `impl AsRef<Path>` parameter, or a `format!` join, so a zero here doesn't prove
// none is left. The sound mechanism is the type's (docs/specs/app-path-plan.md § "Keeping it
// that way": typed carriers with no conversion to `std::path`, compile-asserted, and a
// type-resolved ban on the text exits before milestone 10f). These two budgets are the meter.
var appTextToPathPatterns = gateRegexps(
	regexp.MustCompile(`\b(Path::new|PathBuf::from|OsStr::new|OsString::from)\s*\(\s*&?[\w.\[\]]*\.(path|name)(\.clone\(\)|\.as_str\(\)|\.to_string\(\))?\s*\)`),
	regexp.MustCompile(`\b(Path::new|PathBuf::from|OsStr::new|OsString::from)\s*\([^()]*\.as_str\(\)\s*\)`),
	regexp.MustCompile(`\bChildName::new\s*\(\s*&?[\w.\[\]]*\.name\b`),
)

// bareTextToPathPatterns widen the tripwire to the shapes review 2 found it missing (N2): a
// bare path- or name-shaped identifier handed to a `std::path` constructor (`PathBuf::from(&path)`,
// `Path::new(&dest_dir)`, the dominant IPC shape) and a whole collection mapped through one
// (`paths.into_iter().map(PathBuf::from)`). Many such sites are native, so this budget is a
// coarse floor of its own (`pathFromBareText`), seeded from the tree.
var bareTextToPathPatterns = gateRegexps(
	regexp.MustCompile(`\b(Path::new|PathBuf::from|OsStr::new|OsString::from)\s*\(\s*&?(path|paths|name|dir|root|\w+_(path|paths|name|dir|root))\s*\)`),
	regexp.MustCompile(`\.map\(\s*(PathBuf::from|Path::new|OsStr::new|OsString::from)\s*\)`),
)

// osTextIntoAppDoorPattern matches review 2's N5, the reverse trap: OS text (`to_str()`,
// `to_string_lossy()`, `.display()`) handed on one line to a door that takes APP spelling, so
// a literal escape code point a native name holds as text turns live. Zero tolerance,
// production and tests alike: the native doors (`from_native_os`, `from_native`) exist.
var osTextIntoAppDoorPattern = gateRegexp(regexp.MustCompile(
	`\b(AppName::from_app_spelling|AppPath::from_normalized(_as)?|AppPathBuf::from_wire|AppPathBuf::parse|RelativeAppPath::new)\s*\(.*\.(to_str|to_string_lossy|display)\(\)`))

// escapeProducerPattern matches production code that turns NATIVE names into app spelling,
// the code that makes an escape exist for a name that isn't valid Unicode text: a call, a
// function pointer (`.map(AppPathBuf::from_native)`), or a bare `escape_bytes(` after a `use`.
// `from_native_unicode` (and `from_native_os_unicode`) aren't producers: they refuse, typed,
// any name that would need an escape, which is what milestones 9, 10a, and 10b cross with.
var escapeProducerPattern = gateRegexp(regexp.MustCompile(
	`\b(AppPathBuf|AppNameBuf)::from_native(_as|_posix|_windows|_os|_bytes|_units)?\b|(^|[^\w.])(app_path::)?escape_(bytes|units)\s*\(`))

type appPathStdBridgeAllowlist struct {
	Comment string         `json:"$comment"`
	Files   map[string]int `json:"files"`
	// AppTextToPath is the per-file budget of app text turned into a `std::path` value.
	AppTextToPath map[string]int `json:"appTextToPath"`
	// PathFromBareText is the per-file budget of a bare path- or name-shaped identifier (or a
	// collection mapped) handed to a `std::path` constructor: the coarse floor beside `AppTextToPath`.
	PathFromBareText map[string]int `json:"pathFromBareText"`
	// WrapperCalls is the per-file budget of calls through a named bridge wrapper
	// (`appPathBridgeWrappers`), so the meter reads direct plus wrapper crossings (#1469).
	WrapperCalls map[string]int `json:"wrapperCalls"`
	// StdJoin is the per-file budget of `std::path` joins of a name onto a folder in the write
	// engine's production code (`stdJoinRoots`).
	StdJoin map[string]int `json:"stdJoin"`
}

type appPathScan struct {
	bridge       []appPathStdBridgeSite
	wrapperCalls []appPathStdBridgeSite
	stdJoins     []appPathStdBridgeSite
	joinOrphans  []orphanDirective
	textToPath   []appPathStdBridgeSite
	bareText     []appPathStdBridgeSite
	producers    []appPathStdBridgeSite
	osTextHops   []appPathStdBridgeSite
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
	var scan appPathScan
	scanned := 0
	for _, root := range roots {
		rootScan, rootScanned, scanErr := scanForAppPathStdBridge(ctx.RootDir, root)
		if scanErr != nil {
			return CheckResult{}, fmt.Errorf("failed to scan Rust files: %w", scanErr)
		}
		scan.bridge = append(scan.bridge, rootScan.bridge...)
		scan.wrapperCalls = append(scan.wrapperCalls, rootScan.wrapperCalls...)
		scan.stdJoins = append(scan.stdJoins, rootScan.stdJoins...)
		scan.joinOrphans = append(scan.joinOrphans, rootScan.joinOrphans...)
		scan.textToPath = append(scan.textToPath, rootScan.textToPath...)
		scan.bareText = append(scan.bareText, rootScan.bareText...)
		scan.producers = append(scan.producers, rootScan.producers...)
		scan.osTextHops = append(scan.osTextHops, rootScan.osTextHops...)
		scanned += rootScanned
	}
	sites := scan.bridge

	allowlist := loadAppPathStdBridgeAllowlist(ctx.RootDir)
	counts := siteCounts(sites)
	textCounts := siteCounts(scan.textToPath)
	bareCounts := siteCounts(scan.bareText)
	staleChanges := shrinkwrapCountAllowlist(allowlist.Files, counts, "no bridge calls left")
	staleChanges = append(staleChanges,
		shrinkwrapCountAllowlist(allowlist.AppTextToPath, textCounts, "no app text turned into a std path left")...)
	staleChanges = append(staleChanges,
		shrinkwrapCountAllowlist(allowlist.PathFromBareText, bareCounts, "no bare path text turned into a std path left")...)
	staleChanges = append(staleChanges,
		shrinkwrapCountAllowlist(allowlist.WrapperCalls, siteCounts(scan.wrapperCalls), "no bridge wrapper calls left")...)
	staleChanges = append(staleChanges,
		shrinkwrapCountAllowlist(allowlist.StdJoin, siteCounts(scan.stdJoins), "no std path joins left")...)
	if len(staleChanges) > 0 && !ctx.CI {
		if err := writeJSONAllowlist(filepath.Join(ctx.RootDir, appPathStdBridgeAllowlistRelPath), allowlist); err != nil {
			return CheckResult{}, err
		}
		reformatWithOxfmt(ctx.RootDir, appPathStdBridgeAllowlistRelPath)
	}
	staleMsg := formatLockPoisonStaleness(staleChanges, ctx.CI)

	failures, textTotal := scan.failures(allowlist)
	if len(failures) > 0 {
		return CheckResult{}, fmt.Errorf("%s", strings.Join(appendNonEmpty(failures, staleMsg), "\n"))
	}

	total, wrapperTotal, joinTotal := budgetTotal(allowlist.Files), budgetTotal(allowlist.WrapperCalls), budgetTotal(allowlist.StdJoin)
	okMsg := fmt.Sprintf("%d Rust %s scanned, %d app-path bridge %s left (%d direct in %d %s + %d through a wrapper), "+
		"%d app-text-to-std-path %s, %d std path %s in the write engine left",
		scanned, Pluralize(scanned, "file", "files"),
		total+wrapperTotal, Pluralize(total+wrapperTotal, "crossing", "crossings"),
		total, len(allowlist.Files), Pluralize(len(allowlist.Files), "file", "files"), wrapperTotal,
		textTotal, Pluralize(textTotal, "site", "sites"),
		joinTotal, Pluralize(joinTotal, "join", "joins"))
	if staleMsg != "" {
		return SuccessWithChanges(okMsg + "; " + staleMsg), nil
	}
	return Success(okMsg), nil
}

// classify files one line under each of the scan's lists it belongs to. The bridge list gets
// one site per CALL, so two calls on a line count two and rustfmt splitting that line never
// moves the count (#1317); the two tripwires stay per line.
func (scan *appPathScan) classify(line string, site appPathStdBridgeSite, testFile bool) {
	for range appPathStdBridgePattern.FindAllStringSubmatchIndex(line, -1) {
		scan.bridge = append(scan.bridge, site)
	}
	for range wrapperCallCount(line, site.relPath) {
		scan.wrapperCalls = append(scan.wrapperCalls, site)
	}
	member := false
	for _, pattern := range appTextToPathPatterns {
		if pattern.MatchString(line) {
			scan.textToPath = append(scan.textToPath, site)
			member = true
			break
		}
	}
	if !member {
		for _, pattern := range bareTextToPathPatterns {
			if pattern.MatchString(line) {
				scan.bareText = append(scan.bareText, site)
				break
			}
		}
	}
	if osTextIntoAppDoorPattern.MatchString(line) {
		scan.osTextHops = append(scan.osTextHops, site)
	}
	if !testFile && escapeProducerPattern.MatchString(line) {
		scan.producers = append(scan.producers, site)
	}
}

func budgetTotal(budget map[string]int) int {
	total := 0
	for _, n := range budget {
		total += n
	}
	return total
}

func siteCounts(sites []appPathStdBridgeSite) map[string]int {
	counts := map[string]int{}
	for _, s := range sites {
		counts[s.relPath]++
	}
	return counts
}

// overBudget returns every site in a file holding more sites than its budget allows.
func overBudget(sites []appPathStdBridgeSite, counts, budget map[string]int) []appPathStdBridgeSite {
	var over []appPathStdBridgeSite
	for _, s := range sites {
		if counts[s.relPath] > budget[s.relPath] {
			over = append(over, s)
		}
	}
	return over
}

func formatSites(sites []appPathStdBridgeSite) string {
	sort.Slice(sites, func(i, j int) bool {
		if sites[i].relPath == sites[j].relPath {
			return sites[i].line < sites[j].line
		}
		return sites[i].relPath < sites[j].relPath
	})
	var sb strings.Builder
	for _, s := range sites {
		sb.WriteString(fmt.Sprintf("  %s:%d: %s\n", s.relPath, s.line, s.text))
	}
	return strings.TrimRight(sb.String(), "\n")
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
	var list appPathStdBridgeAllowlist
	if data, err := os.ReadFile(filepath.Join(rootDir, appPathStdBridgeAllowlistRelPath)); err == nil {
		if json.Unmarshal(data, &list) != nil {
			list = appPathStdBridgeAllowlist{}
		}
	}
	for _, budget := range []*map[string]int{&list.Files, &list.AppTextToPath, &list.PathFromBareText, &list.WrapperCalls, &list.StdJoin} {
		if *budget == nil {
			*budget = map[string]int{}
		}
	}
	return list
}

// scanForAppPathStdBridge returns, under srcDir and outside the app-path module, every bridge
// call and every app-text-to-std-path site (tests included: a test doing either is a test the
// conversion still has to visit), and every escape producer in production code.
func scanForAppPathStdBridge(rootDir, srcDir string) (appPathScan, int, error) {
	var scan appPathScan
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
		testFile := isRustTestFile(relPath) || strings.Contains(relPath, "/testing/") || strings.Contains(relPath, "test_support")
		if !testFile && inStdJoinScope(relPath) {
			joins, orphans := scanStdJoins(relPath, lines)
			scan.stdJoins = append(scan.stdJoins, joins...)
			scan.joinOrphans = append(scan.joinOrphans, orphans...)
		}
		for idx, line := range lines {
			if strings.HasPrefix(strings.TrimLeft(line, " \t"), "//") {
				continue
			}
			scan.classify(line, appPathStdBridgeSite{relPath: relPath, line: idx + 1, text: strings.TrimSpace(line)}, testFile)
		}
		return nil
	})
	return scan, scanned, err
}

// failures lists every way the scan breaks its budgets or its rules, and the text budgets'
// total (the tripwire's count).
func (scan *appPathScan) failures(allowlist appPathStdBridgeAllowlist) ([]string, int) {
	var failures []string
	if over := overBudget(scan.bridge, siteCounts(scan.bridge), allowlist.Files); len(over) > 0 {
		failures = append(failures, fmt.Sprintf(
			"found %d app-path bridge %s over the file's budget in %s (speak `AppPath` arithmetic instead: "+
				"`join`, `parent`, `strip_prefix`, and the native crossings `to_native` / `from_native`; "+
				"the bridge only shrinks, crates/cmdr-fs/DETAILS.md § \"App paths\"):\n%s",
			len(over), Pluralize(len(over), "call", "calls"), appPathStdBridgeAllowlistRelPath, formatSites(over)))
	}
	if over := overBudget(scan.wrapperCalls, siteCounts(scan.wrapperCalls), allowlist.WrapperCalls); len(over) > 0 {
		failures = append(failures, fmt.Sprintf(
			"found %d %s through a named bridge wrapper (`%s`) over the file's budget in %s: a wrapper call "+
				"crosses `std::path` like a direct bridge call, so speak `AppPath` arithmetic instead "+
				"(crates/cmdr-fs/DETAILS.md § \"App paths\"):\n%s",
			len(over), Pluralize(len(over), "call", "calls"), strings.Join(appPathBridgeWrappers, "`, `"),
			appPathStdBridgeAllowlistRelPath, formatSites(over)))
	}
	if over := overBudget(scan.stdJoins, siteCounts(scan.stdJoins), allowlist.StdJoin); len(over) > 0 {
		failures = append(failures, formatStdJoinFailure(over))
	}
	if len(scan.joinOrphans) > 0 {
		failures = append(failures, formatOrphanDirectives(stdJoinDirective, scan.joinOrphans))
	}
	if over := overBudget(scan.textToPath, siteCounts(scan.textToPath), allowlist.AppTextToPath); len(over) > 0 {
		failures = append(failures, fmt.Sprintf(
			"found %d %s turning app text into a std path, over the file's budget in %s (a `FileEntry.path` "+
				"or `.name` is app text: build an `AppPath` with `AppPath::from_normalized`, a name with "+
				"`AppName::from_app_spelling`, and cross with `to_native`; crates/cmdr-fs/DETAILS.md § \"App paths\"):\n%s",
			len(over), Pluralize(len(over), "site", "sites"), appPathStdBridgeAllowlistRelPath, formatSites(over)))
	}
	if over := overBudget(scan.bareText, siteCounts(scan.bareText), allowlist.PathFromBareText); len(over) > 0 {
		failures = append(failures, fmt.Sprintf(
			"found %d %s handing path text to a std path constructor, over the file's budget in %s (if it's "+
				"app text, take it as an `AppPathBuf` with `from_wire` at the edge and cross with `to_native`; "+
				"if it's a native path, keep it a `PathBuf` from where the OS handed it over; "+
				"crates/cmdr-fs/DETAILS.md § \"App paths\"):\n%s",
			len(over), Pluralize(len(over), "site", "sites"), appPathStdBridgeAllowlistRelPath, formatSites(over)))
	}
	textTotal := 0
	for _, n := range allowlist.AppTextToPath {
		textTotal += n
	}
	for _, n := range allowlist.PathFromBareText {
		textTotal += n
	}
	if len(scan.producers) > 0 && textTotal > 0 {
		failures = append(failures, fmt.Sprintf(
			"%d escape-producing %s in production code while %d text-to-std-path %s remain: an escape "+
				"would reach code that opens a file literally named with it. Until milestone 10f, cross with "+
				"`from_native_unicode` (a typed refusal where an escape would be needed); the producers land "+
				"after the type-level ban on the text exits (docs/specs/app-path-plan.md § \"Keeping it that way\"):\n%s",
			len(scan.producers), Pluralize(len(scan.producers), "call", "calls"),
			textTotal, Pluralize(textTotal, "site", "sites"), formatSites(scan.producers)))
	}
	if len(scan.osTextHops) > 0 {
		failures = append(failures, fmt.Sprintf(
			"%d %s hand OS text (`to_str()`, `to_string_lossy()`, `.display()`) to a door that takes APP spelling: "+
				"a native name holding an escape-range code point as text would cross `to_native` as a different "+
				"file. Cross a native name with `AppNameBuf::from_native_os` and a native path with "+
				"`AppPathBuf::from_native` (crates/cmdr-fs/DETAILS.md § \"App paths\"):\n%s",
			len(scan.osTextHops), Pluralize(len(scan.osTextHops), "site", "sites"), formatSites(scan.osTextHops)))
	}
	return failures, textTotal
}
