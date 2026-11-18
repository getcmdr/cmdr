package main

import (
	"regexp"
	"strings"
)

// ---- A prelude'd body tail-calling an eager helper ----

var (
	// The prelude statements a body may hold, one name each: the plain and the batch twin.
	eagerPreludeStmt = regexp.MustCompile(`^let (\w+)_std = (\w+)\.as_std_legacy\(\)$|^let (\w+)_std: Vec<PathBuf> = (\w+)\.iter\(\)\.map\(\|p\| p\.as_std_legacy\(\)\.into_owned\(\)\)\.collect\(\)$`)
	preludeShadow    = regexp.MustCompile(`^let (\w+): &(Path|\[PathBuf\]) = &(\w+)_std$`)
	selfCallee       = regexp.MustCompile(`^self\.(\w+)$`)
	selfReceiver     = regexp.MustCompile(`^&\s*(?:'\w+\s+)?self$`)
	pinnedFuture     = regexp.MustCompile(`^Pin<Box<dyn(?:Future|std::future::Future|core::future::Future)<`)
	// `&AppPath::legacy_unchecked(y)` in a helper's future: the typed param, bridged back.
	bridgedBack = `&\s*AppPath::legacy_unchecked\(\s*%s\s*\)`
)

// maxForwardingDepth caps a chain of forwarding helpers (`write_from_stream_impl` →
// `stream_into`); real chains are one or two deep.
const maxForwardingDepth = 4

// eagerHelperEdits answers E0515 on a prelude'd `Volume` impl body whose tail is
// `self.h(…, x, …)`, h being a helper of this crate that does work before its future starts
// (`let abs_path = self.resolve(path); Box::pin(async move { … })`), so the lazy-helper rule
// can't move the call into an `async move` without delaying that work. Instead h takes the
// typed path and holds the prelude itself, at the top of its body, and the caller hands its
// typed param straight on. The prelude is a pure conversion, and it still runs before the
// helper's own statements and before the future exists: nothing moves relative to the first
// poll. A helper whose whole body is `self.h2(…, y, …)` forwards: it only gets the typed
// param, and the prelude goes down the chain to the eager helper at its end.
//
// Every condition is a reason the move might not be the identity, so a miss is residue:
// the caller's body is exactly the prelude and the call; each prelude'd name is exactly one
// argument and appears in no other; h is the crate's only `fn h`, a `&self` method, not
// async, returning `Pin<Box<dyn Future …>>`, with the matching `&Path`/`&[PathBuf]` param;
// the eager helper's final `Box::pin(async move { … })` names the param only as
// `&AppPath::legacy_unchecked(y)` (which becomes the typed param, kept as `y_app`: the bridge
// is lossless), and no statement rebinds it. h's other std callers get bridged by the value
// rules on their next round, as a hand fix does.
func (r *rewriter) eagerHelperEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0515" || !returnsLocalStd.MatchString(d.Message) {
		return nil
	}
	m := src.methodAt(p.ByteStart)
	if m == nil {
		return nil
	}
	names, tail, tailText, ok := preludeThenTail(src, m)
	if !ok {
		return nil
	}
	callee, args, ok := splitCall(tail)
	if !ok {
		return nil
	}
	cm := selfCallee.FindStringSubmatch(callee)
	if cm == nil {
		return nil
	}
	var edits []edit
	for name, kind := range names {
		at := soleArg(args, name)
		if at < 0 {
			return nil
		}
		helper, ok := r.eagerChain(cm[1], at, kind, 0, map[string]bool{})
		if !ok {
			return nil
		}
		edits = append(edits, helper...)
	}
	return append(edits, edit{file: src.path, start: m.bodyStart, end: m.bodyEnd, text: "\n" + tailText + "\n", rule: ruleEagerHelper})
}

// preludeThenTail reads a body that is exactly codemod preludes (`let x_std = …; let x: &Path
// = &x_std;`) followed by one tail expression: the prelude'd names with their kind, and the
// tail (masked and as written).
func preludeThenTail(src *source, m *implMethod) (map[string]string, string, string, bool) {
	masked := string(src.masked[m.bodyStart:m.bodyEnd])
	text := string(src.text[m.bodyStart:m.bodyEnd])
	stmts := splitOuter(masked, ';')
	if len(stmts) < 3 || len(stmts)%2 == 0 {
		return nil, "", "", false
	}
	names := map[string]string{}
	for i := 0; i+1 < len(stmts); i += 2 {
		first, _ := trimmedPair(masked[stmts[i][0]:stmts[i][1]], text[stmts[i][0]:stmts[i][1]])
		second, _ := trimmedPair(masked[stmts[i+1][0]:stmts[i+1][1]], text[stmts[i+1][0]:stmts[i+1][1]])
		a := eagerPreludeStmt.FindStringSubmatch(first)
		b := preludeShadow.FindStringSubmatch(second)
		if a == nil || b == nil {
			return nil, "", "", false
		}
		name, kind := a[1], "one"
		if a[1] == "" {
			name, kind = a[3], "batch"
		}
		if (a[2] != name && a[4] != name) || b[1] != name || b[3] != name || (kind == "one") != (b[2] == "Path") {
			return nil, "", "", false
		}
		if _, dup := names[name]; dup {
			return nil, "", "", false
		}
		names[name] = kind
	}
	last := stmts[len(stmts)-1]
	tail, tailText := trimmedPair(masked[last[0]:last[1]], text[last[0]:last[1]])
	if tail == "" {
		return nil, "", "", false
	}
	return names, tail, tailText, true
}

// eagerChain answers the edits that make helper `name`'s param at index `at` (counting from
// the first after `self`) typed: a forwarder retypes it and recurses, the eager helper at the
// chain's end retypes it and takes the prelude. False when any link doesn't qualify.
func (r *rewriter) eagerChain(name string, at int, kind string, depth int, seen map[string]bool) ([]edit, bool) {
	if depth >= maxForwardingDepth || seen[name] {
		return nil, false
	}
	seen[name] = true
	src, fn, ok := r.uniqueHelper(name)
	if !ok || at >= len(fn.params) {
		return nil, false
	}
	prm := fn.params[at]
	want := "&Path"
	if kind == "batch" {
		want = "&[PathBuf]"
	}
	if normalizeType(prm.typ) != want || hasPrelude(src, fn, prm.name) || rebinds(src, fn, prm.name) {
		return nil, false
	}
	retype, ok := paramRetype(src, fn, at)
	if !ok {
		return nil, false
	}
	body := strings.TrimSpace(string(src.masked[fn.bodyStart:fn.bodyEnd]))
	// A forwarder: the whole body is `self.h2(…, y, …)`, y one argument and nowhere else.
	if callee, args, ok := splitCall(body); ok {
		cm := selfCallee.FindStringSubmatch(callee)
		next := soleArg(args, prm.name)
		if cm == nil || next < 0 {
			return nil, false
		}
		rest, ok := r.eagerChain(cm[1], next, kind, depth+1, seen)
		if !ok {
			return nil, false
		}
		return append(rest, retype), true
	}
	return eagerEndEdits(src, fn, prm.name, kind, retype)
}

// eagerEndEdits are the chain's end: an eager helper (statements, then exactly
// `Box::pin(async move { … })`) gets its retype, the prelude at the top of its body, and
// each `&AppPath::legacy_unchecked(y)` in its future turned into the kept typed `y_app`. Any
// other use of y in the future would borrow the prelude (E0515 one level down): no edit.
func eagerEndEdits(src *source, fn *implMethod, name, kind string, retype edit) ([]edit, bool) {
	finalStart, finalEnd, ok := finalAsyncBox(src, fn)
	if !ok {
		return nil, false
	}
	back := regexp.MustCompile(strings.ReplaceAll(bridgedBack, "%s", regexp.QuoteMeta(name)))
	hits := back.FindAllIndex(src.masked[finalStart:finalEnd], -1)
	if kind == "batch" && len(hits) > 0 {
		return nil, false
	}
	covered := 0
	for _, h := range hits {
		covered += len(identUses(src.masked, finalStart+h[0], finalStart+h[1], name))
	}
	if len(identUses(src.masked, finalStart, finalEnd, name)) != covered {
		return nil, false
	}
	insert := preludeText(name, kind)
	if len(hits) > 0 {
		insert = "\nlet " + name + "_app = " + name + ";" + insert
	}
	edits := []edit{retype, {file: src.path, start: fn.bodyStart, end: fn.bodyStart, text: insert, rule: ruleEagerHelper}}
	for _, h := range hits {
		edits = append(edits, edit{file: src.path, start: finalStart + h[0], end: finalStart + h[1], text: name + "_app", rule: ruleEagerHelper})
	}
	return edits, true
}

// finalAsyncBox is the file span of a body's final expression when the body is statements
// (one at least) and then exactly `Box::pin(async move { … })`.
func finalAsyncBox(src *source, fn *implMethod) (int, int, bool) {
	region := string(src.masked[fn.bodyStart:fn.bodyEnd])
	stmts := splitOuter(region, ';')
	if len(stmts) < 2 {
		return 0, 0, false
	}
	last := stmts[len(stmts)-1]
	final := strings.TrimSpace(region[last[0]:last[1]])
	loc := asyncBlockOnly.FindStringIndex(final)
	if loc == nil {
		return 0, 0, false
	}
	if close := matchBrace([]byte(final), loc[1]-1); close < 0 || strings.TrimSpace(final[close+1:]) != ")" {
		return 0, 0, false
	}
	start := fn.bodyStart + last[0] + strings.Index(region[last[0]:last[1]], final)
	return start, start + len(final), true
}

// soleArg is the index of the one argument that is exactly name, or -1 when none is, two
// are, or another argument mentions it too.
func soleArg(args, name string) int {
	at := -1
	for i, a := range splitOuter(args, ',') {
		arg := strings.TrimSpace(args[a[0]:a[1]])
		switch {
		case arg == name && at < 0:
			at = i
		case arg == name || containsIdent([]byte(arg), name):
			return -1
		}
	}
	return at
}

// uniqueHelper finds the crate's one `fn name` with a body: a `&self` method returning
// `Pin<Box<dyn Future …>>`, not async, its params all plain names (so argument and param
// indexes line up). A second definition or a bodyless declaration (a trait's) is ambiguous.
func (r *rewriter) uniqueHelper(name string) (*source, *implMethod, bool) {
	foundSrc, found, ok := r.soleDefinition(name)
	if !ok || found.retStart < 0 {
		return nil, nil, false
	}
	if !pinnedFuture.MatchString(normalizeType(string(foundSrc.masked[found.retStart:found.retEnd]))) {
		return nil, nil, false
	}
	parts := paramParts(foundSrc, found)
	if len(parts) == 0 || !selfReceiver.MatchString(strings.TrimSpace(parts[0])) || len(parts)-1 != len(found.params) {
		return nil, nil, false
	}
	return foundSrc, found, true
}

// soleDefinition is the crate's only `fn name`, when it has a body and isn't async.
func (r *rewriter) soleDefinition(name string) (*source, *implMethod, bool) {
	def := regexp.MustCompile(`\bfn\s+` + regexp.QuoteMeta(name) + `\b`)
	var foundSrc *source
	var found *implMethod
	for _, file := range r.crateRust {
		src, err := r.load(file)
		if err != nil {
			continue
		}
		for _, loc := range def.FindAllIndex(src.masked, -1) {
			if found != nil || isAsyncDef(src.masked, loc[0]) {
				return nil, nil, false
			}
			for i := range src.fns {
				if src.fns[i].sigStart == loc[1]-len(name) {
					found, foundSrc = &src.fns[i], src
				}
			}
			if found == nil {
				return nil, nil, false // a declaration without a body: a trait's
			}
		}
	}
	return foundSrc, found, found != nil
}

// paramParts splits a fn's param list (masked) at its top-level commas, a trailing comma's
// empty piece dropped.
func paramParts(src *source, fn *implMethod) []string {
	paren := indexByteFrom(src.masked, fn.sigStart, '(')
	close := matchBracket(src.masked, paren, '(', ')')
	var parts []string
	for _, part := range splitTopLevel(string(src.masked[paren+1:close]), ',') {
		if strings.TrimSpace(part) != "" {
			parts = append(parts, part)
		}
	}
	return parts
}

// paramRetype is the edit turning param `at`'s `Path`/`PathBuf` into `AppPath`/`AppPathBuf`.
func paramRetype(src *source, fn *implMethod, at int) (edit, bool) {
	paren := indexByteFrom(src.masked, fn.sigStart, '(')
	close := matchBracket(src.masked, paren, '(', ')')
	offset := paren + 1
	index := -1 // the receiver is part 0
	for _, part := range splitTopLevel(string(src.masked[paren+1:close]), ',') {
		if index == at {
			colon := topLevelColon(part)
			if colon < 0 {
				return edit{}, false
			}
			start, end := offset+colon+1, offset+len(part)
			typ := string(src.text[start:end])
			retyped := stdPathBufWord.ReplaceAllString(typ, "AppPathBuf")
			retyped = stdPathWord.ReplaceAllString(retyped, "AppPath")
			if retyped == typ {
				return edit{}, false
			}
			return edit{file: src.path, start: start, end: end, text: retyped, rule: ruleEagerHelper}, true
		}
		offset += len(part) + 1
		index++
	}
	return edit{}, false
}

// rebinds answers whether the body binds name again (`let name`, `for name in`, a closure
// param, a `Some(name) =`/`=>` pattern), so a textual use of it might not be the param. A
// closure's param list holds no `.`, `(`, `{`, or `;`, which keeps `|e| … x.path() … |`
// (two closures with a method call between) from reading as one.
func rebinds(src *source, fn *implMethod, name string) bool {
	body := src.masked[fn.bodyStart:fn.bodyEnd]
	q := regexp.QuoteMeta(name)
	return regexp.MustCompile(`\b(?:let|for)\s+(?:mut\s+)?` + q + `\b|\|[^|.(){};]*\b` + q + `\b[^|.(){};]*\||\b(?:Some|Ok|Err)\(\s*` + q + `\s*\)\s*=`).Match(body)
}
