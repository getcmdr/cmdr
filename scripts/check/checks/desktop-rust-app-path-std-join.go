package checks

// The `app-path-std-bridge` check's two newer budgets: calls through the bridge's named
// wrappers (`wrapperCalls`, #1469) and `std::path` joins of a name onto a folder in the write
// engine's production code (`stdJoin`). The check's run and the other budgets live in
// desktop-rust-app-path-std-bridge.go.

import (
	"fmt"
	"regexp"
	"strings"
	"unicode"
)

// appPathBridgeWrappers are the named wrappers over the bridge (#1469): a call through one
// crosses `std::path` as surely as a direct `legacy_unchecked` / `as_std_legacy`, so the
// `wrapperCalls` budget counts each call site. Where each is defined:
//   - `std_of`, `into_std`, `on_volume`: crates/cmdr-operations/src/transfer/volume/listed_name.rs
//     (plus private `on_volume` copies in cmdr-operations' `hidden_links.rs`, `source_binding.rs`,
//     `conflict_preflight.rs`, and `in_flight_sweep_surface.rs`).
//   - `seam_path`, `engine_path`: crates/cmdr-operations/src/wire_paths.rs.
//   - a private `std_of` over `as_std_legacy` in cmdr-fs' `volume/in_memory.rs` and `volume/patching.rs`.
//
// ❗ `in_flight_sweep.rs` also has an unrelated private `async fn on_volume(ledger, ...)`; its
// two calls count too, since no text scan can tell the two apart.
//
// Not wrappers: `label_of`, `native_text`, and `own_name` (`listed_name.rs`) read app text and
// hand back text, no `std::path` (`own_name`'s root case crosses through `std_of`, counted at
// its definition); `as_std_legacy` is the bridge itself, in the `files` budget.
var appPathBridgeWrappers = []string{"std_of", "into_std", "on_volume", "seam_path", "engine_path"}

// appPathBridgeWrapperCrates are the crates defining the wrappers. Every wrapper is
// `pub(crate)` or private, so a call can only sit in its own crate, and `on_volume` is a
// common name elsewhere (the desktop's `commands::wire_path::on_volume` turns wire TEXT
// into an app path, with no `std::path` in it; `tools/connect.rs` has its own).
var appPathBridgeWrapperCrates = []string{"crates/cmdr-operations/", "crates/cmdr-fs/"}

func inWrapperScope(relPath string) bool {
	for _, root := range appPathBridgeWrapperCrates {
		if strings.HasPrefix(relPath, root) {
			return true
		}
	}
	return false
}

var appPathBridgeWrapperPattern = gateRegexp(regexp.MustCompile(
	`\b(` + strings.Join(appPathBridgeWrappers, "|") + `)\s*\(`))

// wrapperFnDefinition and wrapperTypeQualifier tell a wrapper's own definition and an
// unrelated associated fn (`ListingPath::on_volume`, `AppHost::on_volume`, `Self::on_volume`)
// from a call of the free wrapper; a module-qualified call (`listed_name::on_volume(`) counts.
var (
	wrapperFnDefinition  = regexp.MustCompile(`\bfn$`)
	wrapperTypeQualifier = regexp.MustCompile(`(\b[A-Z]\w*|>)\s*::$`)
)

// wrapperCallCount counts the calls of a bridge wrapper on one line: not a method call
// (`cmdr-viewer`'s `DocumentImages::on_volume` is called as `.on_volume(`), not a
// definition, not a type's associated fn, and only in a crate defining the wrappers.
func wrapperCallCount(line, relPath string) int {
	if !inWrapperScope(relPath) {
		return 0
	}
	n := 0
	for _, m := range appPathBridgeWrapperPattern.FindAllStringSubmatchIndex(line, -1) {
		prefix := strings.TrimRight(line[:m[0]], " \t")
		if strings.HasSuffix(prefix, ".") || wrapperFnDefinition.MatchString(prefix) || wrapperTypeQualifier.MatchString(prefix) {
			continue
		}
		n++
	}
	return n
}

// stdJoinRoots are where a `std::path` join of a name onto a folder is budgeted: the write
// engine and its desktop host, the code that hands paths to POSIX volumes. On Windows
// `Path::new("/share").join(name)` gives `/share\name`, which SFTP, SMB, MTP, or the in-memory
// double reads as ONE name in `/`.
var stdJoinRoots = []string{"crates/cmdr-operations/src/", "apps/desktop/src-tauri/src/operations_host/"}

// stdJoinDirective opts out a genuinely native local join (a tempdir, the in-flight ledger,
// the local engine's own disk), on the join's line or the line above.
const stdJoinDirective = "// allowed-std-join:"

// stdJoinTypedSuffixes follow a TYPED join: `AppPath::join` and `ListedFolder::join` return a
// `Result`, and `Path::join` doesn't.
var stdJoinTypedSuffixes = []string{"?", ".ok()", ".map_err(", ".expect(", ".unwrap", ".is_ok()", ".is_err()"}

// stdJoinCallFormPattern is the path form of the join (`Path::join(&dir, name)`), which
// `.join(` doesn't see.
var stdJoinCallFormPattern = regexp.MustCompile(`\b(Path|PathBuf)::join\s*\(`)

// stdJoinPushPattern is a `PathBuf::push` of a name onto a path-shaped binding (`dir.push(`,
// `dest_path.push(`). A text scan can't tell a `Vec`'s `push` from a `PathBuf`'s, so it asks
// for a path-shaped receiver name (`*_path`, `*_dir`, `dir`, `dest`, …) and an argument that
// isn't a tuple: a heuristic, ❗ a `PathBuf` named otherwise (`path`, `p`) passes.
var stdJoinPushPattern = regexp.MustCompile(`\b(\w*_(path|dir|root|folder)|dir|root|folder|dest|destination|parent|target)\.push\(\s*([^(\s])`)

// pathPushCount counts [stdJoinPushPattern]'s pushes on a line, a plural receiver (`paths`,
// `dirs`: a collection) excepted.
func pathPushCount(line string) int {
	n := 0
	for _, m := range stdJoinPushPattern.FindAllStringSubmatch(line, -1) {
		if !strings.HasSuffix(m[1], "s") {
			n++
		}
	}
	return n
}

// stdJoinMaxLookahead bounds how many following lines a wrapped join's argument may span.
const stdJoinMaxLookahead = 30

func inStdJoinScope(relPath string) bool {
	for _, root := range stdJoinRoots {
		if strings.HasPrefix(relPath, root) {
			return true
		}
	}
	return false
}

// stdJoinTestGate matches a test-only cfg: `#[cfg(test)]` or `#[cfg(all(test, ...))]`. It
// gates only the ONE item after it (a `use`, a `fn`, a `mod tests { … }`), ❌ never the rest of
// the file: a `#[cfg(test)]` on one helper used to hide every production join below it.
var stdJoinTestGate = regexp.MustCompile(`^#\[cfg\((all\(\s*)?test\b`)

// testItemEnd returns the index of the last line of the item a test cfg at lines[gate] gates:
// past its other attributes, to the `;` that ends a one-line item or the `}` that closes its
// body (braces outside string literals and comments).
func testItemEnd(lines []string, gate int) int {
	depth, opened := 0, false
	for i := gate + 1; i < len(lines); i++ {
		code := strings.TrimSpace(stripLineComment(lines[i]))
		if !opened && (code == "" || strings.HasPrefix(code, "#[")) {
			continue
		}
		inString := false
		for j := 0; j < len(code); j++ {
			c := code[j]
			switch {
			case inString:
				if c == '\\' {
					j++
				} else if c == '"' {
					inString = false
				}
			case c == '"':
				inString = true
			case c == '{':
				depth++
				opened = true
			case c == '}':
				depth--
			}
		}
		if opened && depth <= 0 {
			return i
		}
		if !opened && strings.HasSuffix(code, ";") {
			return i
		}
	}
	return len(lines) - 1
}

// scanStdJoins returns one site per untyped `std::path` join on a production line of a file
// (every line outside a test-gated item), and the file's unused opt-out directives.
func scanStdJoins(relPath string, lines []string) ([]appPathStdBridgeSite, []orphanDirective) {
	tracker := newDirectiveTracker(stdJoinDirective, "//")
	var sites []appPathStdBridgeSite
	for idx := 0; idx < len(lines); idx++ {
		line := lines[idx]
		trimmed := strings.TrimSpace(line)
		if stdJoinTestGate.MatchString(trimmed) {
			idx = testItemEnd(lines, idx)
			continue
		}
		tracker.observe(idx+1, line)
		if strings.HasPrefix(trimmed, "//") {
			continue
		}
		n := stdJoinCallCount(lines, idx)
		if n == 0 {
			continue
		}
		prev := ""
		if idx > 0 {
			prev = lines[idx-1]
		}
		if strings.Contains(line, stdJoinDirective) || strings.Contains(prev, stdJoinDirective) {
			tracker.markUsed(idx+1, line, prev)
			continue
		}
		for i := 0; i < n; i++ {
			sites = append(sites, appPathStdBridgeSite{relPath: relPath, line: idx + 1, text: trimmed})
		}
	}
	return sites, tracker.orphans(relPath)
}

// stdJoinCallCount counts the untyped joins starting on lines[idx]: each `with_file_name(`,
// and each `.join(` whose argument isn't empty, isn't a separator literal (a slice's string
// join), and isn't followed by a `Result` handler (a typed join). The argument may wrap onto
// the following lines, as rustfmt splits it.
func stdJoinCallCount(lines []string, idx int) int {
	line := stripLineComment(lines[idx])
	n := strings.Count(line, "with_file_name(") + strings.Count(line, "set_file_name(")
	n += len(stdJoinCallFormPattern.FindAllStringIndex(line, -1)) + pathPushCount(line)
	end := min(idx+1+stdJoinMaxLookahead, len(lines))
	rest := "\n" + strings.Join(lines[idx+1:end], "\n")
	for offset := 0; ; {
		at := strings.Index(line[offset:], ".join(")
		if at < 0 {
			break
		}
		open := offset + at + len(".join")
		offset = open + 1
		text := line[open:] + rest
		closeAt := matchingParen(text)
		if closeAt < 0 {
			n++
			continue
		}
		arg := strings.TrimSpace(text[1:closeAt])
		if arg == "" || isSeparatorLiteral(arg) || followsTypedJoin(text[closeAt+1:]) {
			continue
		}
		n++
	}
	return n
}

// matchingParen returns the index of the `)` closing the `(` at text[0], skipping string
// literals, or -1.
func matchingParen(text string) int {
	depth := 0
	inString := false
	for i := 0; i < len(text); i++ {
		c := text[i]
		switch {
		case inString:
			if c == '\\' {
				i++
			} else if c == '"' {
				inString = false
			}
		case c == '"':
			inString = true
		case c == '(':
			depth++
		case c == ')':
			depth--
			if depth == 0 {
				return i
			}
		}
	}
	return -1
}

// isSeparatorLiteral reports a string or char literal holding no letter or digit: the
// separator of a slice's string join (`.join("/")`, `.join(", ")`).
func isSeparatorLiteral(arg string) bool {
	if len(arg) < 2 {
		return false
	}
	quote := arg[0]
	if (quote != '"' && quote != '\'') || arg[len(arg)-1] != quote {
		return false
	}
	for _, r := range arg[1 : len(arg)-1] {
		if unicode.IsLetter(r) || unicode.IsDigit(r) {
			return false
		}
	}
	return true
}

func followsTypedJoin(rest string) bool {
	rest = strings.TrimLeft(rest, " \t\r\n")
	for _, suffix := range stdJoinTypedSuffixes {
		if strings.HasPrefix(rest, suffix) {
			return true
		}
	}
	return false
}

// stripLineComment drops a trailing `//` comment outside a string literal.
func stripLineComment(line string) string {
	inString := false
	for i := 0; i < len(line); i++ {
		c := line[i]
		switch {
		case inString:
			if c == '\\' {
				i++
			} else if c == '"' {
				inString = false
			}
		case c == '"':
			inString = true
		case c == '/' && i+1 < len(line) && line[i+1] == '/':
			return line[:i]
		}
	}
	return line
}

func formatStdJoinFailure(over []appPathStdBridgeSite) string {
	return fmt.Sprintf(
		"found %d `std::path` %s of a name onto a folder over the file's budget in %s (on Windows "+
			"`Path::new(\"/share\").join(name)` gives `/share\\name`, which a POSIX volume reads as ONE name in `/`). "+
			"Join a new name under a volume folder through `wire_paths::seam_child`, a plain name through "+
			"`unique_name::sibling_in`, and a listed name through `transfer/volume/listed_name.rs` (`ListedFolder`); "+
			"a genuinely native local path (a tempdir, the in-flight ledger, the local engine's own disk) keeps "+
			"`Path::join` with `%s <reason>` or its budget:\n%s",
		len(over), Pluralize(len(over), "join", "joins"), appPathStdBridgeAllowlistRelPath, stdJoinDirective, formatSites(over))
}
