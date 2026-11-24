package checks

// The two lists the notices take from `deny.toml`, so they cover exactly the
// licenses and the targets `cargo deny` vets.

import "strings"

// parseDenyAllowList pulls the SPDX ids out of deny.toml's `[licenses] allow`
// array. Kept to a line scan rather than a TOML dependency: the array is one
// quoted id per line with optional trailing comments, and a scanner that
// returns nothing fails loudly at the call site.
func parseDenyAllowList(contents string) []string {
	var ids []string
	inAllow := false
	for _, line := range strings.Split(contents, "\n") {
		trimmed := strings.TrimSpace(line)
		if !inAllow {
			if strings.HasPrefix(trimmed, "allow") && strings.Contains(trimmed, "[") {
				inAllow = true
			}
			continue
		}
		if strings.HasPrefix(trimmed, "]") {
			break
		}
		if start := strings.Index(trimmed, `"`); start >= 0 {
			if end := strings.Index(trimmed[start+1:], `"`); end >= 0 {
				ids = append(ids, trimmed[start+1:start+1+end])
			}
		}
	}
	return ids
}

// parseDenyTargets reads `deny.toml`'s `[graph] targets = [...]`, on one line or
// several: every quoted string from the `targets` line through its closing `]`.
func parseDenyTargets(contents string) []string {
	var targets []string
	inTargets := false
	for _, line := range strings.Split(contents, "\n") {
		trimmed := strings.TrimSpace(line)
		if !inTargets {
			if !strings.HasPrefix(trimmed, "targets") || !strings.Contains(trimmed, "[") {
				continue
			}
			inTargets = true
		}
		rest := trimmed
		for {
			start := strings.Index(rest, `"`)
			if start < 0 {
				break
			}
			end := strings.Index(rest[start+1:], `"`)
			if end < 0 {
				break
			}
			targets = append(targets, rest[start+1:start+1+end])
			rest = rest[start+end+2:]
		}
		if strings.Contains(trimmed, "]") {
			break
		}
	}
	return targets
}
