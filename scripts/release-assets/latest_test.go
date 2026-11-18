package main

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"os/exec"
	"slices"
	"strings"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

// The app's updater reads fixtureDir/latest.json too (`updater/manifest.rs`,
// `every_field_the_reader_names_is_one_the_writer_writes`): every field it reads must be a key
// written here, so a rename on either side fails a test.
const fixtureDir = "testdata/v0.61.0"

// v0.61.0's inputs, as its `publish` job had them: the signatures, the release's sizes, the notes,
// and the publication time.
func published(t *testing.T) (latestInput, string) {
	t.Helper()
	raw, err := os.ReadFile(fixtureDir + "/latest.json")
	must(t, err)
	var m struct {
		Notes   string `json:"notes"`
		PubDate string `json:"pub_date"`
	}
	must(t, json.Unmarshal(raw, &m))
	assets, err := readReleaseAssets(fixtureDir + "/release.json")
	must(t, err)
	return latestInput{
		release: manifest.For("0.61.0"), assets: assets, sigDir: fixtureDir,
		notes: m.Notes, pubDate: m.PubDate, baseURL: "https://download.getcmdr.com/v0.61.0",
		trustedKeyIDs: repoKeyIDs(t), newerThan: "0.60.0",
	}, string(raw)
}

// The manifest's latest.json is v0.61.0's published one, byte for byte, with `downloads` appended.
func TestLatestJSONIsThePublishedOnePlusDownloads(t *testing.T) {
	in, want := published(t)
	got, err := buildLatest(in)
	must(t, err)
	downloads := `,
  "downloads": [
    {
      "os": "macos",
      "arch": "aarch64",
      "kind": "dmg",
      "file": "Cmdr_0.61.0_aarch64.dmg",
      "size": 63540866
    },
    {
      "os": "macos",
      "arch": "x86_64",
      "kind": "dmg",
      "file": "Cmdr_0.61.0_x64.dmg",
      "size": 68516564
    },
    {
      "os": "macos",
      "arch": "universal",
      "kind": "dmg",
      "file": "Cmdr_0.61.0_universal.dmg",
      "size": 119705354
    },
    {
      "os": "macos",
      "arch": "universal",
      "kind": "pkg",
      "file": "Cmdr_0.61.0_universal.pkg",
      "size": 119649644
    }
  ]
}
`
	if wantAll := strings.TrimSuffix(want, "\n}\n") + downloads; got != wantAll {
		t.Fatalf("latest.json moved from v0.61.0's published bytes:\n%s", got)
	}
}

func TestLatestJSONLeavesOutAMissingPkg(t *testing.T) {
	in, _ := published(t)
	in.assets = slices.DeleteFunc(in.assets, func(a releaseAsset) bool { return strings.HasSuffix(a.Name, ".pkg") })
	got, err := buildLatest(in)
	must(t, err)
	if strings.Contains(got, ".pkg") {
		t.Fatalf("latest.json names a pkg the release doesn't have:\n%s", got)
	}
}

func TestLatestJSONRefusesAMissingFile(t *testing.T) {
	for _, name := range []string{"Cmdr_0.61.0_x64.dmg", "Cmdr_0.61.0_aarch64.app.tar.gz"} {
		in, _ := published(t)
		in.assets = slices.DeleteFunc(in.assets, func(a releaseAsset) bool { return a.Name == name })
		if _, err := buildLatest(in); err == nil || !strings.Contains(err.Error(), name) {
			t.Errorf("a release without %s gave a latest.json (%v)", name, err)
		}
	}
	in, _ := published(t)
	in.sigDir = t.TempDir()
	if _, err := buildLatest(in); err == nil || !strings.Contains(err.Error(), "no signature") {
		t.Errorf("missing signatures gave a latest.json (%v)", err)
	}
}

// windowsRelease is v0.61.0's inputs with Windows shipped: its files on the release, and every
// signed file's `.sig` (the version-binding ones with `version:` when withVersion).
func windowsRelease(t *testing.T, withVersion bool) latestInput {
	t.Helper()
	in, _ := published(t)
	in.release = manifest.ForOSes("0.61.0", manifest.MacOS, manifest.Windows)
	in.assets = append(slices.Clone(in.assets),
		releaseAsset{Name: "Cmdr_0.61.0_x64-setup.exe", Size: 5000000, State: "uploaded"}, releaseAsset{Name: "Cmdr_0.61.0_x64.msix", Size: 6000000, State: "uploaded"},
		releaseAsset{Name: "cmdr-group-policy-0.61.0.zip", Size: 7000, State: "uploaded"}, releaseAsset{Name: "Cmdr_0.61.0_security-advisories.json", Size: 400, State: "uploaded"})
	in.sigDir = t.TempDir()
	for _, f := range in.release.Signed() {
		src, err := os.ReadFile(fixtureDir + "/" + f.Signature())
		if err != nil || f.SignsVersion() {
			trusted := "timestamp:1\tfile:" + f.Name
			if withVersion && f.SignsVersion() {
				trusted += "\tversion:0.61.0"
			}
			src = []byte(fakeSignature(in.trustedKeyIDs[0], trusted))
		}
		must(t, os.WriteFile(in.sigDir+"/"+f.Signature(), src, 0o644))
	}
	return in
}

// Windows' rows reach `platforms`, `downloads`, and `securityAdvisories` with no code change.
func TestLatestJSONTakesTheWindowsRows(t *testing.T) {
	// Signed without `--app-version`, the Windows installer would be refused by every install.
	if _, err := buildLatest(windowsRelease(t, false)); err == nil || !strings.Contains(err.Error(), "Cmdr_0.61.0_x64-setup.exe's signature doesn't name version 0.61.0") {
		t.Fatalf("a Windows payload signed without its version gave a latest.json (%v)", err)
	}
	in := windowsRelease(t, true)
	got, err := buildLatest(in)
	must(t, err)
	var m struct {
		Platforms          map[string]struct{ URL string }   `json:"platforms"`
		Downloads          []struct{ OS, Kind, File string } `json:"downloads"`
		SecurityAdvisories struct{ URL, Signature string }   `json:"securityAdvisories"`
	}
	must(t, json.Unmarshal([]byte(got), &m))
	if m.Platforms["windows-x86_64"].URL != "https://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_x64-setup.exe" {
		t.Errorf("platforms = %+v", m.Platforms)
	}
	for _, want := range []struct{ kind, file string }{{"msix", "Cmdr_0.61.0_x64.msix"}, {"group-policy", "cmdr-group-policy-0.61.0.zip"}} {
		if !slices.ContainsFunc(m.Downloads, func(d struct{ OS, Kind, File string }) bool {
			return d.OS == "windows" && d.Kind == want.kind && d.File == want.file
		}) {
			t.Errorf("downloads = %+v, want %s", m.Downloads, want.file)
		}
	}
	sig, err := os.ReadFile(in.sigDir + "/Cmdr_0.61.0_security-advisories.json.sig")
	must(t, err)
	if m.SecurityAdvisories.URL != "https://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_security-advisories.json" || m.SecurityAdvisories.Signature != strings.TrimSpace(string(sig)) {
		t.Errorf("securityAdvisories = %+v", m.SecurityAdvisories)
	}
	// The member comes last, so everything before it is what a macOS-only release prints.
	if !strings.HasSuffix(got, "\"\n  }\n}\n") || strings.Index(got, `"securityAdvisories"`) < strings.Index(got, `"downloads"`) {
		t.Errorf("securityAdvisories isn't the last member:\n%s", got)
	}
}

// A release that ships the advisory list can't leave it out of `latest.json`, or point it at
// another file: Windows and Linux installs would hear of no advisory.
func TestValidateLatestHoldsTheAdvisoryEntryToTheList(t *testing.T) {
	in := windowsRelease(t, true)
	got, err := buildLatest(in)
	must(t, err)
	var doc map[string]json.RawMessage
	must(t, json.Unmarshal([]byte(got), &doc))
	delete(doc, "securityAdvisories")
	without, err := json.Marshal(doc)
	must(t, err)
	if err := validateLatest(without, in.rules()); err == nil || !strings.Contains(err.Error(), `no "securityAdvisories"`) {
		t.Errorf("a manifest without the release's advisory list passed: %v", err)
	}
	moved := strings.Replace(got, "Cmdr_0.61.0_security-advisories.json\"", "Cmdr_0.61.0_x64.msix\"", 1)
	if err := validateLatest([]byte(moved), in.rules()); err == nil || !strings.Contains(err.Error(), "the release's advisory list is Cmdr_0.61.0_security-advisories.json") {
		t.Errorf("an advisory entry naming another file passed: %v", err)
	}
}

// writeString prints what `jq -Rs .` prints for every ASCII character, the line separators Go's
// encoder escapes, non-ASCII text, and invalid UTF-8.
func TestWriteStringMatchesJq(t *testing.T) {
	var in strings.Builder
	for c := 1; c < 0x80; c++ {
		in.WriteByte(byte(c))
	}
	in.WriteString("   <a href=\"x\">&amp;</a> ⌘⌥ – 😅 \xff\xfe end")
	cmd := exec.Command("jq", "-Rs", ".")
	cmd.Stdin = strings.NewReader(in.String())
	want, err := cmd.Output()
	must(t, err)
	var got strings.Builder
	writeString(&got, in.String())
	if got.String()+"\n" != string(want) {
		t.Fatalf("writeString =\n%s\njq =\n%s", got.String(), want)
	}
}

// Off macOS the updater binds the version to the signature's trusted comment, so `latest-json`
// refuses to point every Windows and Linux install at a payload signed without `--app-version`.
func TestSignatureNamesVersion(t *testing.T) {
	signed := func(trusted string) string {
		text := "untrusted comment: signature from tauri secret key\nRUSIG\ntrusted comment: " + trusted + "\nGLOBALSIG\n"
		return base64.StdEncoding.EncodeToString([]byte(text))
	}
	withVersion := signed("timestamp:1794957084\tfile:Cmdr_0.70.0_x64-setup.exe\tversion:0.70.0")
	if !signatureNamesVersion(withVersion, "0.70.0") {
		t.Error("a signature made with --app-version 0.70.0 should name 0.70.0")
	}
	if signatureNamesVersion(withVersion, "0.7.0") {
		t.Error("a signature naming 0.70.0 must not pass for 0.7.0")
	}
	macOS, err := os.ReadFile(fixtureDir + "/Cmdr_0.61.0_aarch64.app.tar.gz.sig")
	if err != nil {
		t.Fatal(err)
	}
	if signatureNamesVersion(string(macOS), "0.61.0") {
		t.Error("v0.61.0's macOS signatures were made without --app-version, so they name no version")
	}
	if signatureNamesVersion("not base64!", "0.70.0") {
		t.Error("an undecodable signature names nothing")
	}
}
