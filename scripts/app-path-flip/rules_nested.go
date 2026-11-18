package main

import (
	"fmt"
	"regexp"
	"strings"
)

// ---- A returned async block borrowing a top-of-body prelude ----

var asyncMoveOpen = regexp.MustCompile(`async\s+move\s*\{`)

// nestedPreludeEdits answers E0515 `path_std` on a body whose prelude sits at its top (the
// param is read outside a future too) and whose returned value holds an `async move { … }`
// that reads the std `path` (`return Box::pin(async move { Err(NotFound(path.display()…)) })`,
// each arm of an `if`): the block can't own the borrow of a local. Each such block (the
// outermost ones in rustc's span) gets its own prelude from the typed param, kept as
// `let path_app = path;` before the top one, so the future owns what it reads, the same
// bytes through the same bridge. ❌ Nothing else in the body moves.
func nestedPreludeEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	if d.code() != "E0515" {
		return nil
	}
	m := returnsLocalStd.FindStringSubmatch(d.Message)
	if m == nil {
		return nil
	}
	name := m[1]
	fn := src.methodAt(p.ByteStart)
	if fn == nil {
		return nil
	}
	kind := paramKind(fn, name)
	span := preludeSpan(src, fn, name)
	if kind == "" || span == nil || span[1] > p.ByteStart {
		return nil
	}
	edits := nestedBlockEdits(src, p, name, kind)
	if len(edits) == 0 {
		return nil
	}
	keep := "let " + name + "_app = " + name + ";"
	if !strings.HasSuffix(strings.TrimRight(string(src.text[fn.bodyStart:span[0]]), " \t\n"), keep) {
		edits = append(edits, edit{file: src.path, start: span[0], end: span[0], text: "\n" + keep, rule: ruleNestedPrelude})
	}
	return edits
}

// paramKind is typedParamKind of fn's param called name, or "".
func paramKind(fn *implMethod, name string) string {
	for _, prm := range fn.params {
		if prm.name == name {
			return typedParamKind(prm.typ)
		}
	}
	return ""
}

// nestedBlockEdits gives each outermost `async move { … }` in p that reads name its prelude;
// none when a block runs past p.
func nestedBlockEdits(src *source, p *diagSpan, name, kind string) []edit {
	var edits []edit
	for k := p.ByteStart; k < p.ByteEnd; {
		loc := asyncMoveOpen.FindIndex(src.masked[k:p.ByteEnd])
		if loc == nil {
			break
		}
		open := k + loc[1] // just past the block's `{`
		end := blockClose(src.masked, open)
		if end < 0 || end > p.ByteEnd {
			return nil
		}
		if len(identUses(src.masked, open, end, name)) > 0 {
			edits = append(edits, edit{file: src.path, start: open, end: open, text: nestedPreludeText(name, kind), rule: ruleNestedPrelude})
		}
		k = end + 1
	}
	return edits
}

func nestedPreludeText(name, kind string) string {
	if kind == "batch" {
		return fmt.Sprintf("\nlet %[1]s_std: Vec<PathBuf> = %[1]s_app.iter().map(|p| p.as_std_legacy().into_owned()).collect();\nlet %[1]s: &[PathBuf] = &%[1]s_std;", name)
	}
	return fmt.Sprintf("\nlet %[1]s_std = %[1]s_app.as_std_legacy();\nlet %[1]s: &Path = &%[1]s_std;", name)
}

// blockClose is the offset of the `}` closing the block whose body starts at open, or -1.
func blockClose(masked []byte, open int) int {
	depth := 0
	for k := open; k < len(masked); k++ {
		switch masked[k] {
		case '{':
			depth++
		case '}':
			if depth == 0 {
				return k
			}
			depth--
		}
	}
	return -1
}
