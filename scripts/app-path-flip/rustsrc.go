package main

import (
	"regexp"
	"strings"
)

// source is one Rust file as the rules read it: the bytes, their masked twin (comments and
// literal contents blanked), and the `Volume` impl methods found in it.
type source struct {
	path    string
	text    []byte
	masked  []byte
	methods []implMethod
	fns     []implMethod // every fn with a body, for the rules that aren't about a Volume impl
}

// implMethod is one `fn` inside an `impl … Volume for …` block (or `ScanSource`, `PatchSource`).
type implMethod struct {
	name      string
	sigStart  int    // offset of the `fn` name: the signature runs from here to the body
	selfType  string // the type the impl is for, as written (`ArchiveVolume`, `Wrapper<T>`)
	params    []param
	retStart  int // the return type's span, or -1 when there's none
	retEnd    int
	bodyStart int // offset just past the body's `{`
	bodyEnd   int // offset of the body's closing `}`
}

type param struct {
	name string
	typ  string
}

func newSource(path string, text []byte) *source {
	s := &source{path: path, text: text, masked: mask(text)}
	s.methods = findVolumeImplMethods(s.masked)
	s.fns = allFns(s.masked)
	return s
}

var (
	implHeaderPattern = regexp.MustCompile(`\bimpl\b`)
	// The `Volume` trait and the backend-body traits flipped beside it (their impls get the
	// same signature, prelude, and lazy-helper rules).
	volumeForPattern = regexp.MustCompile(`(?:^|[\s:>])(?:Volume|ScanSource|PatchSource)\s+for\s+([\w:]+(?:<[^{]*>)?)`)
	fnPattern        = regexp.MustCompile(`\bfn\s+(\w+)`)
)

// findVolumeImplMethods lists every method of every `impl … Volume for X` block.
func findVolumeImplMethods(masked []byte) []implMethod {
	var methods []implMethod
	for _, loc := range implHeaderPattern.FindAllIndex(masked, -1) {
		open := indexByteFrom(masked, loc[1], '{')
		if open < 0 {
			continue
		}
		header := string(masked[loc[1]:open])
		if strings.ContainsAny(header, ";}") {
			continue
		}
		m := volumeForPattern.FindStringSubmatch(header)
		if m == nil {
			continue
		}
		close := matchBrace(masked, open)
		if close < 0 {
			continue
		}
		methods = append(methods, methodsIn(masked, open+1, close, strings.TrimSpace(m[1]))...)
	}
	return methods
}

// methodsIn finds the `fn` items directly inside an impl body [from, to).
func methodsIn(masked []byte, from, to int, selfType string) []implMethod {
	var methods []implMethod
	k := from
	for k < to {
		loc := fnPattern.FindSubmatchIndex(masked[k:to])
		if loc == nil {
			break
		}
		nameStart, nameEnd := k+loc[2], k+loc[3]
		method, next, ok := parseMethod(masked, nameStart, nameEnd, to)
		if !ok {
			k = nameEnd
			continue
		}
		method.selfType = selfType
		methods = append(methods, method)
		k = next
	}
	return methods
}

// parseMethod reads one fn's params, return type, and body, starting at its name.
func parseMethod(masked []byte, nameStart, nameEnd, limit int) (implMethod, int, bool) {
	method := implMethod{name: string(masked[nameStart:nameEnd]), sigStart: nameStart, retStart: -1, retEnd: -1}
	paren := indexByteFrom(masked, nameEnd, '(')
	if paren < 0 || paren >= limit {
		return method, 0, false
	}
	closeParen := matchBracket(masked, paren, '(', ')')
	if closeParen < 0 {
		return method, 0, false
	}
	method.params = parseParams(string(masked[paren+1 : closeParen]))
	body := indexByteFrom(masked, closeParen, '{')
	semi := indexByteFrom(masked, closeParen, ';')
	if body < 0 || body >= limit || (semi >= 0 && semi < body) {
		return method, closeParen, false
	}
	method.retStart, method.retEnd = returnTypeSpan(masked, closeParen+1, body)
	end := matchBrace(masked, body)
	if end < 0 {
		return method, 0, false
	}
	method.bodyStart, method.bodyEnd = body+1, end
	return method, end + 1, true
}

// returnTypeSpan is the span after `->` up to a `where` or the body's `{`.
func returnTypeSpan(masked []byte, from, body int) (int, int) {
	segment := string(masked[from:body])
	arrow := strings.Index(segment, "->")
	if arrow < 0 {
		return -1, -1
	}
	start := from + arrow + 2
	end := body
	if where := regexp.MustCompile(`\bwhere\b`).FindStringIndex(string(masked[start:body])); where != nil {
		end = start + where[0]
	}
	for start < end && isSpace(masked[start]) {
		start++
	}
	for end > start && isSpace(masked[end-1]) {
		end--
	}
	return start, end
}

func parseParams(list string) []param {
	var params []param
	for _, part := range splitTopLevel(list, ',') {
		part = strings.TrimSpace(part)
		colon := topLevelColon(part)
		if colon < 0 {
			continue
		}
		name := strings.TrimSpace(part[:colon])
		name = strings.TrimPrefix(name, "mut ")
		if name == "self" || strings.HasSuffix(name, "self") || !isIdent(name) {
			continue
		}
		params = append(params, param{name: name, typ: strings.TrimSpace(part[colon+1:])})
	}
	return params
}

// topLevelColon finds the `:` that separates a param's pattern from its type (not a `::`).
func topLevelColon(part string) int {
	for i := 0; i < len(part); i++ {
		if part[i] != ':' {
			continue
		}
		if i+1 < len(part) && part[i+1] == ':' {
			i++
			continue
		}
		return i
	}
	return -1
}

func isIdent(s string) bool {
	if s == "" {
		return false
	}
	for i := 0; i < len(s); i++ {
		if !isIdentByte(s[i]) {
			return false
		}
	}
	return !(s[0] >= '0' && s[0] <= '9')
}

func isSpace(b byte) bool {
	return b == ' ' || b == '\n' || b == '\t' || b == '\r'
}

// splitTopLevel splits on sep outside every bracket pair.
func splitTopLevel(s string, sep byte) []string {
	var parts []string
	depth, start := 0, 0
	for i := 0; i < len(s); i++ {
		switch s[i] {
		case '(', '[', '{', '<':
			depth++
		case ')', ']', '}', '>':
			if s[i] == '>' && i > 0 && s[i-1] == '-' {
				continue
			}
			depth--
		default:
			if s[i] == sep && depth == 0 {
				parts = append(parts, s[start:i])
				start = i + 1
			}
		}
	}
	return append(parts, s[start:])
}

func indexByteFrom(b []byte, from int, c byte) int {
	for k := from; k < len(b); k++ {
		if b[k] == c {
			return k
		}
	}
	return -1
}

// matchBrace returns the offset of the `}` closing the `{` at open, or -1.
func matchBrace(masked []byte, open int) int {
	return matchBracket(masked, open, '{', '}')
}

func matchBracket(masked []byte, open int, left, right byte) int {
	depth := 0
	for k := open; k < len(masked); k++ {
		switch masked[k] {
		case left:
			depth++
		case right:
			depth--
			if depth == 0 {
				return k
			}
		}
	}
	return -1
}

// allFns lists every `fn` with a body anywhere in the file, nested ones and the ones a
// `macro_rules!` body spells included (`forward_to_inner!`'s methods), in source order.
func allFns(masked []byte) []implMethod {
	var fns []implMethod
	for _, loc := range fnPattern.FindAllSubmatchIndex(masked, -1) {
		if fn, _, ok := parseMethod(masked, loc[2], loc[3], len(masked)); ok {
			fns = append(fns, fn)
		}
	}
	return fns
}

// fnAt returns the innermost fn (any, not only a Volume impl's) whose body holds offset.
func (s *source) fnAt(offset int) *implMethod {
	var best *implMethod
	for i := range s.fns {
		f := &s.fns[i]
		if offset >= f.bodyStart && offset < f.bodyEnd && (best == nil || f.bodyStart > best.bodyStart) {
			best = f
		}
	}
	return best
}

// fnWithSignatureAt returns the innermost fn whose signature (name to body) holds offset.
func (s *source) fnWithSignatureAt(offset int) *implMethod {
	var best *implMethod
	for i := range s.fns {
		f := &s.fns[i]
		if offset >= f.sigStart && offset < f.bodyStart && (best == nil || f.sigStart > best.sigStart) {
			best = f
		}
	}
	return best
}

// methodAt returns the Volume impl method whose body holds offset, or nil.
func (s *source) methodAt(offset int) *implMethod {
	for i := range s.methods {
		m := &s.methods[i]
		if offset >= m.bodyStart && offset < m.bodyEnd {
			return m
		}
	}
	return nil
}
