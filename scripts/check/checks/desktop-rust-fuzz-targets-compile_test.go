package checks

import (
	"path/filepath"
	"slices"
	"testing"
)

// The check must stay a plain stable `cargo check` into its own target dir: a `+nightly`,
// a `fuzz` subcommand, or the shared `target/debug` would bring back the cost this lane
// exists to avoid.
func TestFuzzCompileCmd_PlainCheckIntoItsOwnTargetDir(t *testing.T) {
	cmd := fuzzCompileCmd("/repo")
	args := cmd.Args[1:]
	if args[0] != "check" {
		t.Errorf("want a plain `cargo check`, got %v", args)
	}
	for _, want := range []string{"--bins", "--locked", "--target-dir"} {
		if !slices.Contains(args, want) {
			t.Errorf("missing %s in %v", want, args)
		}
	}
	for i, arg := range args {
		if arg == "--target-dir" && args[i+1] != filepath.Join("/repo", "target", "fuzz-check") {
			t.Errorf("target dir = %q", args[i+1])
		}
	}
	if cmd.Dir != "/repo" {
		t.Errorf("runs from %q, want the repo root so rust-toolchain.toml applies", cmd.Dir)
	}
}

func TestRunFuzzTargetsCompile_NoTargetsSkips(t *testing.T) {
	result, err := RunFuzzTargetsCompile(&CheckContext{RootDir: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	if result.Code != ResultSkipped {
		t.Errorf("code = %v, want skipped", result.Code)
	}
}
