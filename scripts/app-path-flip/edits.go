package main

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
)

// roundEdits turns one round's diagnostics into a non-overlapping, deduplicated, ordered set
// of edits. Two edits over overlapping bytes keep the first (by start, then the longer); the
// other comes back next round against the new text, so overlap only delays, never loses.
func roundEdits(repo, crate string, crateRust, workspaceRust []string, diags []diagnostic) ([]edit, error) {
	cache := map[string]*source{}
	r := &rewriter{crate: crate, crateRust: crateRust, workspaceRust: workspaceRust, load: func(file string) (*source, error) {
		if s, ok := cache[file]; ok {
			return s, nil
		}
		text, err := os.ReadFile(filepath.Join(repo, file))
		if err != nil {
			return nil, err
		}
		s := newSource(file, text)
		cache[file] = s
		return s, nil
	}, readRaw: func(file string) ([]byte, error) { return os.ReadFile(filepath.Join(repo, file)) }}
	retyping, err := methodsBeingRetyped(r, diags)
	if err != nil {
		return nil, err
	}
	var all []edit
	for i := range diags {
		// A method whose signature an E0053 retypes this round still type-checks its body
		// against the OLD signature, so its body's errors are stale: they come back right
		// (or not at all, for a pure delegation) next round. Any fn, not only one a
		// `Volume` impl block holds: a `macro_rules!` body's methods (`forward_to_inner!`)
		// are none, and bridging their stale errors the same round as the retype left a
		// round trip (`&AppPath::legacy_unchecked(&path.as_std_legacy())`).
		p := diags[i].primary()
		if src, err := r.load(p.FileName); err == nil && diags[i].code() != "E0053" {
			if m := src.fnAt(p.ByteStart); m != nil && retyping[methodKey(p.FileName, m)] {
				continue
			}
		}
		edits, err := r.editsFor(&diags[i])
		if err != nil {
			return nil, err
		}
		all = append(all, edits...)
	}
	return chooseEdits(all), nil
}

func methodKey(file string, m *implMethod) string {
	return fmt.Sprintf("%s#%d", file, m.sigStart)
}

// methodsBeingRetyped are the fns an E0053 points into the signature of this round.
func methodsBeingRetyped(r *rewriter, diags []diagnostic) (map[string]bool, error) {
	retyping := map[string]bool{}
	for i := range diags {
		if diags[i].code() != "E0053" {
			continue
		}
		p := diags[i].primary()
		src, err := r.load(p.FileName)
		if err != nil {
			return nil, err
		}
		if m := src.fnWithSignatureAt(p.ByteStart); m != nil {
			retyping[methodKey(p.FileName, m)] = true
		}
	}
	return retyping, nil
}

func chooseEdits(all []edit) []edit {
	sort.SliceStable(all, func(i, j int) bool {
		a, b := all[i], all[j]
		if a.file != b.file {
			return a.file < b.file
		}
		if a.start != b.start {
			return a.start < b.start
		}
		if a.end != b.end {
			return a.end > b.end
		}
		return a.text < b.text
	})
	chosen := []edit{}
	for _, e := range all {
		if len(chosen) > 0 {
			prev := chosen[len(chosen)-1]
			if prev == e {
				continue
			}
			if prev.file == e.file && overlaps(prev, e) {
				continue
			}
		}
		chosen = append(chosen, e)
	}
	return chosen
}

// overlaps answers whether b (starting at or after a) touches a's bytes. Two inserts at one
// point both stand; an insert strictly inside a replaced range doesn't.
func overlaps(a, b edit) bool {
	if a.start == a.end && b.start == b.end {
		return false
	}
	if a.start == a.end {
		return b.start < a.start
	}
	return b.start < a.end
}

// applyEdits writes each file's edits back to front, so earlier offsets stay valid.
func applyEdits(repo string, edits []edit) error {
	byFile := map[string][]edit{}
	var files []string
	for _, e := range edits {
		if _, ok := byFile[e.file]; !ok {
			files = append(files, e.file)
		}
		byFile[e.file] = append(byFile[e.file], e)
	}
	for _, file := range files {
		path := filepath.Join(repo, file)
		text, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		fileEdits, ok := byFile[file]
		if !ok {
			continue
		}
		if err := os.WriteFile(path, applyToText(text, fileEdits), 0o644); err != nil {
			return err
		}
	}
	return nil
}

func applyToText(text []byte, edits []edit) []byte {
	out := append([]byte(nil), text...)
	for i := len(edits) - 1; i >= 0; i-- {
		e := edits[i]
		next := append([]byte(nil), out[:e.start]...)
		next = append(next, e.text...)
		out = append(next, out[e.end:]...)
	}
	return out
}
