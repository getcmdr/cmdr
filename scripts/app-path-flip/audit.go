package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// auditImports lists every line of the dirs' Rust files that is one of the imports the
// codemod adds (`wantedImports`) and sits right under an outer attribute line (`#[cfg(…)]`,
// the last line of a multi-line one): the shape of an import inserted between an item and
// its attribute, which gates the import and ungates the item. A hit is a lead, not a
// verdict (a hand-written `#[cfg(test)] use std::path::Path;` is fine): check it against
// the base. Answers the exit code: 1 when anything is listed.
func auditImports(root string, dirs []string) int {
	repo, err := repoRoot(root)
	if err != nil {
		fail(err)
	}
	lines := map[string]bool{}
	for _, want := range wantedImports {
		for _, crate := range []string{"crate", "cmdr_fs"} {
			lines[strings.TrimSpace(strings.ReplaceAll(want, "{crate}", crate))] = true
		}
	}
	hits := 0
	for _, dir := range dirs {
		files, err := listRust(repo, dir)
		if err != nil {
			fail(err)
		}
		for _, file := range files {
			text, err := os.ReadFile(filepath.Join(repo, file))
			if err != nil {
				fail(err)
			}
			for _, at := range importsUnderAttributes(string(text), lines) {
				fmt.Printf("%s:%d: %s\n", file, at.line, at.text)
				hits++
			}
		}
	}
	if hits > 0 {
		return 1
	}
	return 0
}

type auditHit struct {
	line int
	text string
}

func importsUnderAttributes(text string, imports map[string]bool) []auditHit {
	var hits []auditHit
	all := strings.Split(text, "\n")
	for i := 1; i < len(all); i++ {
		line := strings.TrimSpace(all[i])
		if !imports[line] {
			continue
		}
		prev := strings.TrimSpace(all[i-1])
		if strings.HasSuffix(prev, "]") && !strings.HasPrefix(prev, "#![") && (strings.HasPrefix(prev, "#[") || strings.HasSuffix(prev, ")]")) {
			hits = append(hits, auditHit{line: i + 1, text: line})
		}
	}
	return hits
}
