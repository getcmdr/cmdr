package main

import (
	"encoding/json"
	"fmt"
	"regexp"
	"slices"
	"strings"

	"cmdr/scripts/release-assets/manifest"
)

// mirroredAssets picks, from a release's asset names, the files the EU mirror serves, from the
// release asset manifest (`scripts/release-assets/manifest`): every signed file (the update
// payloads, and the advisory list `latest.json` points Windows and Linux installs at) and its
// signature, every download (an optional one, like the pkg, when the release has it), and
// `checksums.txt` last, so it never names a file the mirror doesn't hold yet. The SBOMs, the
// provenance bundle, and `latest.json` stay on GitHub only (the website serves the manifest).
// A release missing any required file is refused, naming each one. r is the release as it carries
// its legs (`carried`), so a Windows leg the release ships without is neither mirrored nor missed.
func mirroredAssets(names []string, r manifest.Release) ([]string, error) {
	version := r.Version
	have := map[string]bool{}
	for _, n := range names {
		have[n] = true
	}
	var want, missing []string
	need := func(name string, optional bool) {
		switch {
		case have[name]:
			want = append(want, name)
		case !optional:
			missing = append(missing, name)
		}
	}
	for _, f := range r.Signed() {
		need(f.Signature(), false)
	}
	for _, f := range r.Files {
		if f.Binary() || f.Download() || f.Signed() {
			need(f.Name, f.Optional)
		}
	}
	slices.Sort(want)
	slices.Sort(missing)
	want = append(want, "checksums.txt")
	if !have["checksums.txt"] {
		missing = append(missing, "checksums.txt")
	}
	if len(missing) > 0 {
		return nil, fmt.Errorf("release %s is missing files the mirror needs: %s", version, strings.Join(missing, ", "))
	}
	return want, nil
}

// carried is r as the release carries its legs: the ones latestJSON names when it's given (the
// manifest `publish` wrote, which every later step follows), else the ones whole among names.
func carried(r manifest.Release, names []string, latestJSON []byte) (manifest.Release, error) {
	if latestJSON != nil {
		return r.CarriedBy(latestJSON)
	}
	whole, lacking := r.CarriedWhole(names)
	for os, missing := range lacking {
		fmt.Println(manifest.LackingLine(r.Version, os, missing))
	}
	return whole, nil
}

// objectKey is where a release file lives in the bucket: one folder per version, so a key is
// never reused and every object can be cached forever.
func objectKey(version, name string) string {
	return "v" + version + "/" + name
}

// publicURL is the file's address on the mirror's public host.
func publicURL(host, version, name string) string {
	return strings.TrimSuffix(host, "/") + "/" + objectKey(version, name)
}

// immutableCache is every object's `Cache-Control`. A key names one version's one file, and the
// mirror runs only once the release's bytes are final, so a cached copy never goes stale.
const immutableCache = "public, max-age=31536000, immutable"

func contentType(name string) string {
	switch {
	case strings.HasSuffix(name, ".dmg"):
		return "application/x-apple-diskimage"
	case strings.HasSuffix(name, ".tar.gz"):
		return "application/gzip"
	case strings.HasSuffix(name, ".sig"), strings.HasSuffix(name, ".txt"):
		return "text/plain; charset=utf-8"
	case strings.HasSuffix(name, ".json"):
		return "application/json"
	case strings.HasSuffix(name, ".zip"):
		return "application/zip"
	case strings.HasSuffix(name, ".msix"):
		return "application/msix"
	default:
		return "application/octet-stream"
	}
}

var checksumLineRE = regexp.MustCompile(`^([0-9a-f]{64})  (\S+)$`)

// parseChecksums reads `checksums.txt` (`shasum -a 256` output) into name → hex SHA-256.
func parseChecksums(body string) (map[string]string, error) {
	sums := map[string]string{}
	for line := range strings.Lines(body) {
		line = strings.TrimRight(line, "\r\n")
		if line == "" {
			continue
		}
		m := checksumLineRE.FindStringSubmatch(line)
		if m == nil {
			return nil, fmt.Errorf("checksums.txt has a line that isn't `<sha256>  <name>`: %q", line)
		}
		sums[m[2]] = m[1]
	}
	return sums, nil
}

// checkManifest requires `latest.json` to name this version and, for each update payload in the
// release asset manifest, exactly that file on the mirror under its platform key. Every
// installed copy fetches these URLs verbatim with no fallback, so a URL off the mirror (or a
// drifted name) must fail the release while nothing has shipped.
func checkManifest(body []byte, r manifest.Release, host string) error {
	version := r.Version
	var m struct {
		Version   string `json:"version"`
		Platforms map[string]struct {
			URL string `json:"url"`
		} `json:"platforms"`
		SecurityAdvisories *struct {
			URL string `json:"url"`
		} `json:"securityAdvisories"`
	}
	if err := json.Unmarshal(body, &m); err != nil {
		return fmt.Errorf("latest.json isn't JSON: %w", err)
	}
	if m.Version != version {
		return fmt.Errorf("latest.json says version %q, not %q", m.Version, version)
	}
	if list, ok := r.AdvisoryList(); ok {
		if want := publicURL(host, version, list.Name); m.SecurityAdvisories == nil || m.SecurityAdvisories.URL != want {
			return fmt.Errorf("latest.json's securityAdvisories doesn't point at %s", want)
		}
	}
	for _, f := range r.Updates() {
		p, ok := m.Platforms[f.UpdaterKey]
		if !ok {
			return fmt.Errorf("latest.json has no %s platform", f.UpdaterKey)
		}
		if want := publicURL(host, version, f.Name); p.URL != want {
			return fmt.Errorf("latest.json's %s URL is %s, not %s", f.UpdaterKey, p.URL, want)
		}
	}
	return nil
}
