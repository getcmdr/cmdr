package checks

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// fuzzCompileTargetDir is where the compile check builds, relative to the repo root: a
// target dir of its own inside the root `target/`. Its own, because the fuzz crate is
// its own workspace with its own lock, profile, and feature set, so not one of its ~320
// units matches a root lane's (measured 2026-11-18: a check against the warm shared
// `target/debug` recompiled all of them, the same as a cold dir), and sharing would only
// park its units beside the root's under cargo's build-directory lock. Inside `target/`,
// because that's what rust-cache saves in CI (it walks a nested target dir like a profile
// dir) and what a new worktree clones warm.
var fuzzCompileTargetDir = filepath.Join("target", "fuzz-check")

// RunFuzzTargetsCompile type-checks every fuzz target (`cargo check` of the `fuzz/`
// crate's bins) on the pinned stable toolchain: no nightly, no cargo-fuzz, no sanitizer
// instrumentation, no fuzzing. A plain check proves what the landing path needs: that
// each target still compiles against the crates it calls.
//
// `fuzz/` sits outside the workspace, so no other landing lane compiles it, and an API
// change in a crate a target calls (`ArchiveIndex::open_read` gaining an argument) used to
// surface only in the daily `desktop-rust-fuzz` run, a day after it landed.
//
// No debuginfo and no incremental state: nothing runs these artifacts, and both only grow
// the target dir (515 MB -> 358 MB cold). The env applies to this command alone, so the
// instrumented `cargo fuzz` build keeps its own settings.
func RunFuzzTargetsCompile(ctx *CheckContext) (CheckResult, error) {
	targets, err := fuzzTargetNames(ctx.RootDir)
	if err != nil {
		return CheckResult{}, err
	}
	if len(targets) == 0 {
		return Skipped("no fuzz targets ([[bin]] entries in fuzz/Cargo.toml)"), nil
	}

	if output, err := RunCommand(fuzzCompileCmd(ctx.RootDir), true); err != nil {
		return CheckResult{}, fmt.Errorf("a fuzz target doesn't compile against the crates it calls "+
			"(`cargo check --manifest-path fuzz/Cargo.toml --bins` reproduces it)\n%s", indentOutput(output))
	}
	return Success(fmt.Sprintf("%d fuzz %s compile", len(targets), Pluralize(len(targets), "target", "targets"))), nil
}

// fuzzCompileCmd is the quiet `cargo check` of every fuzz bin into fuzzCompileTargetDir.
// `--quiet` drops cargo's per-crate progress (320 lines cold), so a failure prints the
// compiler's errors and nothing else; `--message-format short` keeps each to its line.
func fuzzCompileCmd(rootDir string) *exec.Cmd {
	cmd := exec.Command("cargo", "check", "--quiet", "--locked", "--bins", "--message-format", "short",
		"--manifest-path", filepath.Join("fuzz", "Cargo.toml"),
		"--target-dir", filepath.Join(rootDir, fuzzCompileTargetDir))
	// From the root, so `rust-toolchain.toml` picks the pinned stable.
	cmd.Dir = rootDir
	cmd.Env = append(os.Environ(), "CARGO_PROFILE_DEV_DEBUG=false", "CARGO_INCREMENTAL=0")
	return cmd
}
