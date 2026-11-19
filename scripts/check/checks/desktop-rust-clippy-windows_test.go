package checks

import (
	"strings"
	"testing"
)

// The cross-lint asks the Windows lane's question (`desktop-rust-clippy --ci` on a Windows
// host): every member declared for Windows, all targets, warnings denied, never a `--fix`.
// A Windows-only `unused_mut` in a test reached `main` because nothing local asked it.
func TestWindowsClippyAsksTheWindowsLanesQuestion(t *testing.T) {
	members, err := WorkspaceMembers(repoRootForTest(t))
	if err != nil {
		t.Fatalf("WorkspaceMembers: %v", err)
	}
	script := containerCargoScript(windowsClippyArgs(CargoSelectionArgs(members, "windows"))...)

	for _, want := range []string{
		"cargo 'clippy' '--locked' '--all-targets' '--target' 'x86_64-pc-windows-gnu' '--workspace'",
		"'--exclude' 'cmdr-fsevent-stream'",
		"'--' '-D' 'warnings'",
	} {
		if !strings.Contains(script, want) {
			t.Errorf("the Windows clippy command must contain %q, got:\n%s", want, script)
		}
	}
	if strings.Contains(script, "--fix") {
		t.Errorf("the Windows cross-lint reports; the host lane fixes:\n%s", script)
	}
}

// The container image is what makes the cross-lint possible: mingw-w64's C toolchain for the
// `-sys` crates' build scripts, the Rust target, and the capitalized `Windows.h` that `usearch`
// includes (Linux's disk is case-sensitive).
func TestLinuxImageCarriesTheWindowsCrossToolchain(t *testing.T) {
	dockerfile := linuxDockerfile("1.0.0", "1.0.0")
	for _, want := range []string{
		"gcc-mingw-w64-x86-64",
		"g++-mingw-w64-x86-64",
		"rustup target add " + windowsCrossTarget,
		"ln -s windows.h /usr/x86_64-w64-mingw32/include/Windows.h",
	} {
		if !strings.Contains(dockerfile, want) {
			t.Errorf("the Linux lanes' image must carry %q for the Windows cross-lint", want)
		}
	}
}
