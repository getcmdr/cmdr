package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"cmdr/scripts/release-assets/manifest"
)

// releaseAsset is one asset in `gh api …/releases/tags/<tag>` or `gh release view --json assets`.
type releaseAsset struct {
	Name string `json:"name"`
	Size int64  `json:"size"`
}

func readReleaseAssets(path string) ([]releaseAsset, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var rel struct {
		Assets []releaseAsset `json:"assets"`
	}
	if err := json.Unmarshal(raw, &rel); err != nil {
		return nil, fmt.Errorf("%s isn't a release's JSON: %w", path, err)
	}
	return rel.Assets, nil
}

func assetNames(assets []releaseAsset) []string {
	names := make([]string, 0, len(assets))
	for _, a := range assets {
		names = append(names, a.Name)
	}
	return names
}

// signatureNamesVersion reports whether a Tauri `.sig` (base64 of minisign's text) carries
// `version:<version>` in its trusted comment, the field `--app-version` writes and the updater
// off macOS binds the install's version to. It checks the field only: `release-finish
// -verify-dir` proves the signature itself before `publish` gets here.
func signatureNamesVersion(sig, version string) bool {
	raw, err := base64.StdEncoding.DecodeString(strings.TrimSpace(sig))
	if err != nil {
		return false
	}
	for line := range strings.SplitSeq(string(raw), "\n") {
		comment, ok := strings.CutPrefix(line, "trusted comment: ")
		if !ok {
			continue
		}
		for field := range strings.SplitSeq(comment, "\t") {
			if field == "version:"+version {
				return true
			}
		}
	}
	return false
}

type latestInput struct {
	release manifest.Release
	assets  []releaseAsset // the release as GitHub has it, for the sizes
	sigDir  string         // holds each update payload's `.sig`
	notes   string
	pubDate string
	baseURL string // where the update payloads are served, without a trailing slash
}

// buildLatest writes `latest.json`, the update manifest every installed copy polls.
//
//   - `platforms`: one entry per update payload, keyed by its updater key, with its signature and
//     its URL on the mirror.
//   - `downloads`: one entry per download the release carries (os, arch, kind, file, size), for
//     the website.
//   - `dmgSizes`: the macOS DMG sizes by arch, which website builds from before `downloads` read.
//     It stays for one more release (`docs/guides/releasing.md` § What a release publishes).
//
// The members before `downloads` are in the order, and print in the bytes, of every manifest up to
// v0.61.0 (`pipeline_test.go` holds this to v0.61.0's published file).
func buildLatest(in latestInput) (string, error) {
	sizes := map[string]int64{}
	for _, a := range in.assets {
		sizes[a.Name] = a.Size
	}
	downloads, err := manifest.Present(in.release.Downloads(), assetNames(in.assets))
	if err != nil {
		return "", err
	}

	dmgSizes := object{}
	for _, arch := range []manifest.Arch{manifest.AArch64, manifest.X86_64, manifest.Universal} {
		i := slices.IndexFunc(downloads, func(f manifest.File) bool {
			return f.OS == manifest.MacOS && f.Kind == manifest.DMG && f.Arch == arch
		})
		if i < 0 {
			return "", fmt.Errorf("the manifest has no macOS %s DMG for `dmgSizes`", arch)
		}
		dmgSizes = append(dmgSizes, member{string(arch), sizes[downloads[i].Name]})
	}

	platforms := object{}
	for _, f := range in.release.Updates() {
		if _, ok := sizes[f.Name]; !ok {
			return "", fmt.Errorf("latest.json would point at %s, which is not on the release", f.Name)
		}
		sig, err := os.ReadFile(filepath.Join(in.sigDir, f.Signature()))
		if err != nil {
			return "", fmt.Errorf("no signature for %s: %w", f.Name, err)
		}
		if f.SignsVersion() && !signatureNamesVersion(string(sig), in.release.Version) {
			return "", fmt.Errorf("%s's signature doesn't name version %s (`tauri signer sign --app-version`), so every Windows and Linux install would refuse it", f.Name, in.release.Version)
		}
		platforms = append(platforms, member{f.UpdaterKey, object{
			// Trailing newlines dropped, as the shell's `$(cat …)` that read them before did.
			{"signature", strings.TrimRight(string(sig), "\n")},
			{"url", in.baseURL + "/" + f.Name},
		}})
	}

	var list []any
	for _, f := range downloads {
		list = append(list, object{
			{"os", string(f.OS)},
			{"arch", string(f.Arch)},
			{"kind", string(f.Kind)},
			{"file", f.Name},
			{"size", sizes[f.Name]},
		})
	}

	doc := object{
		{"version", in.release.Version},
		{"dmgSizes", dmgSizes},
		{"notes", in.notes},
		{"pub_date", in.pubDate},
		{"platforms", platforms},
		{"downloads", list},
	}
	var b strings.Builder
	if err := writeJSON(&b, doc, 0); err != nil {
		return "", err
	}
	b.WriteString("\n")
	return b.String(), nil
}
