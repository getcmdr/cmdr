package main

import "unicode/utf8"

// mask returns src with every comment and every string, byte-string, raw-string, and char
// literal's CONTENT blanked to spaces (newlines kept), same length and same byte offsets, so
// brace matching and identifier search over the result can't be fooled by a `{` in a string
// or a `path` in a comment. Lifetimes (`'a`) are kept: they aren't char literals.
func mask(src []byte) []byte {
	out := make([]byte, len(src))
	copy(out, src)
	i := 0
	for i < len(src) {
		switch {
		case hasPrefixAt(src, i, "//"):
			i = blankUntilNewline(out, i)
		case hasPrefixAt(src, i, "/*"):
			i = blankBlockComment(src, out, i)
		case isRawStringStart(src, i):
			i = blankRawString(src, out, i)
		case src[i] == '"' || (src[i] == 'b' && hasPrefixAt(src, i, "b\"") && !identBefore(src, i)):
			start := i
			if src[i] == 'b' {
				start++
			}
			i = blankQuoted(src, out, start)
		case src[i] == '\'':
			i = blankCharLiteral(src, out, i)
		default:
			i++
		}
	}
	return out
}

func hasPrefixAt(src []byte, i int, prefix string) bool {
	return i+len(prefix) <= len(src) && string(src[i:i+len(prefix)]) == prefix
}

func identBefore(src []byte, i int) bool {
	return i > 0 && isIdentByte(src[i-1])
}

func isIdentByte(b byte) bool {
	return b == '_' || (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') || b >= 0x80
}

func blankRange(out []byte, from, to int) {
	for k := from; k < to && k < len(out); k++ {
		if out[k] != '\n' {
			out[k] = ' '
		}
	}
}

func blankUntilNewline(out []byte, i int) int {
	end := i
	for end < len(out) && out[end] != '\n' {
		end++
	}
	blankRange(out, i, end)
	return end
}

func blankBlockComment(src, out []byte, i int) int {
	depth := 0
	k := i
	for k < len(src) {
		if hasPrefixAt(src, k, "/*") {
			depth++
			k += 2
			continue
		}
		if hasPrefixAt(src, k, "*/") {
			depth--
			k += 2
			if depth == 0 {
				break
			}
			continue
		}
		k++
	}
	blankRange(out, i, k)
	return k
}

// isRawStringStart matches `r"`, `r#"`, `br"`, `br#"` (not `r#ident`).
func isRawStringStart(src []byte, i int) bool {
	if identBefore(src, i) {
		return false
	}
	k := i
	if k < len(src) && src[k] == 'b' {
		k++
	}
	if k >= len(src) || src[k] != 'r' {
		return false
	}
	k++
	for k < len(src) && src[k] == '#' {
		k++
	}
	return k < len(src) && src[k] == '"'
}

func blankRawString(src, out []byte, i int) int {
	k := i
	for src[k] != '"' {
		k++
	}
	hashes := 0
	for j := k - 1; j >= i && src[j] == '#'; j-- {
		hashes++
	}
	closing := "\""
	for range hashes {
		closing += "#"
	}
	end := k + 1
	for end < len(src) && !hasPrefixAt(src, end, closing) {
		end++
	}
	blankRange(out, k+1, end)
	return min(end+len(closing), len(src))
}

// blankQuoted blanks a `"…"` literal's content; `start` is the opening quote.
func blankQuoted(src, out []byte, start int) int {
	k := start + 1
	for k < len(src) && src[k] != '"' {
		if src[k] == '\\' {
			k++
		}
		k++
	}
	blankRange(out, start+1, k)
	return k + 1
}

// blankCharLiteral blanks `'x'`, `'\n'`, `'\u{1F600}'`, `'é'`; a lifetime `'a` is left alone.
func blankCharLiteral(src, out []byte, i int) int {
	if i+1 >= len(src) {
		return i + 1
	}
	if src[i+1] == '\\' {
		k := i + 2
		for k < len(src) && src[k] != '\'' {
			k++
		}
		blankRange(out, i+1, k)
		return k + 1
	}
	_, size := utf8.DecodeRune(src[i+1:])
	if i+1+size < len(src) && src[i+1+size] == '\'' {
		blankRange(out, i+1, i+1+size)
		return i + 2 + size
	}
	return i + 1
}
