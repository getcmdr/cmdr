package checks

import "testing"

func TestAnOSGatedTestFileNeedsAReason(t *testing.T) {
	cases := map[string]struct {
		source string
		want   int
	}{
		"bare inner gate":           {"//! Cells for the walk.\n\n#![cfg(unix)]\n\nuse x;\n", 1},
		"trailing reason":           {"#![cfg(unix)] // POSIX modes\n", 0},
		"reason right above":        {"//! Unix only: POSIX modes.\n#![cfg(unix)]\n", 0},
		"negated gate, bare":        {"#![cfg(not(windows))]\n", 1},
		"test-only gate isn't OS":   {"#![cfg(test)]\n", 0},
		"a feature gate isn't OS":   {"#![cfg(feature = \"x\")]\n", 0},
		"gate in a string is prose": {"const A: &str = \"#![cfg(unix)]\";\n", 0},
	}
	for name, c := range cases {
		if got := osTestGatesIn("crates/x/src/a_test.rs", c.source, true); len(got) != c.want {
			t.Errorf("%s: got %d gates (%+v), want %d", name, len(got), got, c.want)
		}
	}
}

func TestAnInnerGateOutsideATestFileIsOutOfScope(t *testing.T) {
	if got := osTestGatesIn("crates/x/src/platform/macos.rs", "#![cfg(target_os = \"macos\")]\n", false); len(got) != 0 {
		t.Fatalf("got %+v", got)
	}
}

func TestAnOSGatedTestModuleNeedsAReason(t *testing.T) {
	source := "#[cfg(test)]\nmod tests;\n" +
		"#[cfg(all(test, unix))]\nmod posix_tests;\n" +
		"// Needs `/proc`.\n#[cfg(all(test, target_os = \"linux\"))]\nmod proc_tests;\n" +
		"#[cfg(unix)]\nmod platform;\n" +
		"#[cfg(target_os = \"macos\")] // FSEvents only\n#[path = \"x.rs\"]\nmod fsevents_test;\n"
	got := osTestGatesIn("crates/x/src/lib.rs", source, false)
	if len(got) != 1 || got[0].key != "crates/x/src/lib.rs#posix_tests" || got[0].line != 3 {
		t.Fatalf("got %+v", got)
	}
}
