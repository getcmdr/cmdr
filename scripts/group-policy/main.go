// Group Policy template: generates Cmdr's ADMX and en-US ADML files from the app's policy
// definition, `apps/desktop/src-tauri/src/managed_policy/policies.json`, so the template can't
// drift from the keys and registry path the app reads. Layout, schema notes, and where the files
// ship: `apps/desktop/packaging/group-policy/DETAILS.md`.
//
// # Usage
//
//	./scripts/group-policy.sh          # write the template files
//	./scripts/group-policy.sh -check   # exit 3 when a committed file differs from what it generates
//
// The `desktop-group-policy-fresh` check runs it.
package main

import (
	"bytes"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
)

const (
	definitionPath = "apps/desktop/src-tauri/src/managed_policy/policies.json"
	outputDir      = "apps/desktop/packaging/group-policy"
	// exitDrift tells the check "the committed files are stale", apart from a generator failure.
	exitDrift = 3
)

func main() {
	root := flag.String("root", "", "the repo root (scripts/group-policy.sh sets it)")
	check := flag.Bool("check", false, "compare the committed files instead of writing them")
	flag.Parse()
	if *root == "" {
		fmt.Fprintln(os.Stderr, "usage: group-policy -root <repo root> [-check]")
		os.Exit(2)
	}
	stale, err := run(*root, *check)
	if err != nil {
		fmt.Fprintln(os.Stderr, "group-policy:", err)
		os.Exit(1)
	}
	if *check && len(stale) > 0 {
		fmt.Fprintln(os.Stderr, "group-policy: these files differ from what policies.json generates. Run ./scripts/group-policy.sh and commit them:")
		for _, path := range stale {
			fmt.Fprintln(os.Stderr, "  "+path)
		}
		os.Exit(exitDrift)
	}
}

// run generates the template and either writes it or compares it. It returns the repo-relative
// paths whose committed bytes differ from the generated ones (all of them written, unless check).
func run(root string, check bool) ([]string, error) {
	raw, err := os.ReadFile(filepath.Join(root, definitionPath))
	if err != nil {
		return nil, err
	}
	def, err := parseDefinition(raw)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", definitionPath, err)
	}
	files, err := generate(def)
	if err != nil {
		return nil, err
	}
	var stale []string
	for _, file := range files {
		rel := filepath.ToSlash(filepath.Join(outputDir, file.path))
		full := filepath.Join(root, rel)
		current, err := os.ReadFile(full)
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			return nil, err
		}
		if bytes.Equal(current, file.content) {
			continue
		}
		stale = append(stale, rel)
		if check {
			continue
		}
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			return nil, err
		}
		if err := os.WriteFile(full, file.content, 0o644); err != nil {
			return nil, err
		}
		fmt.Println("wrote", rel)
	}
	return stale, nil
}
