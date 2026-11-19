package checks

import (
	"fmt"
	"runtime"
)

// windowsCrossTarget is the triple the Windows cross-lint checks from a Mac or Linux box:
// mingw-w64's, which a Linux container cross-compiles C for out of the box. CI's Windows
// lane lints the shipping MSVC target natively; the two differ only in
// `cfg(target_env = ...)` code, which is rare and stays the land gate's.
const windowsCrossTarget = "x86_64-pc-windows-gnu"

// RunClippyWindows lints the workspace for Windows from a Mac (or a Linux box), with the
// question CI's Windows lane asks natively on `windows-latest`: every member declared for
// Windows, `--all-targets`, `-D warnings`. The host lanes never see `cfg(windows)` code, so
// a Windows-only `unused_mut` in a test reached `main` (PR #1270, 2026-11-19) and turned the
// Windows lane red. Run it before pushing Rust that a `cfg(windows)`, `cfg(not(unix))`, or
// `target_os` gate hides from the host; the land gate runs the native lane on every Rust
// change either way. `docs/tooling/ci.md` § Windows lane.
//
// It runs in the Linux lanes' container (`desktop-rust-linux-container.go`), whose image
// carries mingw-w64 and the Windows target, against their target volume: the Windows
// units live under `x86_64-pc-windows-gnu/`, beside the Linux ones, and the volume's sweep
// never touches that tree. It doesn't record a stream for the sweep, so a sweep drops its
// host-side units (build scripts, proc macros), which costs a rebuild of those, never a
// wrong result. It never auto-fixes, like `desktop-rust-clippy-linux`.
func RunClippyWindows(ctx *CheckContext) (CheckResult, error) {
	if runtime.GOOS == "windows" {
		return Skipped("on Windows, `desktop-rust-clippy` lints the native target"), nil
	}
	if skip, unavailable := dockerUnavailable(); unavailable {
		return skip, nil
	}

	members, err := WorkspaceMembers(ctx.RootDir)
	if err != nil {
		return CheckResult{}, err
	}
	selection := CargoSelectionArgs(members, "windows")

	env, err := prepareLinuxContainer(ctx.RootDir)
	if err != nil {
		return CheckResult{}, err
	}

	container := linuxContainerName("clippy-windows")
	if err := startLinuxContainer(container, ctx.RootDir, env); err != nil {
		return CheckResult{}, err
	}
	defer removeLinuxContainer(container)

	output, err := dockerExec(container, containerCargoScript(windowsClippyArgs(selection)...))
	if err != nil {
		return CheckResult{}, fmt.Errorf("clippy found issues on Windows (%s)%s\n%s",
			windowsCrossTarget, env.buildNote(), indentOutput(trimCargoProgress(output)))
	}
	result := clippySuccess(output, " on Windows ("+windowsCrossTarget+")")
	result.Message += env.buildNote()
	return result, nil
}

// windowsClippyArgs is the Windows lane's clippy invocation (`desktop-rust-clippy` under
// `--ci` on a Windows host), cross-targeted. Pure, so the parity is testable without a
// container.
func windowsClippyArgs(selection []string) []string {
	args := append([]string{"clippy", "--locked", "--all-targets", "--target", windowsCrossTarget}, selection...)
	return append(args, "--", "-D", "warnings")
}
