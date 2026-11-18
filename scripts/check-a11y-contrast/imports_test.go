package main

import (
	"os"
	"path/filepath"
	"testing"
)

// The token files `app.css` imports (`app-controls.css`, `app-menu.css`, `app-palette.css`) define
// house tokens the components read, so the var table must see them; their look-keyed blocks
// (`:root[data-look='windows']`) must not leak into the house values.
func TestInlineLocalImportsFeedsHouseTokensOnly(t *testing.T) {
	dir := t.TempDir()
	tokens := `:root { --color-x: #111111; }
:root[data-look='windows'] { --color-x: #222222; }
@media (prefers-color-scheme: dark) {
  :root { --color-x: #333333; }
}
@media (prefers-color-scheme: dark) {
  :root[data-look='windows'] { --color-x: #444444; }
}`
	if err := os.WriteFile(filepath.Join(dir, "tokens.css"), []byte(tokens), 0o600); err != nil {
		t.Fatal(err)
	}
	app := "@import './tokens.css';\n:root { --color-y: var(--color-x); }\n"

	inlined, err := inlineLocalImports(dir, app)
	if err != nil {
		t.Fatal(err)
	}
	vars := ParseAppCSS(inlined)
	if got := vars.Light["color-x"]; got != "#111111" {
		t.Errorf("light --color-x = %q, want #111111", got)
	}
	if got := vars.Dark["color-x"]; got != "#333333" {
		t.Errorf("dark --color-x = %q, want #333333", got)
	}
	if got := vars.Light["color-y"]; got != "var(--color-x)" {
		t.Errorf("light --color-y = %q, want var(--color-x)", got)
	}
}

func TestInlineLocalImportsReportsAMissingFile(t *testing.T) {
	if _, err := inlineLocalImports(t.TempDir(), "@import './missing.css';\n"); err == nil {
		t.Error("want an error for a missing import")
	}
}
