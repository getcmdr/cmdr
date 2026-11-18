package main

import (
	"encoding/base64"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// tauriSig is a signature shaped like `tauri signer sign`'s, with the given trusted comment. Its
// bytes don't verify; `namesVersion` reads the field only, after `verifyTauriSignature` passed.
func tauriSig(comment string) string {
	body := base64.StdEncoding.EncodeToString(append([]byte("ED"), make([]byte, 8+64)...))
	global := base64.StdEncoding.EncodeToString(make([]byte, 64))
	text := "untrusted comment: signature from tauri secret key\n" + body + "\ntrusted comment: " + comment + "\n" + global + "\n"
	return base64.StdEncoding.EncodeToString([]byte(text))
}

func TestNamesVersionReadsTheTrustedCommentsField(t *testing.T) {
	if !namesVersion(tauriSig("timestamp:1\tfile:Cmdr_0.70.0_x64-setup.exe\tversion:0.70.0"), "0.70.0") {
		t.Error("a signature naming 0.70.0 didn't")
	}
	for _, comment := range []string{"timestamp:1\tfile:Cmdr_0.70.0_x64-setup.exe", "timestamp:1\tversion:0.69.0", "timestamp:1\tversion:0.70.0x"} {
		if namesVersion(tauriSig(comment), "0.70.0") {
			t.Errorf("%q named 0.70.0", comment)
		}
	}
}

// A Windows installer signed without `--app-version` fails the check CI and the laptop run before
// any signature goes up; the advisory list and macOS archives carry no version field.
func TestRequireVersionFieldsRefusesAVersionlessInstallerSignature(t *testing.T) {
	dir := t.TempDir()
	write := func(name, comment string) {
		if err := os.WriteFile(filepath.Join(dir, name+".sig"), []byte(tauriSig(comment)), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	names := legSigned(windowsOn, "windows")
	write("Cmdr_0.70.0_security-advisories.json", "timestamp:1\tfile:Cmdr_0.70.0_security-advisories.json")
	write("Cmdr_0.70.0_x64-setup.exe", "timestamp:1\tfile:Cmdr_0.70.0_x64-setup.exe")
	if err := requireVersionFields(windowsOn, dir, names); err == nil || !strings.Contains(err.Error(), "doesn't name version 0.70.0") {
		t.Fatalf("a versionless installer signature passed: %v", err)
	}
	write("Cmdr_0.70.0_x64-setup.exe", "timestamp:1\tfile:Cmdr_0.70.0_x64-setup.exe\tversion:0.70.0")
	if err := requireVersionFields(windowsOn, dir, names); err != nil {
		t.Fatal(err)
	}
}
