package main

import (
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"os"
	"strings"
	"testing"
)

// builtFixture is what `latest-json` builds from v0.61.0's inputs: its published bytes plus
// `downloads`. The app's reader tests (`crates/cmdr-update-manifest`) run every reader that ever
// shipped over this file, so the writer's output and the readers share one fixture.
const builtFixture = fixtureDir + "/latest.built.json"

// repoKeyIDs reads the updater key IDs the app trusts from this checkout.
func repoKeyIDs(t *testing.T) []string {
	t.Helper()
	ids, err := trustedKeyIDs(repoRoot(t))
	must(t, err)
	return ids
}

// fakeSignature is a well-formed Tauri signature by key `keyID` (hex) with `trusted` as its
// trusted comment. Its bytes sign nothing: the shape is what the validator reads.
func fakeSignature(keyID, trusted string) string {
	id := make([]byte, 8)
	if decoded, err := hex.DecodeString(keyID); err == nil && len(decoded) == 8 {
		id = decoded
	}
	raw := []byte("ED")
	for i := range 8 {
		raw = append(raw, id[7-i])
	}
	raw = append(raw, make([]byte, 64)...)
	text := "untrusted comment: signature from tauri secret key\n" + base64.StdEncoding.EncodeToString(raw) +
		"\ntrusted comment: " + trusted + "\n" + base64.StdEncoding.EncodeToString(make([]byte, 64)) + "\n"
	return base64.StdEncoding.EncodeToString([]byte(text))
}

// The built manifest is the fixture the Rust readers read. `-update` rewrites it; review the diff.
func TestBuiltLatestJSONIsTheReadersFixture(t *testing.T) {
	in, _ := published(t)
	got, err := buildLatest(in)
	must(t, err)
	if *update {
		must(t, os.WriteFile(builtFixture, []byte(got), 0o644))
	}
	want, err := os.ReadFile(builtFixture)
	must(t, err)
	if got != string(want) {
		t.Fatalf("latest-json's output moved from %s; rerun with -update and review it", builtFixture)
	}
}

// v0.61.0's own published manifest predates `downloads`, so the validator refuses it for that
// alone: everything else in it passes.
func TestValidatorPassesThePublishedManifestButForDownloads(t *testing.T) {
	in, published := published(t)
	err := validateLatest([]byte(published), in.rules())
	if err == nil || !strings.Contains(err.Error(), `it has no "downloads"`) || strings.Count(err.Error(), "\n  - ") != 1 {
		t.Fatalf("want exactly the missing downloads, got %v", err)
	}
}

// Every way a manifest can break a reader or the release, each refused with a reason naming it.
func TestValidatorRefusesEveryBrokenManifest(t *testing.T) {
	in, _ := published(t)
	built, err := buildLatest(in)
	must(t, err)
	rules := in.rules()
	if err := validateLatest([]byte(built), rules); err != nil {
		t.Fatalf("the built manifest must pass: %v", err)
	}
	foreignKey := fakeSignature("0123456789ABCDEF", "timestamp:1")

	type doc = map[string]any
	edits := []struct {
		name, want string
		edit       func(d doc)
	}{
		{"no version", `no "version"`, func(d doc) { delete(d, "version") }},
		{"no platforms", `no "platforms"`, func(d doc) { delete(d, "platforms") }},
		{"no notes", `no "notes"`, func(d doc) { delete(d, "notes") }},
		{"no pub_date", `no "pub_date"`, func(d doc) { delete(d, "pub_date") }},
		{"no dmgSizes", `no "dmgSizes"`, func(d doc) { delete(d, "dmgSizes") }},
		{"an unknown member", `"channel"`, func(d doc) { d["channel"] = "beta" }},
		{"a numeric version", "version:", func(d doc) { d["version"] = 61 }},
		{"another version", `but the release is 0.61.0`, func(d doc) { d["version"] = "0.62.0" }},
		{"a v-prefixed version", "isn't semver", func(d doc) { d["version"] = "v0.61.0" }},
		{"empty notes", "notes are empty", func(d doc) { d["notes"] = " \n" }},
		{"numeric notes", "notes:", func(d doc) { d["notes"] = 1 }},
		{"a date-only pub_date", "pub_date", func(d doc) { d["pub_date"] = "2026-11-13" }},
		{"an offset pub_date", "pub_date", func(d doc) { d["pub_date"] = "2026-11-13T15:59:56+01:00" }},
		{"platforms as a list", "platforms:", func(d doc) { d["platforms"] = []any{} }},
		{"a missing platform", `no "darwin-x86_64"`, func(d doc) { delete(d["platforms"].(doc), "darwin-x86_64") }},
		{"an extra platform", `"darwin-arm64"`, func(d doc) {
			d["platforms"].(doc)["darwin-arm64"] = d["platforms"].(doc)["darwin-aarch64"]
		}},
		{"an entry as a string", "platforms.darwin-aarch64:", func(d doc) { d["platforms"].(doc)["darwin-aarch64"] = "https://x" }},
		{"an entry without a url", "needs both", func(d doc) { delete(d["platforms"].(doc)["darwin-aarch64"].(doc), "url") }},
		{"an entry with an extra field", "unknown field", func(d doc) { d["platforms"].(doc)["darwin-aarch64"].(doc)["size"] = 1 }},
		{"an http url", "isn't an absolute https URL", func(d doc) {
			d["platforms"].(doc)["darwin-aarch64"].(doc)["url"] = "http://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_aarch64.app.tar.gz"
		}},
		{"a relative url", "isn't an absolute https URL", func(d doc) { d["platforms"].(doc)["darwin-aarch64"].(doc)["url"] = "Cmdr_0.61.0_aarch64.app.tar.gz" }},
		{"a url off the mirror", "want", func(d doc) {
			d["platforms"].(doc)["darwin-aarch64"].(doc)["url"] = "https://github.com/getcmdr/cmdr/releases/download/v0.61.0/Cmdr_0.61.0_aarch64.app.tar.gz"
		}},
		{"another payload's signature", "isn't the release's", func(d doc) {
			p := d["platforms"].(doc)
			p["darwin-aarch64"].(doc)["signature"] = p["darwin-x86_64"].(doc)["signature"]
		}},
		{"a signature that isn't base64", "isn't base64", func(d doc) { d["platforms"].(doc)["darwin-aarch64"].(doc)["signature"] = "not base64!" }},
		{"a signature by a foreign key", "doesn't trust", func(d doc) { d["platforms"].(doc)["darwin-aarch64"].(doc)["signature"] = foreignKey }},
		{"a dropped download", "lists 3 files", func(d doc) { d["downloads"] = d["downloads"].([]any)[:3] }},
		{"a download's wrong size", "says 1 bytes", func(d doc) { d["downloads"].([]any)[0].(doc)["size"] = 1 }},
		{"a download's string size", "downloads[0]:", func(d doc) { d["downloads"].([]any)[0].(doc)["size"] = "63540866" }},
		{"an unknown download kind", "want macos/aarch64/dmg", func(d doc) { d["downloads"].([]any)[0].(doc)["kind"] = "zip" }},
		{"a download's extra field", "unknown field", func(d doc) { d["downloads"].([]any)[0].(doc)["sha256"] = "x" }},
		{"a wrong DMG size", "dmgSizes.aarch64", func(d doc) { d["dmgSizes"].(doc)["aarch64"] = 2 }},
		{"a missing DMG size", "dmgSizes has no universal", func(d doc) { delete(d["dmgSizes"].(doc), "universal") }},
		{"an advisory entry as a string", "securityAdvisories:", func(d doc) { d["securityAdvisories"] = "https://x" }},
		{"an advisory entry without a signature", "needs both", func(d doc) {
			d["securityAdvisories"] = doc{"url": "https://download.getcmdr.com/v0.61.0/checksums.txt"}
		}},
		{"an advisory list the release lacks", "doesn't carry", func(d doc) {
			d["securityAdvisories"] = doc{"url": "https://download.getcmdr.com/v0.61.0/security-advisories.json", "signature": fakeSignature(rules.trustedKeyIDs[0], "timestamp:1")}
		}},
	}
	for _, e := range edits {
		t.Run(e.name, func(t *testing.T) {
			d := doc{}
			must(t, json.Unmarshal([]byte(built), &d))
			e.edit(d)
			raw, err := json.Marshal(d)
			must(t, err)
			err = validateLatest(raw, rules)
			if err == nil || !strings.Contains(err.Error(), e.want) {
				t.Fatalf("want a refusal naming %q, got %v", e.want, err)
			}
		})
	}

	// A well-formed advisory entry pointing at an asset of the release passes.
	d := doc{}
	must(t, json.Unmarshal([]byte(built), &d))
	d["securityAdvisories"] = doc{"url": "https://download.getcmdr.com/v0.61.0/checksums.txt", "signature": fakeSignature(rules.trustedKeyIDs[1], "timestamp:1")}
	raw, err := json.Marshal(d)
	must(t, err)
	if err := validateLatest(raw, rules); err != nil {
		t.Errorf("a well-formed advisory entry must pass: %v", err)
	}
}

// Damage below the JSON level: what a serde reader refuses (a duplicate member), what Go's
// decoder would quietly accept (a null, trailing data), and bodies that aren't a manifest at all.
func TestValidatorRefusesBrokenBytes(t *testing.T) {
	in, _ := published(t)
	built, err := buildLatest(in)
	must(t, err)
	rules := in.rules()
	cases := map[string]struct {
		raw  []byte
		want string
	}{
		"a duplicate member":    {[]byte(strings.Replace(built, `"version": "0.61.0",`, `"version": "0.61.0", "version": "0.61.0",`, 1)), `names "version" twice`},
		"a null":                {[]byte(strings.Replace(built, `"version": "0.61.0",`, `"version": null,`, 1)), "$.version is null"},
		"trailing data":         {[]byte(built + "{}"), "more after"},
		"a truncated body":      {[]byte(built[:len(built)/2]), "isn't JSON"},
		"invalid UTF-8":         {append([]byte(built[:20]), 0xff, 0xfe), "UTF-8"},
		"an HTML error page":    {[]byte("<!doctype html><title>502</title>"), "isn't JSON"},
		"an empty body":         {nil, "isn't JSON"},
		"a list":                {[]byte("[]"), "isn't a JSON object"},
		"an oversized manifest": {[]byte(strings.Replace(built, `"notes": "`, `"notes": "`+strings.Repeat("x", maxLatestBytes), 1)), "over the"},
	}
	for name, c := range cases {
		t.Run(name, func(t *testing.T) {
			err := validateLatest(c.raw, rules)
			if err == nil || !strings.Contains(err.Error(), c.want) {
				t.Fatalf("want a refusal naming %q, got %v", c.want, err)
			}
		})
	}
}

// The live manifest's version: a candidate must move past it.
func TestValidatorRequiresANewerVersion(t *testing.T) {
	in, _ := published(t)
	built, err := buildLatest(in)
	must(t, err)
	for _, live := range []string{"0.61.0", "0.62.0", "1.0.0"} {
		r := in.rules()
		r.newerThan = live
		if err := validateLatest([]byte(built), r); err == nil || !strings.Contains(err.Error(), "isn't newer than the live "+live) {
			t.Errorf("0.61.0 over a live %s must be refused, got %v", live, err)
		}
	}
}

func TestCompareVersionsOrdersAsSemver(t *testing.T) {
	ordered := []string{"0.9.0", "0.10.0-alpha", "0.10.0-alpha.1", "0.10.0-alpha.beta", "0.10.0-beta.2", "0.10.0-beta.11", "0.10.0-rc.1", "0.10.0", "0.10.1", "1.0.0"}
	for i := range ordered {
		for j := range ordered {
			got := compareVersions(ordered[i], ordered[j])
			if (i < j && got >= 0) || (i > j && got <= 0) || (i == j && got != 0) {
				t.Errorf("compareVersions(%s, %s) = %d", ordered[i], ordered[j], got)
			}
		}
	}
}

// The key IDs read off the repo's keys match the one every v0.61.0 signature carries.
func TestTrustedKeyIDsNameTheReleaseSigningKey(t *testing.T) {
	ids := repoKeyIDs(t)
	sig, err := os.ReadFile(fixtureDir + "/Cmdr_0.61.0_aarch64.app.tar.gz.sig")
	must(t, err)
	id, err := minisignKeyID(string(sig))
	must(t, err)
	if len(ids) != 2 || ids[0] != id || ids[1] == id {
		t.Fatalf("trusted %v, v0.61.0 signed by %s (want the main key first, the backup second)", ids, id)
	}
}

// Whatever the bytes, the validator answers instead of panicking.
func FuzzValidateLatest(f *testing.F) {
	raw, err := os.ReadFile(builtFixture)
	if err != nil {
		f.Fatal(err)
	}
	f.Add(raw)
	f.Add([]byte("{}"))
	f.Add([]byte(`{"version":"0.61.0","platforms":{"x":{"url":1}}}`))
	f.Add(bytes.Repeat([]byte("["), 1000))
	r := latestRules{baseURL: "https://download.getcmdr.com/v0.61.0", trustedKeyIDs: []string{"0123456789ABCDEF"}}
	f.Fuzz(func(t *testing.T, raw []byte) {
		_ = validateLatest(raw, r)
	})
}
