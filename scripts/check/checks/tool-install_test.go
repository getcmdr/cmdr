package checks

import "testing"

func TestCargoToolVersionIs(t *testing.T) {
	cases := []struct {
		name   string
		output string
		tool   string
		want   string
		match  bool
	}{
		{"the pinned version", "cargo-nextest 0.9.136 (1d5bf1ec9 2026-05-16)\nrelease: 0.9.136\n", "cargo-nextest", "0.9.136", true},
		{"an older install", "cargo-nextest 0.9.120 (abc 2026-01-02)\n", "cargo-nextest", "0.9.136", false},
		{"a version with the pin as its prefix", "cargo-nextest 0.9.1360 (abc 2026-01-02)\n", "cargo-nextest", "0.9.136", false},
		{"no output", "", "cargo-nextest", "0.9.136", false},
		{"cargo's unknown-command error", "error: no such command: `nextest`\n", "cargo-nextest", "0.9.136", false},
		{"cargo-deny at the pin", "cargo-deny 0.19.6\n", "cargo-deny", "0.19.6", true},
		{"a stale cargo-deny", "cargo-deny 0.18.9\n", "cargo-deny", "0.19.6", false},
		{"another tool at the same version", "cargo-nextest 0.19.6\n", "cargo-deny", "0.19.6", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := cargoToolVersionIs(tc.output, tc.tool, tc.want); got != tc.match {
				t.Errorf("cargoToolVersionIs(%q, %q, %q) = %v, want %v", tc.output, tc.tool, tc.want, got, tc.match)
			}
		})
	}
}
