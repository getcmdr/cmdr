package main

import (
	"bytes"
	"regexp"
	"strings"
)

// borrowedFutureEdits answers an E0515, a returned future borrowing something local: a
// bridge round trip's temporary, or a prelude a lazy helper's future borrows.
func (r *rewriter) borrowedFutureEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() == "E0716" {
		return hoistEdits(src, p)
	}
	if e := roundTripEdits(d, src, p); len(e) > 0 {
		return e
	}
	if e := splitRoundTripEdits(d, src, p); len(e) > 0 {
		return e
	}
	if e := r.asyncHelperEdits(d, src, p); len(e) > 0 {
		return e
	}
	return r.eagerHelperEdits(d, src, p)
}

// ---- A prelude'd body tail-calling a lazy helper ----

var (
	returnsLocalStd = regexp.MustCompile("^cannot return value referencing local variable `(\\w+)_std`$")
	preludeStmts    = regexp.MustCompile(`(?s)\s*let (\w+)_app = (\w+);|\s*let (\w+)_std(?:: Vec<PathBuf>)? = \w+\.[^;]*;\s*let \w+: &(?:\[PathBuf\]|Path) = &\w+_std;`)
	calleePattern   = regexp.MustCompile(`^(?:self(?:\.\w+)*\.|(?:\w+::)*)(\w+)$`)
	plainArgPattern = regexp.MustCompile(`^&?(?:mut\s+)?(?:[A-Za-z_]\w*(?:(?:::|\.)[A-Za-z_]\w*)*|\d[\w.]*)$`)
	asyncBlockOnly  = regexp.MustCompile(`^Box::pin\(\s*async(?:\s+move)?\s*\{`)
)

// asyncHelperEdits answers E0515 on a prelude'd body whose tail is a call tree returning
// the future (`Box::pin(self.helper(path, …))`, `self.helper(path, …)` when the helper
// returns the pinned box itself, `Box::pin(self.noting(self.x_impl(path)))`,
// `Box::pin(scan_walk::scan_one(self, path))`): the prelude's borrow can't outlive the call.
// When every call in the tree is lazy (`lazyFn`: its body runs at the first poll either
// way) and every argument is plain (`plainArg`), the body moves inside one `async move`
// block with the prelude, which is the same future. A call that does work before its future
// starts stays residue: moving it would change when that work runs.
func (r *rewriter) asyncHelperEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0515" || !returnsLocalStd.MatchString(d.Message) {
		return nil
	}
	m := src.methodAt(p.ByteStart)
	if m == nil {
		return nil
	}
	text := string(src.text[m.bodyStart:m.bodyEnd])
	masked := string(src.masked[m.bodyStart:m.bodyEnd])
	var prelude strings.Builder
	var restText, restMasked strings.Builder
	last := 0
	for _, loc := range preludeStmts.FindAllStringIndex(masked, -1) {
		prelude.WriteString(strings.TrimSpace(text[loc[0]:loc[1]]) + "\n")
		restText.WriteString(text[last:loc[0]])
		restMasked.WriteString(masked[last:loc[0]])
		last = loc[1]
	}
	restText.WriteString(text[last:])
	restMasked.WriteString(masked[last:])
	if prelude.Len() == 0 {
		return nil
	}
	tail, tailText := trimmedPair(restMasked.String(), restText.String())
	var body string
	if lets, block, ok := r.lazyLetsThenAsyncBlock(tail, tailText); ok {
		body = "\nBox::pin(async move {\n" + prelude.String() + lets + block + "\n})\n"
	} else {
		if inner, ok := boxPinArg(tail); ok {
			offset := strings.Index(tail, inner)
			tail, tailText = inner, tailText[offset:offset+len(inner)]
		}
		if !r.lazyCallTree(tail) {
			return nil
		}
		body = "\nBox::pin(async move {\n" + prelude.String() + tailText + ".await\n})\n"
	}
	return []edit{{file: src.path, start: m.bodyStart, end: m.bodyEnd, text: body, rule: ruleAsyncHelper}}
}

var lazyLet = regexp.MustCompile(`(?s)^let\s+(?:mut\s+)?\w+\s*=\s*(.+)$`)

// lazyLetsThenAsyncBlock reads a tail of `let x = <lazy call tree>;` statements (one or
// more) and then exactly `Box::pin(async move { … })`, the shape that binds a helper's
// future before boxing a block that awaits it (`let write = self.write_from_stream_impl(…);
// Box::pin(async move { self.noting(write).await })`). The lets move to the head of the
// block: each builds a future that does nothing until polled, and the block polls it no
// sooner than before. It answers the lets' text and the block's inner text.
func (r *rewriter) lazyLetsThenAsyncBlock(masked, text string) (string, string, bool) {
	stmts := splitOuter(masked, ';')
	if len(stmts) < 2 {
		return "", "", false
	}
	var lets strings.Builder
	for _, s := range stmts[:len(stmts)-1] {
		stmt, stmtText := trimmedPair(masked[s[0]:s[1]], text[s[0]:s[1]])
		m := lazyLet.FindStringSubmatch(stmt)
		if m == nil || !r.lazyCallTree(m[1]) {
			return "", "", false
		}
		lets.WriteString(stmtText + ";\n")
	}
	last := stmts[len(stmts)-1]
	final, finalText := trimmedPair(masked[last[0]:last[1]], text[last[0]:last[1]])
	loc := asyncBlockOnly.FindStringIndex(final)
	if loc == nil {
		return "", "", false
	}
	open := loc[1] - 1
	close := matchBrace([]byte(final), open)
	if close < 0 || strings.TrimSpace(final[close+1:]) != ")" {
		return "", "", false
	}
	return lets.String(), finalText[open+1 : close], true
}

// splitOuter splits at the top-level seps (`;` between statements, `,` between args), as
// [start, end) offsets, the last piece being what follows the final sep. Brackets only: a
// `=>` or a generic's `<` is no nesting.
func splitOuter(masked string, sep byte) [][2]int {
	var out [][2]int
	depth, start := 0, 0
	for i := 0; i < len(masked); i++ {
		switch masked[i] {
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
		case sep:
			if depth == 0 {
				out = append(out, [2]int{start, i})
				start = i + 1
			}
		}
	}
	return append(out, [2]int{start, len(masked)})
}

// trimmedPair trims masked's surrounding space and cuts text (same length) the same way.
func trimmedPair(masked, text string) (string, string) {
	start := len(masked) - len(strings.TrimLeft(masked, " \t\r\n"))
	end := len(strings.TrimRight(masked, " \t\r\n"))
	if end < start {
		return "", ""
	}
	return masked[start:end], text[start:end]
}

// boxPinArg is X when expr is exactly `Box::pin(X)`.
func boxPinArg(expr string) (string, bool) {
	if !strings.HasPrefix(expr, "Box::pin(") {
		return "", false
	}
	open := len("Box::pin")
	if matchBracket([]byte(expr), open, '(', ')') != len(expr)-1 {
		return "", false
	}
	return strings.TrimSpace(expr[open+1 : len(expr)-1]), true
}

// splitCall reads `callee(args)` filling all of expr: the callee text and the argument list.
func splitCall(expr string) (string, string, bool) {
	if !strings.HasSuffix(expr, ")") {
		return "", "", false
	}
	open := strings.IndexByte(expr, '(')
	if open <= 0 || matchBracket([]byte(expr), open, '(', ')') != len(expr)-1 {
		return "", "", false
	}
	return strings.TrimSpace(expr[:open]), expr[open+1 : len(expr)-1], true
}

// lazyCallTree answers whether expr (masked) is a call whose evaluation does no work beyond
// building futures and moving plain values: every call in it is a `lazyFn` (or a
// tuple-struct/variant constructor, `Some(x)`, `Opening::Fresh(mode)`), and every leaf
// argument is plain.
func (r *rewriter) lazyCallTree(expr string) bool {
	callee, args, ok := splitCall(strings.TrimSpace(expr))
	if !ok {
		return false
	}
	m := calleePattern.FindStringSubmatch(callee)
	if m == nil {
		return false
	}
	if !isConstructor(m[1]) && !r.lazyFn(m[1]) {
		return false
	}
	for _, a := range splitOuter(args, ',') {
		arg := strings.TrimSpace(args[a[0]:a[1]])
		if arg == "" || plainArg(arg) {
			continue
		}
		if !r.lazyCallTree(arg) {
			return false
		}
	}
	return true
}

// plainArg is an argument whose evaluation is a move, a copy, or a borrow of a local, a
// field read through `self`, a path (`MacOnlyFiles::Include`, `None`), or a number:
// nothing that runs code, reads interior-mutable state, panics, or returns early, so
// evaluating it at the first poll gives the value it gives at the call today.
func plainArg(arg string) bool {
	if !plainArgPattern.MatchString(arg) {
		return false
	}
	for _, seg := range regexp.MustCompile(`[.:]+`).Split(arg, -1) {
		if seg == "await" {
			return false
		}
	}
	return true
}

// isConstructor answers a callee named in UpperCamelCase: a tuple struct or enum variant
// (Rust's naming lint keeps functions snake_case), which builds a value and runs nothing.
func isConstructor(name string) bool {
	return name != "" && name[0] >= 'A' && name[0] <= 'Z'
}

// lazyFn answers whether every definition of `fn name` the crate holds (or, when it holds
// none, the workspace) is lazy: an `async fn`, or a `fn` whose body is only
// `Box::pin(async move { … })`. None found is not lazy.
func (r *rewriter) lazyFn(name string) bool {
	if lazy, ok := r.lazyMemo[name]; ok {
		return lazy
	}
	lazy, found := r.lazyIn(r.crateRust, name)
	if !found {
		lazy, found = r.lazyIn(r.workspaceRust, name)
		lazy = found && lazy
	}
	if r.lazyMemo == nil {
		r.lazyMemo = map[string]bool{}
	}
	r.lazyMemo[name] = lazy
	return lazy
}

func (r *rewriter) lazyIn(files []string, name string) (lazy, found bool) {
	needle := []byte("fn " + name)
	def := regexp.MustCompile(`\bfn\s+` + regexp.QuoteMeta(name) + `\b`)
	lazy = true
	for _, file := range files {
		// The workspace is big: only a file whose bytes name the fn gets parsed.
		if r.readRaw != nil {
			if raw, err := r.readRaw(file); err != nil || !bytes.Contains(raw, needle) {
				continue
			}
		}
		src, err := r.load(file)
		if err != nil || !bytes.Contains(src.text, needle) {
			continue
		}
		for _, loc := range def.FindAllIndex(src.masked, -1) {
			nameStart := loc[1] - len(name)
			fn, _, ok := parseMethod(src.masked, nameStart, loc[1], len(src.masked))
			if !ok {
				continue // a declaration without a body
			}
			found = true
			if !isAsyncDef(src.masked, loc[0]) && !bodyIsOneAsyncBlock(src.masked, fn) {
				lazy = false
			}
		}
	}
	return lazy, found
}

var asyncBeforeFn = regexp.MustCompile(`\basync\s+(?:unsafe\s+)?$`)

func isAsyncDef(masked []byte, fnAt int) bool {
	return asyncBeforeFn.Match(masked[max(0, fnAt-32):fnAt])
}

// bodyIsOneAsyncBlock answers a body that is exactly `Box::pin(async move { … })`, with
// nothing before or after it (comments are masked away).
func bodyIsOneAsyncBlock(masked []byte, fn implMethod) bool {
	body := strings.TrimSpace(string(masked[fn.bodyStart:fn.bodyEnd]))
	loc := asyncBlockOnly.FindStringIndex(body)
	if loc == nil {
		return false
	}
	close := matchBrace([]byte(body), loc[1]-1)
	return close >= 0 && strings.TrimSpace(body[close+1:]) == ")"
}

// ---- A bridge round trip: `&AppPath::legacy_unchecked(&path.as_std_legacy())` ----

var (
	roundTripOne   = regexp.MustCompile(`&\s*AppPath::legacy_unchecked\(\s*&\s*(\w+)\s*\.as_std_legacy\(\)\s*\)`)
	roundTripBatch = regexp.MustCompile(`&\s*(\w+)\s*\.iter\(\)\s*\.map\(\s*\|(\w+)\|\s*AppPath::legacy_unchecked\(\s*&\s*(\w+)\s*\.as_std_legacy\(\)\s*\)\s*\.into_owned\(\)\s*\)\s*\.collect::<Vec<_>>\(\)`)
)

// roundTripEdits collapses a typed param sent through the bridge and back
// (`&AppPath::legacy_unchecked(&path.as_std_legacy())`, the batch `&paths.iter().map(|p|
// AppPath::legacy_unchecked(&p.as_std_legacy()).into_owned()).collect::<Vec<_>>()`) to the
// param itself, on the E0515 its temporary causes. The param is the value the round trip
// rebuilds (the bridge is lossless on it), so this is the identity, minus the temporary.
// Only a param of the enclosing fn typed `&AppPath` (`&[AppPathBuf]`) collapses: the text
// alone can't tell a reference from an owned value.
func roundTripEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0515" {
		return nil
	}
	fn := src.fnAt(p.ByteStart)
	if fn == nil {
		return nil
	}
	from, to := p.ByteStart, p.ByteEnd
	for _, s := range d.Spans {
		if s.FileName == p.FileName && s.ByteEnd <= len(src.masked) {
			from, to = min(from, s.ByteStart), max(to, s.ByteEnd)
		}
	}
	kinds := map[string]string{}
	for _, prm := range fn.params {
		kinds[prm.name] = typedParamKind(prm.typ)
	}
	region := string(src.masked[from:to])
	var edits []edit
	for _, loc := range roundTripBatch.FindAllStringSubmatchIndex(region, -1) {
		name, elem, inner := region[loc[2]:loc[3]], region[loc[4]:loc[5]], region[loc[6]:loc[7]]
		if elem == inner && kinds[name] == "batch" {
			edits = append(edits, edit{file: src.path, start: from + loc[0], end: from + loc[1], text: name, rule: ruleRoundTrip})
		}
	}
	for _, loc := range roundTripOne.FindAllStringSubmatchIndex(region, -1) {
		if name := region[loc[2]:loc[3]]; kinds[name] == "one" {
			edits = append(edits, edit{file: src.path, start: from + loc[0], end: from + loc[1], text: name, rule: ruleRoundTrip})
		}
	}
	return edits
}
