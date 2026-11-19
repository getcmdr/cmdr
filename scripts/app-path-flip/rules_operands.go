package main

import (
	"regexp"
	"strings"
)

// The app's shapes: a typed value (most often a `Volume::root()`) read where `std::path` code
// takes it generically or compares it with `==`. Neither is an E0308, so the type-pair table
// never sees them.

// stdReadEdits is this file's two rules: a plain comparison, else a generic std bound.
func stdReadEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if e := operatorCompareEdits(d, src, p); len(e) > 0 {
		return e
	}
	return boundArgEdits(d, src, p)
}

// ---- A typed value into a generic std bound ----

// stdBound matches the E0277 rustc gives a typed value handed to a generic slot bounded by
// a trait `Path` implements and `AppPath` doesn't: `row.starts_with(volume.root())`
// (`AsRef<Path>`), `LogPrivate::dir_path(volume.root())` and `Path::new(live.root())`
// (`AsRef<OsStr>`), `MountRoot::new(volume.root())` (`Into<PathBuf>`).
var stdBound = regexp.MustCompile("^the trait bound `&?(?:[\\w:]+::)?(?:AppPath|AppPathBuf): (?:AsRef<(?:std::path::)?Path>|AsRef<(?:std::ffi::)?OsStr>|Into<(?:std::path::)?PathBuf>)` is not satisfied$")

// boundArgEdits drops the value rustc points at (the argument, its span exactly) to
// `&*x.as_std_legacy()`: a `&Path`, which meets all three bounds, with the same bytes. Only
// when rustc also labels the call ("required by a bound introduced by this call"), so the
// span is an argument and nothing else.
func boundArgEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0277" || !stdBound.MatchString(d.Message) {
		return nil
	}
	introduced := false
	for _, s := range d.Spans {
		if !s.IsPrimary && s.label() == "required by a bound introduced by this call" {
			introduced = true
		}
	}
	if !introduced {
		return nil
	}
	expr := strings.TrimSpace(string(src.text[p.ByteStart:p.ByteEnd]))
	if expr == "" || strings.Contains(expr, "as_std_legacy") {
		return nil
	}
	start := p.ByteStart + strings.Index(string(src.text[p.ByteStart:p.ByteEnd]), expr)
	text := "&*" + postfix(expr) + ".as_std_legacy()"
	return []edit{{file: p.FileName, start: start, end: start + len(expr), text: text, rule: ruleBoundArg}}
}

// ---- The operands of a plain `==` / `!=` ----

// operatorCompareEdits answers a plain `a == b` outside a macro whose one side is typed: the
// comparison rule's fix, with the operands read off the source. It runs after the prelude
// rule, so a flipped body's stale comparison of its own param gets the prelude instead.
func operatorCompareEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0277" || !cantCompare.MatchString(d.Message) {
		return nil
	}
	left, right, ok := comparedTypes(d)
	if !ok {
		return nil
	}
	return operandEdits(src, p, operatorOperands(src.masked, p), left, right)
}

// operatorOperands finds the two operands of the `==`/`!=` at [p.ByteStart, p.ByteEnd): the
// span an E0277 "can't compare" points at outside a macro, with no label on either side.
// Each operand must be a plain postfix chain (`entry.volume.root()`, `r.path`,
// `std::path::Path::new(x)`) bounded by something binding looser than `==` (a bracket, `,`,
// `;`, `=`, a closure's `|`, `&&`/`||`, a block's `{`, or a keyword like `if`), so the chain
// is the whole operand. Anything else answers nil (residue).
func operatorOperands(masked []byte, p *diagSpan) [][2]int {
	op := strings.TrimSpace(string(masked[p.ByteStart:p.ByteEnd]))
	if op != "==" && op != "!=" {
		return nil
	}
	leftEnd := p.ByteStart
	for leftEnd > 0 && isSpace(masked[leftEnd-1]) {
		leftEnd--
	}
	leftStart := receiverStart(masked, leftEnd)
	if leftStart == leftEnd || !looseBefore(masked, leftStart) {
		return nil
	}
	rightStart := p.ByteEnd
	for rightStart < len(masked) && isSpace(masked[rightStart]) {
		rightStart++
	}
	rightEnd := chainEnd(masked, rightStart)
	if rightEnd == rightStart || !looseAfter(masked, rightEnd) {
		return nil
	}
	return [][2]int{{leftStart, leftEnd}, {rightStart, rightEnd}}
}

// chainEnd reads a postfix chain forward from k: path segments (`a::b`), each optionally
// called or indexed, joined by `.`, with `?`s. It answers where the chain ends (k when none).
func chainEnd(masked []byte, k int) int {
	end := k
	j := k
	for {
		next, ok := linkEnd(masked, j)
		if !ok {
			return end
		}
		end = next
		if next >= len(masked) || masked[next] != '.' {
			return end
		}
		j = next + 1
	}
}

// linkEnd reads one link of a chain from j (`a::b(x)[i]?`): where it ends, or false when
// none starts at j.
func linkEnd(masked []byte, j int) (int, bool) {
	j, ok := identEnd(masked, j)
	if !ok {
		return 0, false
	}
	for j+1 < len(masked) && masked[j] == ':' && masked[j+1] == ':' {
		if j, ok = identEnd(masked, j+2); !ok {
			return 0, false
		}
	}
	for j < len(masked) && (masked[j] == '(' || masked[j] == '[') {
		close := matchForward(masked, j)
		if close < 0 {
			return 0, false
		}
		j = close + 1
	}
	for j < len(masked) && masked[j] == '?' {
		j++
	}
	return j, true
}

// identEnd is the end of the identifier starting at j, or false when none does.
func identEnd(masked []byte, j int) (int, bool) {
	start := j
	for j < len(masked) && isIdentByte(masked[j]) {
		j++
	}
	return j, j > start
}

// matchForward is the offset of the bracket closing the `(` or `[` at open, or -1.
func matchForward(masked []byte, open int) int {
	closeBy := map[byte]byte{'(': ')', '[': ']'}[masked[open]]
	depth := 0
	for k := open; k < len(masked); k++ {
		switch masked[k] {
		case masked[open]:
			depth++
		case closeBy:
			depth--
			if depth == 0 {
				return k
			}
		}
	}
	return -1
}

// closesClosureParams answers whether the `|` at bar closes a closure's parameter list
// (`|(_, v)| v.root() == …`), not a bit-or, which binds tighter than `==`: an opening `|`
// earlier on the line with no `|` between, preceded by a bracket, `,`, `=`, or `move`.
func closesClosureParams(masked []byte, bar int) bool {
	q := bar - 1
	for q >= 0 && masked[q] != '|' && masked[q] != '\n' {
		q--
	}
	if q < 0 || masked[q] != '|' {
		return false
	}
	k := q
	for k > 0 && isSpace(masked[k-1]) {
		k--
	}
	if k > 0 && strings.IndexByte("({[,=", masked[k-1]) >= 0 {
		return true
	}
	return k >= 4 && string(masked[k-4:k]) == "move"
}

var looseKeywords = map[string]bool{"if": true, "while": true, "return": true, "in": true, "else": true, "match": true}

// looseBefore answers whether what precedes offset binds looser than `==`.
func looseBefore(masked []byte, offset int) bool {
	k := offset
	for k > 0 && isSpace(masked[k-1]) {
		k--
	}
	if k == 0 {
		return false
	}
	if loose, decided := looseOperatorBefore(masked, k); decided {
		return loose
	}
	w := k
	for w > 0 && isIdentByte(masked[w-1]) {
		w--
	}
	return looseKeywords[string(masked[w:k])]
}

// looseOperatorBefore reads the punctuation ending at k (exclusive): whether it binds looser
// than `==`, and whether it decided at all (a word is the caller's to read).
func looseOperatorBefore(masked []byte, k int) (bool, bool) {
	c := masked[k-1]
	prev := byte(0)
	if k >= 2 {
		prev = masked[k-2]
	}
	switch {
	case strings.IndexByte("({[,;", c) >= 0:
		return true, true
	case c == '=':
		return prev == 0 || strings.IndexByte("=!<>", prev) < 0, true
	case c == '|':
		return prev == '|' || closesClosureParams(masked, k-1), true
	case c == '&':
		return prev == '&', true
	case c == '>':
		return prev == '=', true // a match arm's `=>`
	}
	return false, !isIdentByte(c)
}

// looseAfter answers whether what follows offset binds looser than `==`.
func looseAfter(masked []byte, offset int) bool {
	k := offset
	for k < len(masked) && isSpace(masked[k]) {
		k++
	}
	if k >= len(masked) {
		return false
	}
	c := masked[k]
	switch {
	case strings.IndexByte(")}],;{", c) >= 0:
		return true
	case c == '&' || c == '|':
		return k+1 < len(masked) && masked[k+1] == c
	case c == '=':
		return k+1 < len(masked) && masked[k+1] == '>'
	}
	return false
}
