package main

import (
	"fmt"
	"regexp"
	"strings"
)

// edit replaces [start, end) of file with text; a zero-width edit inserts.
type edit struct {
	file  string
	start int
	end   int
	text  string
	rule  string
}

// rewriter turns one round's diagnostics into edits. It reads each file once per round.
type rewriter struct {
	load      func(file string) (*source, error)
	crate     string
	crateRust []string // every .rs file of the crate, for finding a struct a field rule retypes
	// every .rs file under `crates/`, for finding a helper another crate defines
	// (`scan_walk::scan_one`), and a cheap byte read of one before it's parsed
	workspaceRust []string
	readRaw       func(file string) ([]byte, error)
	lazyMemo      map[string]bool // `lazyFn`'s answers this round
}

// editsFor returns the edits for one diagnostic, or none when no rule applies (residue).
func (r *rewriter) editsFor(d *diagnostic) ([]edit, error) {
	p := d.primary()
	src, err := r.load(p.FileName)
	if err != nil {
		return nil, err
	}
	if p.ByteEnd > len(src.text) {
		return nil, nil
	}
	if d.Level == "warning" || d.code() == "unused_imports" {
		return unusedImportEdits(d, p.FileName), nil
	}
	if d.Level != "error" {
		return nil, nil
	}
	if p.Macro != "" {
		return compareEdits(d, src, p), nil
	}
	if e := compareEdits(d, src, p); len(e) > 0 {
		return e, nil
	}
	if e := importEdits(d, src, r.crate); len(e) > 0 {
		return e, nil
	}
	if d.code() == "E0053" {
		return signatureEdits(src, p), nil
	}
	if e := r.borrowedFutureEdits(d, src, p); len(e) > 0 {
		return e, nil
	}
	from, to := region(d, src, p)
	if e := preludeEdits(src, p.ByteStart, from, to); len(e) > 0 {
		return e, nil
	}
	if e := receiverEdits(d, src, p); len(e) > 0 {
		return e, nil
	}
	return r.mismatchEditsFor(d, src, p), nil
}

// mismatchEditsFor answers an E0308: the `from_ref` row, else each labeled value's rewrite.
func (r *rewriter) mismatchEditsFor(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0308" {
		return nil
	}
	if e := fromRefEdits(d, src, p); len(e) > 0 {
		return e
	}
	var edits []edit
	for _, s := range mismatchSpans(d, p.FileName) {
		edits = append(edits, r.valueEdits(src, s)...)
	}
	return edits
}

// valueEdits rewrites one mismatched value: a typed param handed back, a borrowed return,
// or a row of the type-pair table.
func (r *rewriter) valueEdits(src *source, s *diagSpan) []edit {
	expected, found, ok := mismatch(s.label())
	if !ok {
		return nil
	}
	if e := typedParamBackEdits(src, s, expected); len(e) > 0 {
		return e
	}
	if e := r.borrowedReturnEdits(src, s, expected); len(e) > 0 {
		return e
	}
	return mismatchEdits(src, s, expected, found)
}

// mismatchSpans are the spans an E0308 labels `expected X, found Y`: the primary one, or,
// when several arguments of one call mismatch, each labeled argument.
func mismatchSpans(d *diagnostic, file string) []*diagSpan {
	var spans []*diagSpan
	for i := range d.Spans {
		s := &d.Spans[i]
		if _, _, ok := mismatch(s.label()); ok && s.FileName == file {
			spans = append(spans, s)
		}
	}
	return spans
}

// region is the source a diagnostic is about: its labeled spans, plus a method call's
// receiver (outside every span rustc points at; `path.display()` names the param only there).
func region(d *diagnostic, src *source, p *diagSpan) (int, int) {
	from, to := p.ByteStart, p.ByteEnd
	for _, s := range mismatchSpans(d, p.FileName) {
		from, to = min(from, s.ByteStart), max(to, s.ByteEnd)
	}
	if dot, _, ok := methodCallAt(d, src, p); ok {
		from = min(from, receiverStart(src.masked, dot))
	}
	// A comparison (`path == dir`, E0277 "can't compare", E0369) points at its operator:
	// the operands are on its line.
	if d.code() == "E0277" || d.code() == "E0369" {
		for from > 0 && src.masked[from-1] != '\n' {
			from--
		}
		for to < len(src.masked) && src.masked[to] != '\n' {
			to++
		}
	}
	return from, min(to, len(src.masked))
}

// ---- E0053: a Volume impl method's signature ----

var (
	stdPathBufWord = regexp.MustCompile(`\b(?:std::path::)?PathBuf\b`)
	stdPathWord    = regexp.MustCompile(`\b(?:std::path::)?Path\b`)
	// A macro body spells std fully qualified (`&'a ::std::path::Path`) and needs the typed
	// name spelled the same way: a bare `::AppPath` names a crate (E0425).
	qualifiedStdPath = regexp.MustCompile(`::std::path::(Path|PathBuf)\b`)
)

// retypeStd spells a std path type as its app-path twin, a fully qualified one fully qualified.
func retypeStd(text string) string {
	retyped := qualifiedStdPath.ReplaceAllString(text, "::cmdr_fs::app_path::App${1}")
	retyped = stdPathBufWord.ReplaceAllString(retyped, "AppPathBuf")
	return stdPathWord.ReplaceAllString(retyped, "AppPath")
}

// signatureEdits retypes the param or return type rustc points at to the trait's.
func signatureEdits(src *source, p *diagSpan) []edit {
	expected, found, ok := mismatch(p.label())
	if !ok || !strings.Contains(expected, "AppPath") || strings.Contains(found, "AppPath") {
		return nil
	}
	text := string(src.text[p.ByteStart:p.ByteEnd])
	retyped := retypeStd(text)
	if retyped == text {
		return nil
	}
	return []edit{{file: p.FileName, start: p.ByteStart, end: p.ByteEnd, text: retyped, rule: ruleSignature}}
}

// ---- The body prelude ----

// preludeEdits answers an error inside a flipped method's body that names one of its typed
// params: the body gets `let P_std = P.as_std_legacy(); let P: &Path = &P_std;`, so its own
// `std::path` arithmetic reads exactly what it read before.
func preludeEdits(src *source, at, from, to int) []edit {
	m := src.methodAt(at)
	if m == nil {
		return nil
	}
	spanText := src.masked[from:to]
	for _, prm := range m.params {
		kind := typedParamKind(prm.typ)
		if kind == "" || !containsIdent(spanText, prm.name) || hasPrelude(src, m, prm.name) {
			continue
		}
		return preludeFor(src, m, prm.name, kind)
	}
	return nil
}

// typedParamKind is "one" for `&AppPath`, "batch" for `&[AppPathBuf]`, "" otherwise.
func typedParamKind(typ string) string {
	switch normalizeType(typ) {
	case "&AppPath":
		return "one"
	case "&[AppPathBuf]":
		return "batch"
	}
	return ""
}

func preludeText(name, kind string) string {
	if kind == "batch" {
		return fmt.Sprintf("\nlet %[1]s_std: Vec<PathBuf> = %[1]s.iter().map(|p| p.as_std_legacy().into_owned()).collect();\nlet %[1]s: &[PathBuf] = &%[1]s_std;", name)
	}
	return fmt.Sprintf("\nlet %[1]s_std = %[1]s.as_std_legacy();\nlet %[1]s: &Path = &%[1]s_std;", name)
}

func hasPrelude(src *source, m *implMethod, name string) bool {
	body := string(src.text[m.bodyStart:m.bodyEnd])
	return strings.Contains(body, "let "+name+"_std")
}

var asyncMovePattern = regexp.MustCompile(`\basync\s+move\s*\{`)

// preludeFor places the prelude: at the start of each outermost `async move` block that
// names the param when EVERY use is inside one (a future must own what it borrows, and the
// param is a `Copy` reference it captures), else at the start of the body.
func preludeFor(src *source, m *implMethod, name, kind string) []edit {
	uses := identUses(src.masked, m.bodyStart, m.bodyEnd, name)
	blocks := outermostAsyncBlocks(src.masked, m.bodyStart, m.bodyEnd)
	var inside []int
	for _, use := range uses {
		holder := -1
		for i, b := range blocks {
			if use > b[0] && use < b[1] {
				holder = i
			}
		}
		if holder < 0 {
			return []edit{{file: src.path, start: m.bodyStart, end: m.bodyStart, text: preludeText(name, kind), rule: rulePrelude}}
		}
		if len(inside) == 0 || inside[len(inside)-1] != holder {
			inside = append(inside, holder)
		}
	}
	var edits []edit
	for _, i := range inside {
		at := blocks[i][0] + 1
		edits = append(edits, edit{file: src.path, start: at, end: at, text: preludeText(name, kind), rule: rulePrelude})
	}
	return edits
}

// outermostAsyncBlocks lists each `async move {` block's `{`/`}` offsets, outermost only.
func outermostAsyncBlocks(masked []byte, from, to int) [][2]int {
	blocks := [][2]int{}
	for _, loc := range asyncMovePattern.FindAllIndex(masked[from:to], -1) {
		open := from + loc[1] - 1
		if len(blocks) > 0 && open < blocks[len(blocks)-1][1] {
			continue
		}
		close := matchBrace(masked, open)
		if close < 0 || close > to {
			continue
		}
		blocks = append(blocks, [2]int{open, close})
	}
	return blocks
}

// identUses lists the offsets in [from, to) where name is used as a value: a whole word, not
// a field (`.name`), not a path segment (`name::`), not a struct field label (`name: x`).
func identUses(masked []byte, from, to int, name string) []int {
	var uses []int
	for k := from; k+len(name) <= to; k++ {
		if string(masked[k:k+len(name)]) != name {
			continue
		}
		if (k > 0 && (isIdentByte(masked[k-1]) || masked[k-1] == '.')) || (k+len(name) < len(masked) && isIdentByte(masked[k+len(name)])) {
			continue
		}
		after := strings.TrimLeft(string(masked[k+len(name):min(to, k+len(name)+3)]), " ")
		if strings.HasPrefix(after, "::") || (strings.HasPrefix(after, ":") && !strings.HasPrefix(after, "::")) {
			continue
		}
		uses = append(uses, k)
	}
	return uses
}

func containsIdent(text []byte, name string) bool {
	return len(identUses(text, 0, len(text), name)) > 0
}

// ---- Imports ----

var notFoundName = regexp.MustCompile("(?:cannot find (?:type|macro|struct|value) `|use of undeclared type `|failed to resolve: use of undeclared type `)(\\w+)`")

// The import each name a rule introduces gets, `{crate}` being `crate` inside `cmdr-fs` and
// `cmdr_fs` everywhere else.
var wantedImports = map[string]string{
	"AppPath":     "use {crate}::app_path::AppPath;\n",
	"AppPathBuf":  "use {crate}::app_path::AppPathBuf;\n",
	"BridgedPath": "use {crate}::app_path::BridgedPath;\n",
	"app_path":    "use {crate}::app_path;\n",
	"Path":        "use std::path::Path;\n",
	"PathBuf":     "use std::path::PathBuf;\n",
}

// importEdits imports a name a rule introduced, at the point rustc's own "consider
// importing" suggestion picks (the right module, after its other `use`s). The text is ours:
// rustc may suggest a re-export (`crate::volume::AppPath`) where the canonical path is wanted.
func importEdits(d *diagnostic, src *source, crate string) []edit {
	file := src.path
	m := notFoundName.FindStringSubmatch(d.Message)
	if m == nil {
		return nil
	}
	want, ok := wantedImports[m[1]]
	if !ok {
		return nil
	}
	root := "cmdr_fs"
	if crate == "cmdr-fs" {
		root = "crate"
	}
	want = strings.ReplaceAll(want, "{crate}", root)
	for _, child := range d.Children {
		for _, s := range child.Spans {
			if s.SuggestedReplacement == nil || s.FileName != file || !strings.HasPrefix(strings.TrimSpace(*s.SuggestedReplacement), "use ") {
				continue
			}
			start, end := s.ByteStart, s.ByteEnd
			if start == end {
				start = aboveOuterAttributes(src, start)
				end = start
			}
			return []edit{{file: file, start: start, end: end, text: want, rule: ruleImport}}
		}
	}
	// rustc suggests nothing for a name from another crate here; the import goes at the head
	// of the innermost module holding the use that doesn't already import it.
	if at, ok := importPoint(src, d.primary().ByteStart, strings.TrimSpace(want)); ok {
		at = aboveOuterAttributes(src, at)
		return []edit{{file: file, start: at, end: at, text: want, rule: ruleImport}}
	}
	return nil
}

// aboveOuterAttributes moves a line-start insertion point above the outer attributes and
// doc comments of the item it precedes (`#[cfg(test)]` over a `pub use`, a multi-line
// `#[cfg(any(…))]`, `///` lines): inserted below them, the import takes the item's
// attribute and the item loses it, a silent change of what compiles.
func aboveOuterAttributes(src *source, at int) int {
	for at > 0 && src.masked[at-1] == '\n' {
		lineStart := at - 1
		for lineStart > 0 && src.masked[lineStart-1] != '\n' {
			lineStart--
		}
		line := strings.TrimSpace(string(src.text[lineStart : at-1]))
		maskedLine := strings.TrimRight(string(src.masked[lineStart:at-1]), " \t\r")
		switch {
		case strings.HasPrefix(line, "///") || strings.HasPrefix(line, "/**"):
			at = lineStart
		case strings.HasSuffix(maskedLine, "]"):
			open := matchBackward(src.masked, lineStart+len(maskedLine)-1)
			if open < 1 || src.masked[open-1] != '#' {
				return at
			}
			attrLine := open - 1
			for attrLine > 0 && src.masked[attrLine-1] != '\n' {
				attrLine--
			}
			if strings.TrimSpace(string(src.masked[attrLine:open-1])) != "" {
				return at // `#` mid-line: not an attribute on its own lines
			}
			at = attrLine
		default:
			return at
		}
	}
	return at
}

var inlineModPattern = regexp.MustCompile(`\bmod\s+\w+\s*\{`)

// importPoint is where a `use` line goes for a name used at offset: before the first `use`
// of the innermost inline `mod … {` around it (or of the file), after its inner docs and
// attributes when it has none. A scope already holding the line gets nothing (the name is
// out of its reach some other way: residue), so a re-run never stacks duplicates.
func importPoint(src *source, offset int, line string) (int, bool) {
	from, to := 0, len(src.masked)
	for _, loc := range inlineModPattern.FindAllIndex(src.masked, -1) {
		open := loc[1] - 1
		close := matchBrace(src.masked, open)
		if close > offset && open < offset && open+1 > from {
			from, to = open+1, close
		}
	}
	if strings.Contains(string(src.text[from:to]), line) {
		return 0, false
	}
	return from + headInsertOffset(src.masked[from:to]), true
}

var firstUsePattern = regexp.MustCompile(`(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?use\s`)

// headInsertOffset is the offset in a module body of its first `use`, or past its leading
// blank lines, inner docs (masked to blanks), and `#![…]` attributes.
func headInsertOffset(masked []byte) int {
	if loc := firstUsePattern.FindIndex(masked); loc != nil {
		// Only a `use` at the module's own level: nothing but items and blanks before it.
		return loc[0]
	}
	k := 0
	for k < len(masked) {
		lineEnd := indexByteFrom(masked, k, '\n')
		if lineEnd < 0 {
			lineEnd = len(masked)
		}
		trimmed := strings.TrimSpace(string(masked[k:lineEnd]))
		if trimmed != "" && !strings.HasPrefix(trimmed, "#![") {
			return k
		}
		k = lineEnd + 1
	}
	return len(masked)
}

var pathImportNames = regexp.MustCompile("^unused imports?: (`(std::path::)?(Path|PathBuf|self)`(, | and )?)+$")

// unusedImportEdits drops a `Path`/`PathBuf` import the rewrites left unused, through rustc's
// machine-applicable suggestion. Nothing else: the base compiled warning-free.
func unusedImportEdits(d *diagnostic, file string) []edit {
	if !pathImportNames.MatchString(d.Message) {
		return nil
	}
	var edits []edit
	for _, child := range d.Children {
		for _, s := range child.Spans {
			if s.SuggestedReplacement == nil || s.SuggestionApplicability == nil || *s.SuggestionApplicability != "MachineApplicable" || s.FileName != file {
				continue
			}
			edits = append(edits, edit{file: file, start: s.ByteStart, end: s.ByteEnd, text: *s.SuggestedReplacement, rule: ruleUnusedImport})
		}
	}
	return edits
}
