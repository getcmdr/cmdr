// Package manifest is the release asset manifest: one row per file a release publishes, naming
// its OS, arch, kind, filename, and (for an update payload) its `latest.json` platform key.
// Everything that handles release files derives from it: the release pipeline's plan (`guard`),
// `latest.json`, the checksums, the update-archive vetting, the SBOMs and their attestations
// (`release-pipeline.yml`, through `scripts/release-assets.sh`), and the Go release tools
// (`release-finish`, `release-mirror`, `release-evidence`). The website reads the `downloads` block
// `latest.json` gets from it. Adding a platform is rows here plus the build job that makes them,
// then its OS in `ShippedOSes`.
package manifest

import (
	"fmt"
	"slices"
	"strings"
)

// OS is the platform a file is for, as the repo spells it everywhere but the updater's keys.
type OS string

const (
	MacOS   OS = "macos"
	Linux   OS = "linux"
	Windows OS = "windows"
)

// Arch is the CPU a file runs on, in the repo's spelling (`x86_64`, never Tauri's `x64`, which
// only filenames use). `universal` is a macOS fat binary holding both.
type Arch string

const (
	AArch64   Arch = "aarch64"
	X86_64    Arch = "x86_64"
	Universal Arch = "universal"
)

// Kind is what a file is. Whether people download it comes from the kind; whether the updater
// fetches it comes from the row's `UpdaterKey`, since one file can be both (a Windows installer).
type Kind string

const (
	// DMG is the macOS disk image people download.
	DMG Kind = "dmg"
	// Pkg is the macOS installer package, for MDM deployment.
	Pkg Kind = "pkg"
	// AppArchive is the macOS updater payload, `Cmdr.app` as a `.tar.gz`.
	AppArchive Kind = "app-archive"
	// RustSBOM is the CycloneDX SBOM of the Rust crate graph for its `Targets`.
	RustSBOM Kind = "rust-sbom"
	// FrontendSBOM is the CycloneDX SBOM of the npm packages in the built frontend, which every
	// build ships.
	FrontendSBOM Kind = "frontend-sbom"
	// AppImage, Deb, NSIS, and MSIX are the Linux and Windows packages. The AppImage and the NSIS
	// `-setup.exe` double as their OS's update payload (a row with an `UpdaterKey`); the MSIX is
	// the Microsoft Store's upload, unsigned (the Store signs it). Linux has no rows yet
	// (`docs/specs/linux-builds-plan.md`).
	AppImage Kind = "appimage"
	Deb      Kind = "deb"
	NSIS     Kind = "nsis"
	MSIX     Kind = "msix"
	// GroupPolicy is the Windows Group Policy template as one zip (the ADMX files plus `en-US/`
	// with their ADMLs, `apps/desktop/packaging/group-policy/`): a download for admins, not a
	// build of the app, so no SBOM describes it and it has no arch.
	GroupPolicy Kind = "group-policy"
	// AdvisoryList is the release's security advisory list, minisign-signed like an update
	// payload: the installs that can't open their update before running it (Windows, Linux) read
	// it through `latest.json`'s `securityAdvisories` (`File.Signed`).
	AdvisoryList Kind = "advisory-list"
)

// downloadKinds are the kinds people download from the website (and an MDM fetches): each is in
// `checksums.txt`, on the EU mirror, and in `latest.json`'s `downloads` block.
var downloadKinds = []Kind{DMG, Pkg, AppImage, Deb, NSIS, MSIX, GroupPolicy}

var knownKinds = []Kind{DMG, Pkg, AppArchive, RustSBOM, FrontendSBOM, AppImage, Deb, NSIS, MSIX, GroupPolicy, AdvisoryList}

// Asset is one row: a file every release carries (or, if Optional, may carry).
type Asset struct {
	OS   OS
	Arch Arch
	Kind Kind
	// File is the published name with `{version}` for the app version. Tauri's bundler spells the
	// Intel arch `x64` in filenames, so these do too.
	File string
	// UpdaterKey is the file's `latest.json` platform key (`darwin-aarch64`, later
	// `windows-x86_64` and `linux-x86_64`): set only on an update payload, which then carries a
	// minisign `.sig`. `Validate` holds it to a key the app reads (`updaterKeys`).
	UpdaterKey string
	// Optional files may be missing from a release without failing it (the pkg, which needs an
	// installer certificate).
	Optional bool
	// Targets are the Rust target triples a binary is built from, or that a Rust SBOM describes.
	// A Rust SBOM is bound to every binary that shares one of its targets.
	Targets []string
}

const (
	macARM   = "aarch64-apple-darwin"
	macIntel = "x86_64-apple-darwin"
	winX64   = "x86_64-pc-windows-msvc"
)

// Assets is the manifest, every platform's rows whether it ships yet or not. Order matters only
// for readability and output order: `latest.json` lists its platforms in update-row order
// (universal first, as every release has, so a new OS's rows go after macOS's).
var Assets = []Asset{
	{OS: MacOS, Arch: Universal, Kind: AppArchive, File: "Cmdr_{version}_universal.app.tar.gz", UpdaterKey: "darwin-universal", Targets: []string{macARM, macIntel}},
	{OS: MacOS, Arch: AArch64, Kind: AppArchive, File: "Cmdr_{version}_aarch64.app.tar.gz", UpdaterKey: "darwin-aarch64", Targets: []string{macARM}},
	{OS: MacOS, Arch: X86_64, Kind: AppArchive, File: "Cmdr_{version}_x64.app.tar.gz", UpdaterKey: "darwin-x86_64", Targets: []string{macIntel}},
	{OS: MacOS, Arch: AArch64, Kind: DMG, File: "Cmdr_{version}_aarch64.dmg", Targets: []string{macARM}},
	{OS: MacOS, Arch: X86_64, Kind: DMG, File: "Cmdr_{version}_x64.dmg", Targets: []string{macIntel}},
	{OS: MacOS, Arch: Universal, Kind: DMG, File: "Cmdr_{version}_universal.dmg", Targets: []string{macARM, macIntel}},
	{OS: MacOS, Arch: Universal, Kind: Pkg, File: "Cmdr_{version}_universal.pkg", Optional: true, Targets: []string{macARM, macIntel}},
	{OS: MacOS, Arch: AArch64, Kind: RustSBOM, File: "Cmdr_{version}_aarch64.rust.cdx.json", Targets: []string{macARM}},
	{OS: MacOS, Arch: X86_64, Kind: RustSBOM, File: "Cmdr_{version}_x64.rust.cdx.json", Targets: []string{macIntel}},
	{Kind: FrontendSBOM, File: "Cmdr_{version}_frontend.cdx.json"},
	{OS: Windows, Arch: X86_64, Kind: NSIS, File: "Cmdr_{version}_x64-setup.exe", UpdaterKey: "windows-x86_64", Targets: []string{winX64}},
	{OS: Windows, Arch: X86_64, Kind: MSIX, File: "Cmdr_{version}_x64.msix", Targets: []string{winX64}},
	{OS: Windows, Arch: X86_64, Kind: RustSBOM, File: "Cmdr_{version}_windows_x64.rust.cdx.json", Targets: []string{winX64}},
	{OS: Windows, Kind: GroupPolicy, File: "cmdr-group-policy-{version}.zip"},
	{Kind: AdvisoryList, File: "Cmdr_{version}_security-advisories.json"},
}

// shipped keeps the rows of the shipped OSes, plus the ones that belong to no one OS (the
// frontend SBOM). The advisory list ships once some update payload signs its version: those
// installs (Windows, Linux) read the list from `latest.json`, while macOS reads the one inside
// the bundle it extracts, so a macOS-only release publishes exactly what it always did.
func shipped(assets []Asset, oses []OS) []Asset {
	var out, lists []Asset
	signsVersion := false
	for _, a := range assets {
		switch {
		case a.OS != "" && !slices.Contains(oses, a.OS):
			continue
		case a.Kind == AdvisoryList:
			lists = append(lists, a)
			continue
		case a.UpdaterKey != "" && a.OS != MacOS:
			signsVersion = true
		}
		out = append(out, a)
	}
	if signsVersion {
		out = append(out, lists...)
	}
	return out
}

// File is one manifest row resolved for a version.
type File struct {
	Asset
	Name string
}

// Download reports whether people download this file (website, MDM).
func (f File) Download() bool { return slices.Contains(downloadKinds, f.Kind) }

// Update reports whether the updater fetches this file, so it's signed and in `latest.json`.
func (f File) Update() bool { return f.UpdaterKey != "" }

// SignsVersion reports whether this update payload's signature must name the version
// (`tauri signer sign --app-version <version>`, which writes `version:<v>` into minisign's signed
// trusted comment). Off macOS the updater can't open an installer before it runs, so that field is
// the only signed place the version lives, and it refuses a signature without it
// (`apps/desktop/src-tauri/src/updater/DETAILS.md` § Windows and Linux). macOS reads the version
// from the signed bundle's `Info.plist` instead.
func (f File) SignsVersion() bool { return f.Update() && f.OS != MacOS }

// updaterKeys are the `latest.json` keys the app reads for a row's OS and arch, mirroring
// `InstallKind::platform_keys` in `apps/desktop/src-tauri/src/updater/install_kind.rs`.
func updaterKeys(os OS, arch Arch) []string {
	switch os {
	case MacOS:
		return []string{"darwin-" + string(arch)}
	case Windows:
		return []string{"windows-" + string(arch) + "-nsis", "windows-" + string(arch)}
	case Linux:
		return []string{"linux-" + string(arch) + "-appimage", "linux-" + string(arch)}
	}
	return nil
}

// Binary reports whether this is a build of the app (a download or an update payload), the
// files provenance and SBOMs are about. The Group Policy template is a download but no build.
func (f File) Binary() bool { return (f.Download() && f.Kind != GroupPolicy) || f.Update() }

// Signed reports whether the updater key signs this file (it then has a `.sig`): every update
// payload, and the advisory list the updater reads beside one.
func (f File) Signed() bool { return f.Update() || f.Kind == AdvisoryList }

// SBOM reports whether this file is an SBOM.
func (f File) SBOM() bool { return f.Kind == RustSBOM || f.Kind == FrontendSBOM }

// Signature is the name of a signed file's minisign signature (`Signed`).
func (f File) Signature() string { return f.Name + ".sig" }

// Slug names a row for the pipeline's step outputs: `<os>-<arch>`, or the kind for a file that
// belongs to no one platform (`frontend-sbom` → `frontend`).
func (f File) Slug() string {
	if f.OS == "" {
		return strings.TrimSuffix(string(f.Kind), "-sbom")
	}
	return string(f.OS) + "-" + string(f.Arch)
}

// Release is the manifest resolved for one version.
type Release struct {
	Version string
	Files   []File
}

// For resolves what a release of this version publishes: the rows of the OSes `ShippedOSes`
// ships at that version.
func For(version string) Release {
	return ForShipping(version, ShippedOSes)
}

// ForShipping is `For` with another shipping list (tests, and the Windows-on dry run's).
func ForShipping(version string, shipping []Shipping) Release {
	oses, _ := shippedFor(version, shipping)
	return ForOSes(version, oses...)
}

// ForOSes resolves what a release would publish with oses shipped (`For` with `ShippedOSes`;
// tests pass others).
func ForOSes(version string, oses ...OS) Release {
	return Resolve(version, shipped(Assets, oses))
}

// All resolves every row, shipped or not, for the checks that hold the pipeline's wiring to the
// whole manifest (`plan`'s vetted OSes and attest steps), so a platform's jobs are in place and
// tested before it ships.
func All(version string) Release {
	return Resolve(version, Assets)
}

// Ships reports whether this release publishes files for os.
func (r Release) Ships(os OS) bool {
	return slices.ContainsFunc(r.Files, func(f File) bool { return f.OS == os })
}

// Resolve resolves any list of rows for a version: For with the real manifest, tests with more.
func Resolve(version string, assets []Asset) Release {
	r := Release{Version: version}
	for _, a := range assets {
		r.Files = append(r.Files, File{Asset: a, Name: strings.ReplaceAll(a.File, "{version}", version)})
	}
	return r
}

func (r Release) filter(keep func(File) bool) []File {
	var out []File
	for _, f := range r.Files {
		if keep(f) {
			out = append(out, f)
		}
	}
	return out
}

// Updates are the update payloads, in manifest order.
func (r Release) Updates() []File { return r.filter(File.Update) }

// Signed are the files the updater key signs, each with a `.sig`, in manifest order.
func (r Release) Signed() []File { return r.filter(File.Signed) }

// AdvisoryList is the release's signed advisory list, when it ships one.
func (r Release) AdvisoryList() (File, bool) {
	for _, f := range r.Files {
		if f.Kind == AdvisoryList {
			return f, true
		}
	}
	return File{}, false
}

// Downloads are the files people download, in manifest order.
func (r Release) Downloads() []File { return r.filter(File.Download) }

// Binaries are every build of the app, in manifest order.
func (r Release) Binaries() []File { return r.filter(File.Binary) }

// SBOMs are the SBOM files, in manifest order.
func (r Release) SBOMs() []File { return r.filter(File.SBOM) }

// Names maps files to their names.
func Names(files []File) []string {
	out := make([]string, 0, len(files))
	for _, f := range files {
		out = append(out, f.Name)
	}
	return out
}

// Required drops the optional files.
func Required(files []File) []File {
	var out []File
	for _, f := range files {
		if !f.Optional {
			out = append(out, f)
		}
	}
	return out
}

// Present keeps the files a release has: every required one (each missing one is an error) and
// the optional ones it carries.
func Present(files []File, have []string) ([]File, error) {
	out := []File{}
	var missing []string
	for _, f := range files {
		switch {
		case slices.Contains(have, f.Name):
			out = append(out, f)
		case !f.Optional:
			missing = append(missing, f.Name)
		}
	}
	if len(missing) > 0 {
		return nil, fmt.Errorf("the release is missing %s", strings.Join(missing, ", "))
	}
	return out, nil
}

// Subjects are the binaries an SBOM describes: for a Rust SBOM, every binary built from one of its
// targets (the universal build holds both slices, so both Rust SBOMs describe it); for the
// frontend SBOM, every binary.
func (r Release) Subjects(sbom File) []File {
	return r.filter(func(f File) bool {
		if !f.Binary() {
			return false
		}
		if sbom.Kind == FrontendSBOM {
			return true
		}
		return slices.ContainsFunc(f.Targets, func(t string) bool { return slices.Contains(sbom.Targets, t) })
	})
}

// updatePayloadProblems checks an update payload's own rules, noting its key in `keys` (the
// updater keys seen so far).
func updatePayloadProblems(f File, keys map[string]bool) []string {
	var problems []string
	if keys[f.UpdaterKey] {
		problems = append(problems, fmt.Sprintf("updater key %s is on two files", f.UpdaterKey))
	}
	keys[f.UpdaterKey] = true
	if !slices.Contains(updaterKeys(f.OS, f.Arch), f.UpdaterKey) {
		problems = append(problems, fmt.Sprintf("%s has updater key %s, which no %s %s install reads", f.Name, f.UpdaterKey, f.OS, f.Arch))
	}
	if f.Optional {
		problems = append(problems, fmt.Sprintf("%s is an update payload, so it can't be optional", f.Name))
	}
	return problems
}

// rowProblems checks one row on its own: a known kind, the version in its name, and the fields
// its kind needs.
func rowProblems(f File) []string {
	var problems []string
	if !slices.Contains(knownKinds, f.Kind) {
		problems = append(problems, fmt.Sprintf("%s has an unknown kind %q", f.Name, f.Kind))
	}
	if !strings.Contains(f.File, "{version}") {
		problems = append(problems, fmt.Sprintf("%s doesn't carry the version", f.File))
	}
	if f.Binary() && (f.OS == "" || f.Arch == "" || len(f.Targets) == 0) {
		problems = append(problems, fmt.Sprintf("%s is a build, so it needs an OS, an arch, and its Rust targets", f.Name))
	}
	if f.Kind == AdvisoryList && (f.OS != "" || f.Optional) {
		problems = append(problems, fmt.Sprintf("%s is the advisory list, which belongs to no one OS and can't be optional", f.Name))
	}
	return problems
}

// Validate checks the manifest's own rules, the ones a new row could break.
func (r Release) Validate() error {
	var problems []string
	names, keys, slugs := map[string]bool{}, map[string]bool{}, map[string]bool{}
	for _, f := range r.Files {
		problems = append(problems, rowProblems(f)...)
		if names[f.Name] {
			problems = append(problems, fmt.Sprintf("%s is listed twice", f.Name))
		}
		names[f.Name] = true
		if f.Update() {
			problems = append(problems, updatePayloadProblems(f, keys)...)
		}
		if f.SBOM() {
			if slugs[f.Slug()] {
				problems = append(problems, fmt.Sprintf("two SBOMs are both %s", f.Slug()))
			}
			slugs[f.Slug()] = true
		}
	}
	problems = append(problems, r.uncoveredTargets()...)
	if len(problems) > 0 {
		return fmt.Errorf("the release asset manifest is inconsistent:\n  - %s", strings.Join(problems, "\n  - "))
	}
	return nil
}

// uncoveredTargets names each Rust target a binary is built from that no Rust SBOM describes:
// a build with no SBOM would ship without one, silently.
func (r Release) uncoveredTargets() []string {
	var described []string
	for _, s := range r.SBOMs() {
		described = append(described, s.Targets...)
	}
	var problems []string
	for _, b := range r.Binaries() {
		for _, t := range b.Targets {
			if !slices.Contains(described, t) {
				problems = append(problems, fmt.Sprintf("%s is built for %s, which no Rust SBOM row describes", b.Name, t))
			}
		}
	}
	return problems
}
