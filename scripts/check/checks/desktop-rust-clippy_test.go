package checks

import (
	"strings"
	"testing"
)

// Trimmed from real `cargo clippy --message-format=json` output (cargo 1.99.1).
const (
	clippyUnfixableLintLine = `{"reason":"compiler-message","package_id":"path+file:///w/crates/cmdr-fs#0.1.0","manifest_path":"/w/crates/cmdr-fs/Cargo.toml","target":{"name":"cmdr_fs"},"message":{"rendered":"error: very complex type used\n  --> crates/cmdr-fs/src/lib.rs:3:8\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/","rendered":null,"spans":[]}],"code":{"code":"clippy::type_complexity","explanation":null},"level":"error","message":"very complex type used","spans":[{"file_name":"crates/cmdr-fs/src/lib.rs","is_primary":true,"label":null,"suggested_replacement":null,"suggestion_applicability":null}]}}`
	clippyCompileErrorLine  = `{"reason":"compiler-message","package_id":"path+file:///w/apps/desktop/src-tauri#0.1.0","target":{"name":"cmdr_lib"},"message":{"rendered":"error[E0425]: cannot find value ` + "`foo`" + ` in this scope\n","$message_type":"diagnostic","children":[],"code":{"code":"E0425","explanation":"..."},"level":"error","message":"cannot find value","spans":[{"file_name":"apps/desktop/src-tauri/src/lib.rs","is_primary":true,"suggested_replacement":null,"suggestion_applicability":null}]}}`
	clippyFixableLintLine   = `{"reason":"compiler-message","package_id":"path+file:///w/crates/cmdr-fs#0.1.0","target":{"name":"cmdr_fs"},"message":{"rendered":"error: redundant clone\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"remove this","rendered":null,"spans":[{"file_name":"crates/cmdr-fs/src/lib.rs","is_primary":true,"suggested_replacement":"","suggestion_applicability":"MachineApplicable"}]}],"code":{"code":"clippy::redundant_clone","explanation":null},"level":"error","message":"redundant clone","spans":[]}}`
	clippyArtifactLine      = `{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0","target":{"name":"serde"},"fresh":true}`
	clippyBuildFinishedLine = `{"reason":"build-finished","success":false}`
)

func TestParseClippyJSONRendersDiagnosticsAndKeepsCargoLines(t *testing.T) {
	output := strings.Join([]string{
		clippyArtifactLine,
		"    Checking cmdr-fs v0.1.0 (/w/crates/cmdr-fs)",
		clippyUnfixableLintLine,
		clippyBuildFinishedLine,
		"error: could not compile `cmdr-fs` (lib) due to 1 previous error",
	}, "\n")
	display, fixable := parseClippyJSON(output)
	if fixable {
		t.Errorf("a lint with no machine-applicable suggestion read as fixable")
	}
	for _, want := range []string{"Checking cmdr-fs", "error: very complex type used", "could not compile"} {
		if !strings.Contains(display, want) {
			t.Errorf("display lost %q:\n%s", want, display)
		}
	}
	if strings.Contains(display, `"reason"`) {
		t.Errorf("display kept raw JSON:\n%s", display)
	}
}

func TestParseClippyJSONSkipsTheFixPassForCompileErrors(t *testing.T) {
	if _, fixable := parseClippyJSON(clippyCompileErrorLine); fixable {
		t.Errorf("a compile error read as fixable")
	}
}

func TestParseClippyJSONFindsANestedMachineApplicableSuggestion(t *testing.T) {
	output := clippyCompileErrorLine + "\n" + clippyFixableLintLine
	if _, fixable := parseClippyJSON(output); !fixable {
		t.Errorf("a MachineApplicable suggestion in a child diagnostic was missed")
	}
}

func TestClippySuccessCountsCheckingLinesFromJSONRuns(t *testing.T) {
	display, _ := parseClippyJSON(strings.Join([]string{
		clippyArtifactLine,
		"    Checking cmdr-fs v0.1.0 (/w/crates/cmdr-fs)",
		"    Checking cmdr v0.1.0 (/w/apps/desktop/src-tauri)",
		clippyBuildFinishedLine,
	}, "\n"))
	if got := clippySuccess(display, "").Total; got != 2 {
		t.Errorf("counted %d crates, want 2", got)
	}
}

// The shipped-app pass asks what a release build compiles (#1382): the app package alone,
// default features, no test targets (their dev-dependencies turn `testing` on below), and
// warnings denied. The workspace pass's union of features hid dead code in `cmdr-local`.
func TestShippedAppClippyLintsTheAppAsItShips(t *testing.T) {
	got := strings.Join(shippedAppClippyArgs(), " ")
	if want := "clippy --locked --package " + appPackage + " -- -D warnings"; got != want {
		t.Errorf("the shipped-app clippy is %q, want %q", got, want)
	}
	for _, banned := range []string{"--all-targets", "--workspace", "--features", "--all-features", "--fix", "--tests"} {
		if strings.Contains(got, banned) {
			t.Errorf("the shipped-app clippy must not pass %s: %q", banned, got)
		}
	}
	cross := strings.Join(windowsShippedAppClippyArgs(), " ")
	if want := "clippy --locked --package " + appPackage + " --target " + windowsCrossTarget + " -- -D warnings"; cross != want {
		t.Errorf("the Windows cross-lint's shipped-app clippy is %q, want %q", cross, want)
	}
	for _, tree := range rustMemberTrees {
		if tree.Pkg == appPackage {
			if tree.Glob != "apps/desktop/src-tauri/**" {
				t.Errorf("appPackage %q is %s, not the desktop app", appPackage, tree.Glob)
			}
			return
		}
	}
	t.Errorf("appPackage %q is no workspace member", appPackage)
}
