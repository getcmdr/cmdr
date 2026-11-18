package main

import (
	"regexp"
	"strings"
)

// The lib phase: `app_path!` lives behind `cmdr-fs/testing`, and every `--all-targets` build
// unifies that feature in through the dev-dependencies, so the literal rule sees the macro
// compile in code a plain build of the crate compiles without it (a feature that doesn't
// enable `cmdr-fs/testing`, like `cmdr-operations`'s `golden`, which the browser demo's wasm
// build turns on). The lib phase builds the crate's normal targets with the same
// `-cargo-args` (no dev-dependencies, so no unified `testing`), and each `app_path!` rustc
// can't find there crosses through the bridge, the literal rule's other form.

var missingAppPathMacro = regexp.MustCompile("^cannot find macro `app_path` in this scope$")

// appPathLiteralAt matches the invocation the literal rule wrote, from its name on.
var appPathLiteralAt = regexp.MustCompile(`^app_path!\(\s*"([^"\\]*)"\s*\)`)

// libPhaseEdits answers one diagnostic of the normal-targets build: an `app_path!` that build
// can't find becomes `&AppPath::legacy_unchecked(Path::new("…"))`, and the names that form
// needs are imported. Anything else there is residue: the two builds differ only by features.
func (r *rewriter) libPhaseEdits(d *diagnostic) ([]edit, error) {
	p := d.primary()
	src, err := r.load(p.FileName)
	if err != nil {
		return nil, err
	}
	if p.ByteEnd > len(src.text) {
		return nil, nil
	}
	if d.code() == "unused_imports" {
		// The `app_path` import the bridged literals emptied (an error where the crate
		// denies the lint). An import a test target still reads comes back from the
		// all-targets build's import rule, which runs after this phase.
		return unusedImportEdits(d, p.FileName), nil
	}
	if d.Level != "error" {
		return nil, nil
	}
	if missingAppPathMacro.MatchString(d.Message) {
		return macroFallbackEdits(src, p), nil
	}
	if m := notFoundName.FindStringSubmatch(d.Message); m != nil && m[1] != "app_path" {
		return importEdits(d, src, r.crate), nil
	}
	return nil, nil
}

// macroFallbackEdits rewrites `app_path!("/a")` at the span rustc names to the bridge form.
// The literal is canonical (the literal rule wrote it), so the bridge wraps the same text.
func macroFallbackEdits(src *source, p *diagSpan) []edit {
	m := appPathLiteralAt.FindSubmatchIndex(src.text[p.ByteStart:])
	if m == nil || strings.TrimSpace(string(src.text[p.ByteStart:p.ByteEnd])) != "app_path" {
		return nil
	}
	literal := string(src.text[p.ByteStart+m[2] : p.ByteStart+m[3]])
	return []edit{{file: src.path, start: p.ByteStart, end: p.ByteStart + m[1],
		text: `&AppPath::legacy_unchecked(Path::new("` + literal + `"))`, rule: ruleLiteralNoMacro}}
}

// libPhaseRoundEdits is roundEdits for the lib phase.
func libPhaseRoundEdits(repo, crate string, crateRust, workspaceRust []string, diags []diagnostic) ([]edit, error) {
	r := newRewriter(repo, crate, crateRust, workspaceRust)
	var all []edit
	for i := range diags {
		edits, err := r.libPhaseEdits(&diags[i])
		if err != nil {
			return nil, err
		}
		all = append(all, edits...)
	}
	return chooseEdits(all), nil
}
