package main

import (
	"bytes"
	"fmt"
	"os/exec"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// Cache pruning (`lane-markers prune`, run by `ci-upkeep.yml`): keeps the repo under GitHub's
// 10 GB cache budget so LRU eviction doesn't reach entries a run still needs, lane markers first.
// A key rotation (a new `Cargo.lock`, toolchain, or `.mise.toml`) saves a second generation of
// every big cache, and until the old one goes the repo sits past the cap (#1368).

// cacheEntry is one Actions cache entry as the REST API lists it. Times are RFC 3339 strings in
// UTC, which order the same as the instants they name.
type cacheEntry struct {
	id                    int64
	key, ref              string
	lastAccessed, created string
}

var (
	trailingHashes = regexp.MustCompile(`(-[0-9a-f]{8,})+$`)
	lastHash       = regexp.MustCompile(`-[0-9a-f]{8,}$`)
	pullRef        = regexp.MustCompile(`^refs/pull/([0-9]+)/`)
)

// cacheFamily names the family a `main` entry supersedes within, or "" for an entry pruning
// leaves alone. A family is the key minus its trailing hash parts: every generation of one cache
// shares it. mise-action's key ends in the config hash after an optional `install_args` hash
// that names a variant of its own (the Go-only install), so only the last hash goes there.
//   - ❌ Never `lane-green-*`: every marker key is its own family's only member, so a family rule
//     would read live markers as superseded. Markers on `main` age out (7 unused days).
func cacheFamily(key string) string {
	switch {
	case strings.HasPrefix(key, "v0-rust-"), strings.Contains(key, "-docker-e2e-"),
		strings.HasPrefix(key, "light-"), strings.HasPrefix(key, "pnpm-store-"),
		strings.HasPrefix(key, "wasm-toolchain-"):
		return trailingHashes.ReplaceAllString(key, "")
	case strings.HasPrefix(key, "mise-v1-"):
		return lastHash.ReplaceAllString(key, "")
	}
	return ""
}

// pruneVerdict is one entry to delete and why.
type pruneVerdict struct {
	entry  cacheEntry
	reason string
}

// planPrune picks the entries to delete:
//   - Every entry in a pull request's scope whose PR isn't open (openPRs nil means the lookup
//     failed, so this rule deletes nothing). No later run can read a closed PR's scope, and a
//     reopened PR only reruns what it would have skipped.
//   - In `main`'s scope, every member of a family but its most recently used and its newest.
//     Keeping both means no current key loses its entry: the job that last restored or saved the
//     current key made it the most recently used, and a just-saved generation is the newest even
//     when an older-based PR restored the old one since. A stale key saved after the live one (a
//     flipping toolchain hash, 2026-10-22) is the newest, and the live one is still kept as the
//     most recently used.
//
// Everything else stays, `lane-green-*` markers included (`cacheFamily`).
func planPrune(caches []cacheEntry, mainRef string, openPRs map[int]bool) []pruneVerdict {
	var out []pruneVerdict
	families := map[string][]cacheEntry{}
	for _, c := range caches {
		if m := pullRef.FindStringSubmatch(c.ref); m != nil {
			n, _ := strconv.Atoi(m[1])
			if openPRs != nil && !openPRs[n] {
				out = append(out, pruneVerdict{c, fmt.Sprintf("PR #%d is closed", n)})
			}
			continue
		}
		if c.ref != mainRef {
			continue
		}
		if f := cacheFamily(c.key); f != "" {
			families[f] = append(families[f], c)
		}
	}
	names := make([]string, 0, len(families))
	for f := range families {
		names = append(names, f)
	}
	sort.Strings(names)
	for _, f := range names {
		out = append(out, superseded(families[f])...)
	}
	return out
}

// superseded is every member of one family but its most recently used and its newest.
func superseded(members []cacheEntry) []pruneVerdict {
	if len(members) < 2 {
		return nil
	}
	mru, newest := members[0], members[0]
	for _, c := range members[1:] {
		if c.lastAccessed > mru.lastAccessed {
			mru = c
		}
		if c.created > newest.created {
			newest = c
		}
	}
	var out []pruneVerdict
	for _, c := range members {
		if c.id != mru.id && c.id != newest.id {
			out = append(out, pruneVerdict{c, fmt.Sprintf("superseded by %s", mru.key)})
		}
	}
	return out
}

// parseCacheList reads `id ref last_accessed_at created_at key` lines, tab-separated.
func parseCacheList(out string) ([]cacheEntry, error) {
	var cs []cacheEntry
	for line := range strings.SplitSeq(out, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		f := strings.Split(line, "\t")
		if len(f) != 5 {
			return nil, fmt.Errorf("unexpected cache list line %q", line)
		}
		id, err := strconv.ParseInt(f[0], 10, 64)
		if err != nil {
			return nil, fmt.Errorf("cache id in %q: %w", line, err)
		}
		cs = append(cs, cacheEntry{id: id, ref: f[1], lastAccessed: f[2], created: f[3], key: f[4]})
	}
	return cs, nil
}

// parseOpenPRs reads one PR number per line.
func parseOpenPRs(out string) (map[int]bool, error) {
	open := map[int]bool{}
	for line := range strings.SplitSeq(out, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		n, err := strconv.Atoi(line)
		if err != nil {
			return nil, fmt.Errorf("PR number %q: %w", line, err)
		}
		open[n] = true
	}
	return open, nil
}

// ghAPI runs `gh api <method> <path> <extra...>` and returns its stdout.
func ghAPI(method, apiPath string, extra ...string) (string, error) {
	cmd := exec.Command("gh", append([]string{"api", "-X", method, apiPath}, extra...)...)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("gh api %s %s: %v: %s", method, apiPath, err, strings.TrimSpace(stderr.String()))
	}
	return string(out), nil
}

func listAllCaches(repo string) ([]cacheEntry, error) {
	out, err := ghAPI("GET", "repos/"+repo+"/actions/caches", "-F", "per_page=100", "--paginate",
		"--jq", `.actions_caches[] | "\(.id)\t\(.ref)\t\(.last_accessed_at)\t\(.created_at)\t\(.key)"`)
	if err != nil {
		return nil, err
	}
	return parseCacheList(out)
}

func listOpenPRs(repo string) (map[int]bool, error) {
	out, err := ghAPI("GET", "repos/"+repo+"/pulls", "-f", "state=open", "-F", "per_page=100",
		"--paginate", "--jq", ".[].number")
	if err != nil {
		return nil, err
	}
	return parseOpenPRs(out)
}

func deleteCache(repo string, id int64) error {
	_, err := ghAPI("DELETE", fmt.Sprintf("repos/%s/actions/caches/%d", repo, id))
	return err
}
