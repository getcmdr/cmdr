package main

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
)

// localImport matches a relative `@import './x.css';` line, the form app.css uses for its token
// files (`app-palette.css`, `app-menu.css`, `app-controls.css`).
var localImport = regexp.MustCompile(`(?m)^@import\s+['"](\./[^'"]+\.css)['"]\s*;`)

// loadVarTable builds the var table from app.css and the token files it imports, exiting on a
// missing import (a broken import is a broken app, not a contrast finding).
func loadVarTable(appCSSPath, content string) *VarTable {
	tokenCSS, err := inlineLocalImports(filepath.Dir(appCSSPath), content)
	if err != nil {
		fmt.Fprintf(os.Stderr, "%sError reading app.css imports: %v%s\n", colorRed, err, colorReset)
		os.Exit(1)
	}
	return ParseAppCSS(tokenCSS)
}

// inlineLocalImports replaces each local `@import` in content with that file's own text
// (recursively), so the var table sees the house tokens the token files define. Their look-keyed
// blocks (`:root[data-look='windows']`) stay out of it, because ParseAppCSS only reads a bare
// `:root`: the check models the house look.
func inlineLocalImports(dir, content string) (string, error) {
	var firstErr error
	inlined := localImport.ReplaceAllStringFunc(content, func(line string) string {
		rel := localImport.FindStringSubmatch(line)[1]
		path := filepath.Join(dir, rel)
		body, err := os.ReadFile(path)
		if err != nil {
			if firstErr == nil {
				firstErr = fmt.Errorf("read %s: %w", rel, err)
			}
			return ""
		}
		nested, err := inlineLocalImports(filepath.Dir(path), string(body))
		if err != nil && firstErr == nil {
			firstErr = err
		}
		return nested
	})
	return inlined, firstErr
}
