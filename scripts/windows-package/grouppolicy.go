package main

import (
	"archive/zip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"
)

// zipGroupPolicy writes the Group Policy template as admins copy it into `PolicyDefinitions`:
// every `.admx` at the top, and each language folder's `.adml` files under it (`en-US/`). The
// folder's docs stay out. Entries are sorted and carry a fixed time, so the same files always
// make the same bytes (and the same checksum).
func zipGroupPolicy(dir, out string) error {
	names, err := templateFiles(dir)
	if err != nil {
		return err
	}
	f, err := os.Create(out)
	if err != nil {
		return err
	}
	if err := writeZip(f, dir, names); err != nil {
		_ = f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	fmt.Printf("%s: %s\n", filepath.Base(out), strings.Join(names, ", "))
	return nil
}

// templateFiles are the template's files under dir, `/`-separated and sorted.
func templateFiles(dir string) ([]string, error) {
	var names []string
	err := filepath.WalkDir(dir, func(path string, d os.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		rel, err := filepath.Rel(dir, path)
		if err != nil {
			return err
		}
		rel = filepath.ToSlash(rel)
		admx := !strings.Contains(rel, "/") && strings.HasSuffix(rel, ".admx")
		adml := strings.Count(rel, "/") == 1 && strings.HasSuffix(rel, ".adml")
		if admx || adml {
			names = append(names, rel)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	hasExt := func(ext string) bool {
		return slices.ContainsFunc(names, func(n string) bool { return strings.HasSuffix(n, ext) })
	}
	if !hasExt(".admx") || !hasExt(".adml") {
		return nil, fmt.Errorf("%s holds no ADMX and ADML files to ship", dir)
	}
	slices.Sort(names)
	return names, nil
}

func writeZip(w io.Writer, dir string, names []string) error {
	zw := zip.NewWriter(w)
	for _, name := range names {
		entry, err := zw.CreateHeader(&zip.FileHeader{Name: name, Method: zip.Deflate, Modified: time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)})
		if err != nil {
			return err
		}
		src, err := os.Open(filepath.Join(dir, filepath.FromSlash(name)))
		if err != nil {
			return err
		}
		_, err = io.Copy(entry, src)
		_ = src.Close()
		if err != nil {
			return err
		}
	}
	return zw.Close()
}
