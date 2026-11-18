package main

import (
	"os"
	"strings"
	"testing"
)

// `guard` and `publish` build this module on every macOS release, in jobs that hold a write
// token, so it stays stdlib-only: a third-party module here would be a new trust edge and a new
// way for `proxy.golang.org` to stop a release. Windows-only code that needs one lives in its own
// module (`scripts/release-nsis/`).
func TestTheModuleNeedsNothingOutsideTheStandardLibrary(t *testing.T) {
	raw, err := os.ReadFile("go.mod")
	must(t, err)
	if strings.Contains(string(raw), "require") {
		t.Fatalf("scripts/release-assets/go.mod requires a module:\n%s", raw)
	}
}
