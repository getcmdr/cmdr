package main

import (
	"regexp"
	"strings"
)

// ---- A one-element slice of a std path into a typed batch slot ----

var fromRefCallee = regexp.MustCompile(`^(?:(?:std|core)::)?(?:slice::)?from_ref$`)

// fromRefEdits answers `std::slice::from_ref(&path)` (a `PathBuf`) handed to a `&[AppPathBuf]`
// slot. rustc infers `from_ref`'s element from the slot, so it points at the ARGUMENT
// (`expected &AppPathBuf, found &PathBuf`, the call labeled "arguments to this function are
// incorrect"), which no other row answers. The element crosses the bridge as an owned value:
// `from_ref(&AppPath::legacy_unchecked(&path).into_owned())`, the same one-element slice of
// the same bytes. Only the real `core::slice::from_ref` (rustc's "function defined here"),
// called with that one argument.
func fromRefEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0308" {
		return nil
	}
	expected, found, ok := mismatch(p.label())
	if !ok || expected != "&AppPathBuf" || !isStdPathRef(found) {
		return nil
	}
	if !fromRefSoleArgument(d, src, p) || !definedInCoreSlice(d) {
		return nil
	}
	expr := string(src.text[p.ByteStart:p.ByteEnd])
	text := "&AppPath::legacy_unchecked(" + dropExtraBorrows(expr, found) + ").into_owned()"
	return []edit{{file: p.FileName, start: p.ByteStart, end: p.ByteEnd, text: text, rule: ruleFromRef}}
}

// fromRefSoleArgument answers whether p is the one argument of a call rustc labels
// "arguments to this function are incorrect" that is spelled `from_ref`.
func fromRefSoleArgument(d *diagnostic, src *source, p *diagSpan) bool {
	var call *diagSpan
	for i := range d.Spans {
		if s := &d.Spans[i]; s.label() == "arguments to this function are incorrect" && s.FileName == p.FileName && s.ByteEnd <= len(src.masked) {
			call = s
		}
	}
	if call == nil || !fromRefCallee.MatchString(strings.Join(strings.Fields(string(src.masked[call.ByteStart:call.ByteEnd])), "")) {
		return false
	}
	open := call.ByteEnd
	for open < len(src.masked) && isSpace(src.masked[open]) {
		open++
	}
	if open >= len(src.masked) || src.masked[open] != '(' {
		return false
	}
	close := matchBracket(src.masked, open, '(', ')')
	return close >= 0 && strings.TrimSpace(string(src.masked[open+1:p.ByteStart])) == "" &&
		strings.TrimRight(strings.TrimSpace(string(src.masked[p.ByteEnd:close])), ",") == ""
}

func definedInCoreSlice(d *diagnostic) bool {
	for _, child := range d.Children {
		if child.Message != "function defined here" {
			continue
		}
		for _, s := range child.Spans {
			if strings.Contains(s.FileName, "library/core/src/slice/") {
				return true
			}
		}
	}
	return false
}
