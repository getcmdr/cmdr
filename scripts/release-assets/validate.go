package main

import (
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"cmdr/scripts/release-assets/manifest"
)

// maxLatestBytes is the most `latest.json` may weigh. The app reads at most
// `cmdr_update_manifest::MAX_MANIFEST_BYTES` (1 MiB) and refuses anything bigger, so this keeps
// a 4x margin: a manifest near it is a bug (a runaway changelog section), never a release.
const maxLatestBytes = 256 << 10

// latestRules is what a candidate `latest.json` must agree with: the release it describes and
// what's already live. One validator, run on the bytes `latest-json` writes and again on what the
// release serves back after the upload (`validate-latest`).
type latestRules struct {
	release manifest.Release
	assets  []releaseAsset // the release as GitHub has it
	sigDir  string         // each update payload's `.sig`, which its `platforms` entry must carry verbatim
	baseURL string         // where the update payloads are served, without a trailing slash
	// trustedKeyIDs are the minisign key IDs (hex) of every key the app accepts an update from.
	trustedKeyIDs []string
	// newerThan is the live manifest's version, which this one must move past; empty when the
	// pipeline doesn't know it (the dry run) or the release is a deliberate republish.
	newerThan string
}

// topLevelKeys are the members `latest.json` may carry. Every reader that ever shipped ignores
// members it doesn't know, but a key appears here only on purpose: adding one is also a fixture
// in `cmdr-update-manifest`'s history tests.
var topLevelKeys = []string{"version", "dmgSizes", "notes", "pub_date", "platforms", "downloads", "securityAdvisories"}

// optionalTopLevelKeys may be absent.
var optionalTopLevelKeys = []string{"securityAdvisories"}

// rustSemverPattern is what the app's `semver::Version::parse` accepts: no leading `v`, no
// leading zeros in the three numbers.
var rustSemverPattern = regexp.MustCompile(`^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$`)

// pubDatePattern is the one shape the pipeline writes (`date -u +%Y-%m-%dT%H:%M:%SZ`), which the
// Tauri updater plugin in Cmdr 0.7 and earlier parses as RFC 3339 and refuses otherwise.
var pubDatePattern = regexp.MustCompile(`^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$`)

// problemFunc records one problem with the manifest.
type problemFunc = func(format string, args ...any)

// memberChecks checks each top-level member present, by name.
var memberChecks = map[string]func(json.RawMessage, latestRules, problemFunc){
	"version":            checkVersion,
	"notes":              checkNotes,
	"pub_date":           checkPubDate,
	"platforms":          checkPlatforms,
	"downloads":          checkDownloads,
	"dmgSizes":           checkDMGSizes,
	"securityAdvisories": checkAdvisoryEntry,
}

// validateLatest checks a candidate `latest.json` against everything the readers and the release
// need from it, and answers every problem it finds at once (nil when there are none).
func validateLatest(raw []byte, r latestRules) error {
	var problems []string
	add := func(format string, args ...any) { problems = append(problems, fmt.Sprintf(format, args...)) }
	if top, ok := decodeTop(raw, add); ok {
		for _, key := range topLevelKeys {
			if v, present := top[key]; present {
				memberChecks[key](v, r, add)
			}
		}
		if list, ships := r.release.AdvisoryList(); ships {
			if _, present := top["securityAdvisories"]; !present {
				add("it has no \"securityAdvisories\", though the release ships %s, so Windows and Linux installs would hear of no advisory", list.Name)
			}
		}
	}
	return joinProblems(problems)
}

// decodeTop reads the manifest's bytes into its members, checking the encoding, the JSON
// syntax, and the member names. It says false when the members can't be read at all.
func decodeTop(raw []byte, add problemFunc) (map[string]json.RawMessage, bool) {
	if len(raw) > maxLatestBytes {
		add("it's %d bytes, over the %d this pipeline allows (the app refuses a manifest over 1 MiB)", len(raw), maxLatestBytes)
	}
	if !utf8.Valid(raw) {
		add("it isn't valid UTF-8")
		return nil, false
	}
	if err := checkJSONSyntax(raw); err != nil {
		add("%v", err)
		return nil, false
	}
	var top map[string]json.RawMessage
	if err := json.Unmarshal(raw, &top); err != nil {
		add("it isn't a JSON object: %v", err)
		return nil, false
	}
	for key := range top {
		if !slices.Contains(topLevelKeys, key) {
			add("it carries %q, a member no reader knows (add it to `topLevelKeys` and the reader's history fixtures on purpose)", key)
		}
	}
	for _, key := range topLevelKeys {
		if _, ok := top[key]; !ok && !slices.Contains(optionalTopLevelKeys, key) {
			add("it has no %q", key)
		}
	}
	return top, true
}

// checkNotes holds `notes` to the string every reader takes. Empty notes publish (a hotfix tag
// with no CHANGELOG section): `writeLatest` warns about them, ❌ never refuses, since by then the
// release is signed (`docs/guides/releasing.md` § Release notes).
func checkNotes(raw json.RawMessage, _ latestRules, add problemFunc) {
	var notes string
	if err := strictDecode(raw, &notes); err != nil {
		add("notes: %v", err)
	}
}

func checkPubDate(raw json.RawMessage, _ latestRules, add problemFunc) {
	var date string
	if err := strictDecode(raw, &date); err != nil {
		add("pub_date: %v", err)
	} else if _, err := time.Parse(time.RFC3339, date); err != nil || !pubDatePattern.MatchString(date) {
		add("pub_date %q isn't YYYY-MM-DDTHH:MM:SSZ, which Cmdr 0.7 and earlier refuse the whole manifest over", date)
	}
}

func joinProblems(problems []string) error {
	if len(problems) == 0 {
		return nil
	}
	return errors.New("latest.json would break the update channel:\n  - " + strings.Join(problems, "\n  - "))
}

// checkJSONSyntax walks the document once: exactly one value with nothing after it, no member
// named twice in one object (serde refuses a duplicate struct field, so one would fail every
// install's parse while Go's decoder quietly keeps the last), and no `null` (nothing in the
// manifest is nullable, and Go reads a null into a string as "").
func checkJSONSyntax(raw []byte) error {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	if err := walkValue(dec, "$"); err != nil {
		return err
	}
	if _, err := dec.Token(); err != io.EOF {
		return errors.New("it has more after its JSON value")
	}
	return nil
}

func walkValue(dec *json.Decoder, at string) error {
	tok, err := dec.Token()
	if err != nil {
		return fmt.Errorf("it isn't JSON at %s: %w", at, err)
	}
	switch t := tok.(type) {
	case nil:
		return fmt.Errorf("%s is null", at)
	case json.Delim:
		switch t {
		case '{':
			seen := map[string]bool{}
			for dec.More() {
				keyTok, err := dec.Token()
				if err != nil {
					return fmt.Errorf("it isn't JSON at %s: %w", at, err)
				}
				key, _ := keyTok.(string)
				if seen[key] {
					return fmt.Errorf("%s names %q twice", at, key)
				}
				seen[key] = true
				if err := walkValue(dec, at+"."+key); err != nil {
					return err
				}
			}
		case '[':
			for i := 0; dec.More(); i++ {
				if err := walkValue(dec, at+"["+strconv.Itoa(i)+"]"); err != nil {
					return err
				}
			}
		}
		if _, err := dec.Token(); err != nil { // the closing delimiter
			return fmt.Errorf("it isn't JSON at %s: %w", at, err)
		}
	}
	return nil
}

// strictDecode reads one value into v, refusing members v doesn't declare.
func strictDecode(raw json.RawMessage, v any) error {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.DisallowUnknownFields()
	return dec.Decode(v)
}

func checkVersion(raw json.RawMessage, r latestRules, add problemFunc) {
	var version string
	if err := strictDecode(raw, &version); err != nil {
		add("version: %v", err)
		return
	}
	if version != r.release.Version {
		add("version is %q, but the release is %s", version, r.release.Version)
	}
	if !rustSemverPattern.MatchString(version) {
		add("version %q isn't semver the app can parse, so every install would read it as no update", version)
		return
	}
	if r.newerThan != "" && compareVersions(version, r.newerThan) <= 0 {
		add("version %s isn't newer than the live %s, which offers every install a downgrade or nothing", version, r.newerThan)
	}
}

// entry is a `platforms` or `securityAdvisories` value: the one shape every reader since Cmdr
// 0.1 reads, both members required strings.
type entry struct {
	Signature *string `json:"signature"`
	URL       *string `json:"url"`
}

func checkPlatforms(raw json.RawMessage, r latestRules, add problemFunc) {
	var platforms map[string]json.RawMessage
	if err := strictDecode(raw, &platforms); err != nil {
		add("platforms: %v", err)
		return
	}
	sizes := assetSizes(r.assets)
	want := map[string]manifest.File{}
	for _, f := range r.release.Updates() {
		want[f.UpdaterKey] = f
	}
	for key := range platforms {
		if _, ok := want[key]; !ok {
			add("platforms has %q, which no update payload in the release asset manifest uses", key)
		}
	}
	for key, f := range want {
		v, ok := platforms[key]
		if !ok {
			add("platforms has no %q, so every %s install would sit on its version", key, key)
			continue
		}
		checkPlatformEntry(v, key, f, sizes[f.Name], r, add)
	}
}

// checkPlatformEntry checks one update payload's entry: its URL on the release, its signature the
// release's own, by a trusted key, and naming the version where the app binds it there.
func checkPlatformEntry(raw json.RawMessage, key string, f manifest.File, size int64, r latestRules, add problemFunc) {
	e, ok := decodeEntry(raw, "platforms."+key, add)
	if !ok {
		return
	}
	if wantURL := r.baseURL + "/" + f.Name; *e.URL != wantURL {
		add("platforms.%s.url is %q, want %q", key, *e.URL, wantURL)
	}
	if size <= 0 {
		add("platforms.%s names %s, which the release doesn't carry (or carries empty)", key, f.Name)
	}
	checkReleaseSignature(*e.Signature, "platforms."+key, f, r, add)
	checkSignature(*e.Signature, "platforms."+key, r, add)
	if f.SignsVersion() && !signatureNamesVersion(*e.Signature, r.release.Version) {
		add("%s's signature doesn't name version %s (`tauri signer sign --app-version`), so every Windows and Linux install would refuse it", f.Name, r.release.Version)
	}
}

// checkAdvisoryEntry checks the optional signed advisory list: its URL serves an asset of this
// release, and its signature is one the app can verify.
func checkAdvisoryEntry(raw json.RawMessage, r latestRules, add problemFunc) {
	e, ok := decodeEntry(raw, "securityAdvisories", add)
	if !ok {
		return
	}
	name := path.Base(*e.URL)
	if *e.URL != r.baseURL+"/"+name {
		add("securityAdvisories.url %q isn't under %s", *e.URL, r.baseURL)
	}
	if size, ok := assetSizes(r.assets)[name]; !ok || size <= 0 {
		add("securityAdvisories names %s, which the release doesn't carry (or carries empty)", name)
	}
	if list, ships := r.release.AdvisoryList(); ships {
		if name != list.Name {
			add("securityAdvisories names %s, but the release's advisory list is %s", name, list.Name)
		}
		checkReleaseSignature(*e.Signature, "securityAdvisories", list, r, add)
	}
	checkSignature(*e.Signature, "securityAdvisories", r, add)
}

// checkReleaseSignature holds an entry's signature to the release's own `.sig` of f, byte for byte.
func checkReleaseSignature(sig, at string, f manifest.File, r latestRules, add problemFunc) {
	if r.sigDir == "" {
		return
	}
	want, err := os.ReadFile(filepath.Join(r.sigDir, f.Signature()))
	switch {
	case err != nil:
		add("%s: no %s to compare with: %v", at, f.Signature(), err)
	case strings.TrimRight(string(want), "\n") != sig:
		add("%s.signature isn't the release's %s", at, f.Signature())
	}
}

// decodeEntry reads a `{ signature, url }` entry, with an absolute `https` URL and both present.
func decodeEntry(raw json.RawMessage, at string, add problemFunc) (entry, bool) {
	var e entry
	if err := strictDecode(raw, &e); err != nil {
		add("%s: %v", at, err)
		return e, false
	}
	if e.Signature == nil || e.URL == nil {
		add("%s needs both a signature and a url", at)
		return e, false
	}
	u, err := url.Parse(*e.URL)
	if err != nil || u.Scheme != "https" || u.Host == "" || u.User != nil {
		add("%s.url %q isn't an absolute https URL (the Tauri updater in Cmdr 0.7 and earlier refuses the whole manifest over one)", at, *e.URL)
		return e, false
	}
	return e, true
}

// checkSignature checks a Tauri signature's shape (base64 of minisign's four lines) and that a
// key the app trusts made it. `release-finish -verify-dir` proves the signature itself.
func checkSignature(sig, at string, r latestRules, add problemFunc) {
	keyID, err := minisignKeyID(sig)
	if err != nil {
		add("%s.signature: %v", at, err)
		return
	}
	if len(r.trustedKeyIDs) > 0 && !slices.Contains(r.trustedKeyIDs, keyID) {
		add("%s.signature is by key %s, which the app doesn't trust (trusts %s)", at, keyID, strings.Join(r.trustedKeyIDs, ", "))
	}
}

// minisignKeyID reads the key ID (hex, as minisign prints it) off a Tauri signature.
func minisignKeyID(sig string) (string, error) {
	text, err := base64.StdEncoding.DecodeString(strings.TrimSpace(sig))
	if err != nil {
		return "", fmt.Errorf("isn't base64: %w", err)
	}
	lines := strings.Split(strings.TrimRight(string(text), "\n"), "\n")
	if len(lines) != 4 || !strings.HasPrefix(lines[0], "untrusted comment: ") || !strings.HasPrefix(lines[2], "trusted comment: ") {
		return "", errors.New("isn't a minisign signature (untrusted comment, signature, trusted comment, global signature)")
	}
	raw, err := base64.StdEncoding.DecodeString(lines[1])
	if err != nil || len(raw) != 2+8+64 || (string(raw[:2]) != "Ed" && string(raw[:2]) != "ED") {
		return "", errors.New("its signature line isn't an Ed25519 minisign signature")
	}
	global, err := base64.StdEncoding.DecodeString(lines[3])
	if err != nil || len(global) != 64 {
		return "", errors.New("its global signature isn't 64 bytes of base64")
	}
	id := make([]byte, 8)
	for i := range id { // minisign stores the key ID little-endian and prints it big-endian
		id[i] = raw[2+7-i]
	}
	return strings.ToUpper(hex.EncodeToString(id)), nil
}

// download is one `downloads` entry.
type download struct {
	OS   *string `json:"os"`
	Arch *string `json:"arch"`
	Kind *string `json:"kind"`
	File *string `json:"file"`
	Size *int64  `json:"size"`
}

func checkDownloads(raw json.RawMessage, r latestRules, add problemFunc) {
	got := []json.RawMessage{}
	if err := strictDecode(raw, &got); err != nil {
		add("downloads: %v", err)
		return
	}
	want, err := manifest.Present(r.release.Downloads(), assetNames(r.assets))
	if err != nil {
		add("downloads: %v", err)
		return
	}
	if len(got) != len(want) {
		add("downloads lists %d files, but the release carries %d downloads", len(got), len(want))
	}
	sizes := assetSizes(r.assets)
	for i := range min(len(got), len(want)) {
		checkDownload(got[i], i, want[i], sizes[want[i].Name], add)
	}
}

// checkDownload checks one `downloads` entry against the file it must describe.
func checkDownload(raw json.RawMessage, i int, f manifest.File, size int64, add problemFunc) {
	var d download
	if err := strictDecode(raw, &d); err != nil {
		add("downloads[%d]: %v", i, err)
		return
	}
	if d.OS == nil || d.Arch == nil || d.Kind == nil || d.File == nil || d.Size == nil {
		add("downloads[%d] needs os, arch, kind, file, and size", i)
		return
	}
	if *d.OS != string(f.OS) || *d.Arch != string(f.Arch) || *d.Kind != string(f.Kind) || *d.File != f.Name {
		add("downloads[%d] is %s/%s/%s %s, want %s/%s/%s %s", i, *d.OS, *d.Arch, *d.Kind, *d.File, f.OS, f.Arch, f.Kind, f.Name)
	}
	if *d.Size != size || size <= 0 {
		add("downloads[%d] (%s) says %d bytes, the release has %d", i, f.Name, *d.Size, size)
	}
}

func checkDMGSizes(raw json.RawMessage, r latestRules, add problemFunc) {
	var got map[string]int64
	if err := strictDecode(raw, &got); err != nil {
		add("dmgSizes: %v", err)
		return
	}
	sizes := assetSizes(r.assets)
	want := map[string]int64{}
	for _, f := range r.release.Downloads() {
		if f.OS == manifest.MacOS && f.Kind == manifest.DMG {
			want[string(f.Arch)] = sizes[f.Name]
		}
	}
	for arch, size := range got {
		if w, ok := want[arch]; !ok || w != size || size <= 0 {
			add("dmgSizes.%s is %d, the release's DMG is %d", arch, size, w)
		}
	}
	for arch := range want {
		if _, ok := got[arch]; !ok {
			add("dmgSizes has no %s", arch)
		}
	}
}

func assetSizes(assets []releaseAsset) map[string]int64 {
	sizes := map[string]int64{}
	for _, a := range assets {
		sizes[a.Name] = a.Size
	}
	return sizes
}

// compareVersions orders two semver versions as the app does (`semver::Version`'s `Ord`, build
// metadata aside): by the three numbers, then a prerelease below its release, then prerelease
// identifiers left to right, numeric ones numerically and below alphanumeric ones.
func compareVersions(a, b string) int {
	coreA, preA := splitVersion(a)
	coreB, preB := splitVersion(b)
	for i := range 3 {
		if c := compareNumeric(coreA[i], coreB[i]); c != 0 {
			return c
		}
	}
	switch {
	case preA == "" && preB == "":
		return 0
	case preA == "":
		return 1
	case preB == "":
		return -1
	}
	idsA, idsB := strings.Split(preA, "."), strings.Split(preB, ".")
	for i := range min(len(idsA), len(idsB)) {
		_, errA := strconv.ParseUint(idsA[i], 10, 64)
		_, errB := strconv.ParseUint(idsB[i], 10, 64)
		var c int
		switch {
		case errA == nil && errB == nil:
			c = compareNumeric(idsA[i], idsB[i])
		case errA == nil:
			c = -1
		case errB == nil:
			c = 1
		default:
			c = strings.Compare(idsA[i], idsB[i])
		}
		if c != 0 {
			return c
		}
	}
	return len(idsA) - len(idsB)
}

func splitVersion(v string) ([3]string, string) {
	v, _, _ = strings.Cut(v, "+")
	core, pre, _ := strings.Cut(v, "-")
	var parts [3]string
	copy(parts[:], strings.SplitN(core, ".", 3))
	return parts, pre
}

// compareNumeric orders two decimal strings without leading zeros by value, at any length.
func compareNumeric(a, b string) int {
	if len(a) != len(b) {
		return len(a) - len(b)
	}
	return strings.Compare(a, b)
}

// trustedKeyIDs reads the key IDs of the main updater key (`tauri.conf.json`) and the offline
// backup one (`updater-backup.pub`) from the checkout at root: the keys the app accepts.
func trustedKeyIDs(root string) ([]string, error) {
	raw, err := os.ReadFile(filepath.Join(root, "apps", "desktop", "src-tauri", "tauri.conf.json"))
	if err != nil {
		return nil, err
	}
	var conf struct {
		Plugins struct {
			Updater struct {
				Pubkey string `json:"pubkey"`
			} `json:"updater"`
		} `json:"plugins"`
	}
	if err := json.Unmarshal(raw, &conf); err != nil {
		return nil, fmt.Errorf("reading tauri.conf.json: %w", err)
	}
	backup, err := os.ReadFile(filepath.Join(root, "apps", "desktop", "src-tauri", "updater-backup.pub"))
	if err != nil {
		return nil, err
	}
	var ids []string
	for _, key := range []string{conf.Plugins.Updater.Pubkey, string(backup)} {
		id, err := pubkeyID(key)
		if err != nil {
			return nil, err
		}
		ids = append(ids, id)
	}
	return ids, nil
}

// pubkeyID reads the key ID off a Tauri public key (base64 of minisign's two lines).
func pubkeyID(b64 string) (string, error) {
	text, err := base64.StdEncoding.DecodeString(strings.TrimSpace(b64))
	if err != nil {
		return "", fmt.Errorf("an updater public key isn't base64: %w", err)
	}
	lines := strings.Split(strings.TrimRight(string(text), "\n"), "\n")
	if len(lines) != 2 {
		return "", errors.New("an updater public key isn't minisign's two lines")
	}
	raw, err := base64.StdEncoding.DecodeString(lines[1])
	if err != nil || len(raw) != 2+8+32 {
		return "", errors.New("an updater public key isn't an Ed25519 minisign key")
	}
	id := make([]byte, 8)
	for i := range id {
		id[i] = raw[2+7-i]
	}
	return strings.ToUpper(hex.EncodeToString(id)), nil
}
