package main

import (
	"archive/zip"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func repoRoot(t *testing.T) string {
	t.Helper()
	root, err := filepath.Abs("../..")
	must(t, err)
	return root
}

// fakeRoot is a repo root holding the real manifest, identity file, Tauri config, and logos,
// plus the resource the build's `beforeBuildCommand` would have fetched.
func fakeRoot(t *testing.T) string {
	t.Helper()
	real, root := repoRoot(t), t.TempDir()
	files := []string{msixDir + "/Package.appxmanifest", msixDir + "/identity.json", srcTauriDir + "/tauri.conf.json"}
	for _, logo := range logos {
		files = append(files, srcTauriDir+"/icons/"+logo)
	}
	for _, f := range files {
		must(t, copyFile(filepath.Join(real, f), filepath.Join(root, f)))
	}
	must(t, os.MkdirAll(filepath.Join(root, srcTauriDir, "resources/ai"), 0o755))
	must(t, os.WriteFile(filepath.Join(root, srcTauriDir, "resources/ai/llama-server"), nil, 0o644))
	return root
}

func fakeExe(t *testing.T, marker string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), exeName)
	must(t, os.WriteFile(path, []byte("MZ...static "+marker+" ..."), 0o644))
	return path
}

// zipPack stands in for winapp: it zips the staged folder the way MakeAppx lays a package out,
// with the manifest as `AppxManifest.xml`.
func zipPack(stage, manifest, out string) error {
	f, err := os.Create(out)
	if err != nil {
		return err
	}
	zw := zip.NewWriter(f)
	add := func(path, name string) error {
		w, err := zw.Create(name)
		if err != nil {
			return err
		}
		src, err := os.Open(path)
		if err != nil {
			return err
		}
		defer func() { _ = src.Close() }()
		_, err = io.Copy(w, src)
		return err
	}
	err = filepath.WalkDir(stage, func(path string, d os.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		rel, _ := filepath.Rel(stage, path)
		return add(path, filepath.ToSlash(rel))
	})
	if err != nil {
		return err
	}
	if err := add(manifest, "AppxManifest.xml"); err != nil {
		return err
	}
	if err := zw.Close(); err != nil {
		return err
	}
	return f.Close()
}

// The committed release identity is still Partner Center's placeholder, so a release build
// fails before packing anything, naming the step that fixes it.
func TestTheReleaseIdentityFailsLoudlyWhileItsAPlaceholder(t *testing.T) {
	_, err := loadIdentity(repoRoot(t), "release")
	if err == nil || !strings.Contains(err.Error(), "copy it from Partner Center") {
		t.Fatalf("a placeholder release identity loaded: %v", err)
	}
	if _, err := loadIdentity(repoRoot(t), "dev"); err != nil {
		t.Fatalf("the dev identity: %v", err)
	}
}

func TestAnMSIXHoldsTheUnstampedExeTheResourcesAndTheVersion(t *testing.T) {
	root := fakeRoot(t)
	id, err := loadIdentity(root, "dev")
	must(t, err)
	out := filepath.Join(t.TempDir(), "Cmdr_0.70.0_x64.msix")
	must(t, buildMSIX(msixInput{root: root, exe: fakeExe(t, string(unstampedMarker)), out: out, identity: id, version: "0.70.0.0", pack: zipPack}))

	zr, err := zip.OpenReader(out)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = zr.Close() }()
	var names []string
	for _, f := range zr.File {
		names = append(names, f.Name)
	}
	for _, want := range []string{"AppxManifest.xml", "Cmdr.exe", "resources/ai/llama-server", "Assets/StoreLogo.png"} {
		if !slices.Contains(names, want) {
			t.Errorf("the package has no %s: %v", want, names)
		}
	}
}

// An exe `tauri bundle` already stamped as NSIS would run the NSIS updater inside the Store's
// package, which the Store must update instead.
func TestAnMSIXRefusesAStampedExe(t *testing.T) {
	root := fakeRoot(t)
	id, err := loadIdentity(root, "dev")
	must(t, err)
	err = buildMSIX(msixInput{root: root, exe: fakeExe(t, "__TAURI_BUNDLE_TYPE_VAR_NSS"), out: filepath.Join(t.TempDir(), "x.msix"), identity: id, version: "0.70.0.0", pack: zipPack})
	if err == nil || !strings.Contains(err.Error(), "__TAURI_BUNDLE_TYPE_VAR_NSS stamp") {
		t.Fatalf("a stamped exe was packaged: %v", err)
	}
}

func TestVerifyMSIXCatchesAWrongPackage(t *testing.T) {
	root := fakeRoot(t)
	id, err := loadIdentity(root, "dev")
	must(t, err)
	exe := fakeExe(t, string(unstampedMarker))
	cases := []struct {
		name   string
		tamper func(stage, manifest string)
		want   string
	}{
		{"a registry hive", func(stage, _ string) { must(t, os.WriteFile(filepath.Join(stage, "Registry.dat"), nil, 0o644)) }, "registry keys"},
		{"another version", func(_, manifest string) {
			raw, _ := os.ReadFile(manifest)
			must(t, os.WriteFile(manifest, []byte(strings.Replace(string(raw), `Version="0.70.0.0"`, `Version="0.69.0.0"`, 1)), 0o644))
		}, `its version is "0.69.0.0"`},
		// winapp packs every file in its folder, which once shipped the template beside the real
		// manifest (seen on the builder, 2026-11-18).
		{"the template inside the package", func(stage, manifest string) {
			must(t, copyFile(manifest, filepath.Join(stage, "Package.appxmanifest")))
		}, "manifest template"},
	}
	for _, c := range cases {
		pack := func(stage, manifest, out string) error {
			c.tamper(stage, manifest)
			return zipPack(stage, manifest, out)
		}
		err := buildMSIX(msixInput{root: root, exe: exe, out: filepath.Join(t.TempDir(), "x.msix"), identity: id, version: "0.70.0.0", pack: pack})
		if err == nil || !strings.Contains(err.Error(), c.want) {
			t.Errorf("%s: got %v, want %q", c.name, err, c.want)
		}
	}
}

func TestPackageVersionRefusesAPrerelease(t *testing.T) {
	if v, err := packageVersion("0.70.0"); err != nil || v != "0.70.0.0" {
		t.Fatalf("0.70.0 → %q, %v", v, err)
	}
	for _, bad := range []string{"0.70.0-beta.1", "v0.70.0", "0.70"} {
		if _, err := packageVersion(bad); err == nil {
			t.Errorf("%q passed", bad)
		}
	}
}

// The zip holds what admins copy into PolicyDefinitions, and nothing else from the folder.
func TestTheGroupPolicyZipHoldsTheTemplateOnly(t *testing.T) {
	out := filepath.Join(t.TempDir(), "cmdr-group-policy-0.70.0.zip")
	must(t, zipGroupPolicy(filepath.Join(repoRoot(t), groupPolicyDir), out))
	zr, err := zip.OpenReader(out)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = zr.Close() }()
	var names []string
	for _, f := range zr.File {
		names = append(names, f.Name)
	}
	if want := []string{"cmdr.admx", "en-US/cmdr.adml", "en-US/rymd.adml", "rymd.admx"}; !slices.Equal(names, want) {
		t.Fatalf("the zip holds %v, want %v", names, want)
	}
	first, err := os.ReadFile(out)
	must(t, err)
	must(t, zipGroupPolicy(filepath.Join(repoRoot(t), groupPolicyDir), out))
	second, err := os.ReadFile(out)
	must(t, err)
	if string(first) != string(second) {
		t.Error("two zips of the same files differ, so the checksum would too")
	}
}
