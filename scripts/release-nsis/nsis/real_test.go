package nsis

import (
	"os"
	"strings"
	"testing"
)

// A real Tauri NSIS installer, when `CMDR_REAL_NSIS` names one of 0.61.0 (the Windows builder's
// `D:\targets\t52\release\bundle\nsis\Cmdr_0.61.0_x64-setup.exe`, 37 MiB, too big to commit): the
// vet passes it against the repo's Tauri config and refuses it as any other version. About 40 s
// per pass on an M3, most of it decompressing the 150 MiB app.
// (Last run 2026-11-18 on tauri-cli 2.12's build of 3224901455: passed.)
func TestARealInstaller(t *testing.T) {
	path := os.Getenv("CMDR_REAL_NSIS")
	if path == "" {
		t.Skip("set CMDR_REAL_NSIS to a Tauri NSIS installer of 0.61.0 to run this")
	}
	want, err := ExpectFromRepo("../../..", "0.61.0")
	must(t, err)
	must(t, Vet(path, want))
	want.Version = "0.62.0"
	if err := Vet(path, want); err == nil || !strings.Contains(err.Error(), "says 0.61.0.0, not 0.62.0.0") {
		t.Fatalf("0.61.0's installer passed as 0.62.0: %v", err)
	}
}
