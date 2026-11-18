package main

import (
	"bytes"
	"regexp"
	"strings"
)

// ---- A round trip split across a prelude ----

var (
	bridgedNameOne   = regexp.MustCompile(`&\s*AppPath::legacy_unchecked\(\s*(\w+)\s*\)`)
	bridgedNameBatch = regexp.MustCompile(`&\s*(\w+)\s*\.iter\(\)\s*\.map\(\s*\|(\w+)\|\s*AppPath::legacy_unchecked\(\s*(\w+)\s*\)\s*\.into_owned\(\)\s*\)\s*\.collect::<Vec<_>>\(\)`)
)

// splitRoundTripEdits answers E0515 on a typed param bridged back after a prelude shadowed
// it (`let path_std = path.as_std_legacy(); let path: &Path = &path_std;` …
// `self.get_metadata(&AppPath::legacy_unchecked(path))`: an earlier pass's bridge meeting a
// later pass's prelude). The bridged value is the typed param itself (the bridge is
// lossless), so the call gets the param: when nothing else reads the shadowing `path`, the
// prelude goes and the call says `path`; otherwise the param is kept as `path_app` before the
// prelude, as the typed-param-handed-back rule does.
func splitRoundTripEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0515" {
		return nil
	}
	fn := src.fnAt(p.ByteStart)
	if fn == nil {
		return nil
	}
	hits := bridgedHits(string(src.masked[p.ByteStart:p.ByteEnd]), p.ByteStart)
	if len(hits) == 0 {
		return nil
	}
	name := hits[0].name
	pre := preludeSpan(src, fn, name)
	if !hasTypedParam(fn, name) || pre == nil {
		return nil
	}
	for _, h := range hits {
		if h.name != name || h.start < pre[1] {
			return nil // another name, or a use the prelude doesn't shadow: not this shape
		}
	}
	replacement := name
	var edits []edit
	if usesOutside(src, name, pre[1], fn.bodyEnd, hits) == 0 {
		edits = append(edits, edit{file: src.path, start: pre[0], end: pre[1], text: "", rule: ruleRoundTrip})
	} else {
		replacement = name + "_app"
		keep := "let " + name + "_app = " + name + ";"
		if !strings.HasSuffix(strings.TrimRight(string(src.text[fn.bodyStart:pre[0]]), " \t\n"), keep) {
			edits = append(edits, edit{file: src.path, start: pre[0], end: pre[0], text: "\n" + keep, rule: ruleRoundTrip})
		}
	}
	for _, h := range hits {
		edits = append(edits, edit{file: src.path, start: h.start, end: h.end, text: replacement, rule: ruleRoundTrip})
	}
	return edits
}

type bridgedHit struct {
	start, end int
	name       string
}

// bridgedHits are the `&AppPath::legacy_unchecked(name)` (and batch-map) sites in region,
// which starts at offset base of the file.
func bridgedHits(region string, base int) []bridgedHit {
	var hits []bridgedHit
	for _, loc := range bridgedNameBatch.FindAllStringSubmatchIndex(region, -1) {
		if region[loc[4]:loc[5]] == region[loc[6]:loc[7]] {
			hits = append(hits, bridgedHit{base + loc[0], base + loc[1], region[loc[2]:loc[3]]})
		}
	}
	for _, loc := range bridgedNameOne.FindAllStringSubmatchIndex(region, -1) {
		hits = append(hits, bridgedHit{base + loc[0], base + loc[1], region[loc[2]:loc[3]]})
	}
	return hits
}

func hasTypedParam(fn *implMethod, name string) bool {
	for _, prm := range fn.params {
		if prm.name == name && typedParamKind(prm.typ) != "" {
			return true
		}
	}
	return false
}

// usesOutside counts name's uses in [from, to) that aren't inside one of the hits.
func usesOutside(src *source, name string, from, to int, hits []bridgedHit) int {
	n := 0
	for _, use := range identUses(src.masked, from, to, name) {
		inHit := false
		for _, h := range hits {
			inHit = inHit || (use >= h.start && use < h.end)
		}
		if !inHit {
			n++
		}
	}
	return n
}

// preludeSpan is the [start, end) of name's prelude in fn's body (its two `let`s), or nil.
func preludeSpan(src *source, fn *implMethod, name string) []int {
	q := regexp.QuoteMeta(name)
	pattern := regexp.MustCompile(`\s*let ` + q + `_std(?:: Vec<PathBuf>)? = [^;]*;\s*let ` + q + `: &(?:\[PathBuf\]|Path) = &` + q + `_std;`)
	loc := pattern.FindIndex(src.masked[fn.bodyStart:fn.bodyEnd])
	if loc == nil {
		return nil
	}
	return []int{fn.bodyStart + loc[0], fn.bodyStart + loc[1]}
}

// ---- E0716: a bridge temporary a held future outlives ----

var bridgeTemporary = regexp.MustCompile(`^AppPath::legacy_unchecked\(\s*(&?\s*[\w.]+)\s*\)$`)

// hoistEdits answers E0716 on `AppPath::legacy_unchecked(&dir)` inside a statement whose
// result outlives it (`let fut = volume.list_directory(&AppPath::legacy_unchecked(&dir),
// None);`, `tokio::join!(…)`): the bridge moves to `let dir_app = AppPath::legacy_unchecked(&dir);`
// just before the statement, and the argument names it. Only a plain argument (`plainArg`)
// moves, so nothing in the statement runs in a different order; a name the fn already uses
// stays residue.
func hoistEdits(src *source, p *diagSpan) []edit {
	return hoistFrom(src, p, p.ByteStart, nil)
}

// hoistFrom hoists the bridge temporary spanned by p to before the statement holding from,
// unless its argument's root is one of the names in bound (a closure's own params).
func hoistFrom(src *source, p *diagSpan, from int, bound []string) []edit {
	// The span is the call (the `Cow` temporary), or the borrow of it.
	expr := strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(string(src.text[p.ByteStart:p.ByteEnd])), "&"))
	m := bridgeTemporary.FindStringSubmatch(expr)
	if m == nil || !plainArg(strings.TrimSpace(m[1])) {
		return nil
	}
	fn := src.fnAt(p.ByteStart)
	if fn == nil {
		return nil
	}
	segments := regexp.MustCompile(`\w+`).FindAllString(m[1], -1)
	for _, b := range bound {
		if segments[0] == b {
			return nil
		}
	}
	name := segments[len(segments)-1] + "_app"
	if len(identUses(src.masked, fn.bodyStart, fn.bodyEnd, name)) > 0 {
		return nil
	}
	at := statementStart(src.masked, from, fn.bodyStart)
	inner := blockStatementStart(src.masked, outsideMacros(src.masked, from, fn.bodyStart), fn.bodyStart)
	if inner != at {
		// Out of a match arm: an arm's pattern may bind the root (`Some(dir) => …`), so the
		// root may not appear anywhere in the match's body before the bridge.
		open := enclosingOpenBrace(src.masked, inner, fn.bodyStart)
		if open < 0 || len(identUses(src.masked, open, p.ByteStart, segments[0])) > 0 {
			return nil
		}
	}
	start := p.ByteStart + strings.Index(string(src.text[p.ByteStart:p.ByteEnd]), expr)
	return []edit{
		{file: src.path, start: at, end: at, text: "\nlet " + name + " = " + expr + ";", rule: ruleHoist},
		{file: src.path, start: start, end: start + len(expr), text: name, rule: ruleHoist},
	}
}

// ---- E0515: a closure returning a future that borrows a bridge temporary ----

var closureParams = regexp.MustCompile(`(?:move\s+)?\|([^|]*)\|\s*(\{\s*)?$`)

// closureHoistEdits answers E0515 "cannot return value referencing temporary value" on a
// closure whose whole body is the call returning the future (`|| source.list_directory(
// &AppPath::legacy_unchecked(&path), None)`, a retry helper's closure): the temporary can't
// outlive the closure's own call, so it moves to `let path_app = …;` before the statement
// holding the closure, which the closure then borrows. A plain argument only, and ❌ never
// one rooted in the closure's own params (`|row| v.rename_work(&…(&row.source))`): residue.
func closureHoistEdits(d *diagnostic, src *source) []edit {
	if d.code() != "E0515" || d.Message != "cannot return value referencing temporary value" {
		return nil
	}
	call := d.primary()
	if call == nil {
		return nil
	}
	head := src.masked[:call.ByteStart]
	loc := closureParams.FindSubmatchIndex(head)
	if loc == nil {
		return nil
	}
	if loc[4] >= 0 && !bytes.HasPrefix(bytes.TrimLeft(src.masked[call.ByteEnd:], " \t\r\n"), []byte("}")) {
		return nil // a block body with more in it than the call
	}
	var bound []string
	for _, prm := range strings.Split(string(head[loc[2]:loc[3]]), ",") {
		if ident := regexp.MustCompile(`\w+`).FindString(prm); ident != "" {
			bound = append(bound, ident)
		}
	}
	for i := range d.Spans {
		s := &d.Spans[i]
		if s.IsPrimary || s.label() != "temporary value created here" || s.ByteStart < call.ByteStart || s.ByteEnd > call.ByteEnd {
			continue
		}
		return hoistFrom(src, s, loc[0], bound)
	}
	return nil
}

// statementStart walks back from offset to just past the `;`, `{`, or `}` that ends the
// previous statement at offset's own block level (enclosing parens and brackets are crossed,
// closed blocks skipped), never before floor. Inside a macro's token tree (`tokio::select! {
// biased; … }`, whose `;` and arm blocks are no statement ends) it starts from the outermost
// enclosing macro invocation instead.
// A match arm's expression (`None => read(…)`) is no statement either: the start is the
// whole `match`'s.
func statementStart(masked []byte, offset, floor int) int {
	offset = outsideMacros(masked, offset, floor)
	at := blockStatementStart(masked, offset, floor)
	if hasTopLevelArrow(masked[at:offset]) {
		if open := enclosingOpenBrace(masked, at, floor); open >= floor {
			return statementStart(masked, open, floor)
		}
	}
	return at
}

// hasTopLevelArrow answers whether a `=>` sits outside every bracket in b.
func hasTopLevelArrow(b []byte) bool {
	depth := 0
	for k := 0; k < len(b); k++ {
		switch b[k] {
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
		case '=':
			if depth == 0 && k+1 < len(b) && b[k+1] == '>' {
				return true
			}
		}
	}
	return false
}

// enclosingOpenBrace is the offset of the unmatched `{` before at, or -1.
func enclosingOpenBrace(masked []byte, at, floor int) int {
	depth := 0
	for k := at - 1; k >= floor; k-- {
		switch masked[k] {
		case '}':
			depth++
		case '{':
			if depth == 0 {
				return k
			}
			depth--
		}
	}
	return -1
}

func blockStatementStart(masked []byte, offset, floor int) int {
	braces, parens := 0, 0
	for k := offset - 1; k >= floor; k-- {
		switch masked[k] {
		case ')', ']':
			parens++
		case '(', '[':
			if parens > 0 {
				parens--
			}
		case '}':
			if parens == 0 && braces == 0 && k+1 < offset && isStatementEnd(masked, k) {
				return k + 1
			}
			braces++
		case '{':
			if braces == 0 {
				return k + 1
			}
			braces--
		case ';':
			if braces == 0 && parens == 0 {
				return k + 1
			}
		}
	}
	return floor
}

// outsideMacros returns the offset of the outermost macro invocation (its `!`) whose
// delimited token tree encloses offset, or offset itself when none does, never before floor.
func outsideMacros(masked []byte, offset, floor int) int {
	depth, out := 0, offset
	for k := offset - 1; k >= floor; k-- {
		switch masked[k] {
		case ')', ']', '}':
			depth++
		case '(', '[', '{':
			if depth > 0 {
				depth--
				continue
			}
			j := k - 1
			for j >= floor && (masked[j] == ' ' || masked[j] == '\t' || masked[j] == '\n' || masked[j] == '\r') {
				j--
			}
			if j > floor && masked[j] == '!' && isIdentByte(masked[j-1]) {
				out = j
			} else if masked[k] == '{' {
				// A real block (a fn body, a closure, a `select!` arm's `=> { … }`): the
				// statement lives in it, so no macro further out is ours.
				return out
			}
		}
	}
	return out
}

// isStatementEnd answers whether the `}` at k closes a block statement (followed by a new
// line before the statement we're in), not a block inside our expression (`if a { b } else`).
func isStatementEnd(masked []byte, k int) bool {
	rest := masked[k+1:]
	i := 0
	for i < len(rest) && (rest[i] == ' ' || rest[i] == '\t' || rest[i] == '\r') {
		i++
	}
	return i < len(rest) && rest[i] == '\n'
}
