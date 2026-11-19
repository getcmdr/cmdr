package main

import "strings"

// An assert whose typed operand rustc read first: `assert_eq!(v.stands_in_for_own_disk(),
// Some(Path::new("/u")))` infers the pair's type from the left, so rustc reports the EXPECTED
// value as an E0308 (`expected Option<&AppPath>, found Option<&Path>`) rather than a
// comparison error, and its span is the argument itself, inside the invocation (no expansion
// to lift). The fix is the comparison rule's: the OTHER operand, the typed one, drops to
// `std::path` (`stdOperand`), and the expected value is untouched.

// assertArgEdits answers an E0308 whose span is one whole argument of an `assert_eq!`-family
// invocation, when the other argument has a std shape for the pair.
func assertArgEdits(src *source, p *diagSpan) []edit {
	expected, found, ok := mismatch(p.label())
	if !ok {
		return nil
	}
	args, ok := enclosingAssertArgs(src, p.ByteStart)
	if !ok || len(args) < 2 {
		return nil
	}
	for i := 0; i < 2; i++ {
		arg := strings.TrimSpace(string(src.text[args[i][0]:args[i][1]]))
		if arg != strings.TrimSpace(string(src.text[p.ByteStart:p.ByteEnd])) {
			continue
		}
		other := args[1-i]
		raw := string(src.text[other[0]:other[1]])
		expr := strings.TrimSpace(raw)
		text, ok := stdOperand(expr, expected, found)
		if !ok {
			return nil
		}
		start := other[0] + strings.Index(raw, expr)
		return []edit{{file: src.path, start: start, end: start + len(expr), text: text, rule: ruleCompare}}
	}
	return nil
}

// enclosingAssertArgs finds the innermost `(` open around offset and, when an assert macro's
// name and `!` precede it, answers that invocation's arguments.
func enclosingAssertArgs(src *source, offset int) ([][2]int, bool) {
	depth := 0
	for k := offset - 1; k >= 0; k-- {
		switch src.masked[k] {
		case ')', ']', '}':
			depth++
		case '[', '{':
			if depth == 0 {
				return nil, false
			}
			depth--
		case '(':
			if depth > 0 {
				depth--
				continue
			}
			j := k - 1
			for j >= 0 && isSpace(src.masked[j]) {
				j--
			}
			if j < 0 || src.masked[j] != '!' {
				return nil, false
			}
			end := j
			for j > 0 && isIdentByte(src.masked[j-1]) {
				j--
			}
			if !assertMacros[string(src.masked[j:end])] {
				return nil, false
			}
			close := matchParen(src.masked, k)
			if close < 0 {
				return nil, false
			}
			return macroArgs(src.masked, k, close+1), true
		}
	}
	return nil, false
}

// matchParen is the offset of the `)` closing the `(` at open, or -1.
func matchParen(masked []byte, open int) int {
	depth := 0
	for k := open; k < len(masked); k++ {
		switch masked[k] {
		case '(':
			depth++
		case ')':
			depth--
			if depth == 0 {
				return k
			}
		}
	}
	return -1
}
