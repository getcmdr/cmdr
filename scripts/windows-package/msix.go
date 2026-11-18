package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"encoding/xml"
	"fmt"
	"io"
	"maps"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"text/template"
)

const (
	msixDir        = "apps/desktop/packaging/msix"
	srcTauriDir    = "apps/desktop/src-tauri"
	groupPolicyDir = "apps/desktop/packaging/group-policy"
	// exeName is the app's binary, as Tauri names it from `productName` and the manifest's
	// `Application/@Executable` names it.
	exeName = "Cmdr.exe"
	// placeholderPrefix marks an identity field David hasn't filled in from Partner Center yet.
	placeholderPrefix = "PLACEHOLDER_"
)

// logos are the manifest's images, from the icon set `tauri icon` generates
// (`apps/desktop/src-tauri/icons/`, made from `brand/logos/cmdr.svg`).
var logos = []string{"StoreLogo.png", "Square150x150Logo.png", "Square44x44Logo.png"}

// Tauri's bundle-type stamp (`tauri_utils::platform::bundle_type`, verified in tauri-utils
// 2.10.2's `platform.rs`, 2026-11-18): the bundler patches the placeholder in the binary it
// packages. An NSIS-stamped exe updates itself through the NSIS installer, so the Store's copy
// must carry the unpatched placeholder: then the updater reads it as "not an installed build"
// and stays off, and the Store delivers updates (D3).
var (
	unstampedMarker = []byte("__TAURI_BUNDLE_TYPE_VAR_UNK")
	stampedMarker   = regexp.MustCompile(`__TAURI_BUNDLE_TYPE_VAR_[A-Z]{3}`)
)

// registryHives are the files that would put registry keys into the package. A packaged app's
// registry is merged with the real one, so a key shipped here would mix into the `Policies` keys
// admins set (Microsoft Learn, "MSIX registry virtualization", read 2026-11-18); Cmdr ships none.
var registryHives = []string{"registry.dat", "user.dat", "userclasses.dat"}

type identity struct {
	Name                 string `json:"name"`
	Publisher            string `json:"publisher"`
	PublisherDisplayName string `json:"publisherDisplayName"`
}

// loadIdentity reads one identity from `identity.json`. The release one must be Partner Center's:
// a placeholder fails here, before anything is built, naming the step that fills it in.
func loadIdentity(root, which string) (identity, error) {
	raw, err := os.ReadFile(filepath.Join(root, msixDir, "identity.json"))
	if err != nil {
		return identity{}, err
	}
	var ids map[string]identity
	if err := json.Unmarshal(raw, &ids); err != nil {
		return identity{}, fmt.Errorf("identity.json: %w", err)
	}
	id, ok := ids[which]
	if !ok {
		return identity{}, fmt.Errorf("identity.json has no %q identity (release or dev)", which)
	}
	for field, v := range map[string]string{"name": id.Name, "publisher": id.Publisher, "publisherDisplayName": id.PublisherDisplayName} {
		if v == "" || strings.HasPrefix(v, placeholderPrefix) {
			return identity{}, fmt.Errorf("identity.json's %s.%s is %q: copy it from Partner Center › Product identity first (docs/guides/releasing.md § Windows releases)", which, field, v)
		}
	}
	return id, nil
}

var semverCore = regexp.MustCompile(`^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$`)

// packageVersion is the MSIX version of a release: its three numbers and a zero, the fourth part
// the Store reserves. A prerelease has no MSIX spelling, so it's refused.
func packageVersion(version string) (string, error) {
	if !semverCore.MatchString(version) {
		return "", fmt.Errorf("-version must be a plain release version like 0.62.0 (an MSIX version can't carry a prerelease), got %q", version)
	}
	return version + ".0", nil
}

type msixInput struct {
	root, exe, out string
	identity       identity
	version        string
	// pack turns a staged folder into the package (`winappPack`; tests stand in for it).
	pack func(stage, manifest, out string) error
}

// buildMSIX stages, packs, and verifies the Store package.
func buildMSIX(in msixInput) error {
	if err := requireUnstamped(in.exe); err != nil {
		return err
	}
	stage, err := os.MkdirTemp("", "cmdr-msix-")
	if err != nil {
		return err
	}
	defer func() { _ = os.RemoveAll(stage) }()

	if err := copyFile(in.exe, filepath.Join(stage, exeName)); err != nil {
		return err
	}
	if err := stageResources(in.root, stage); err != nil {
		return err
	}
	for _, logo := range logos {
		if err := copyFile(filepath.Join(in.root, srcTauriDir, "icons", logo), filepath.Join(stage, "Assets", logo)); err != nil {
			return err
		}
	}
	manifest, err := renderManifest(in.root, in.identity, in.version)
	if err != nil {
		return err
	}
	manifestPath := filepath.Join(stage, "Package.appxmanifest")
	if err := os.WriteFile(manifestPath, manifest, 0o644); err != nil {
		return err
	}
	if err := refuseRegistry(stage); err != nil {
		return err
	}
	if err := in.pack(stage, manifestPath, in.out); err != nil {
		return err
	}
	return verifyMSIX(in.out, in.identity, in.version)
}

// requireUnstamped refuses an exe the bundler already patched (`stampedMarker`).
func requireUnstamped(path string) error {
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	return checkUnstamped(raw, path)
}

func checkUnstamped(raw []byte, name string) error {
	if bytes.Contains(raw, unstampedMarker) {
		return nil
	}
	if m := stampedMarker.Find(raw); m != nil {
		return fmt.Errorf("%s carries the bundler's %s stamp, so its updater would act as that install inside the Store package: package the exe `tauri build --no-bundle` left, from before `tauri bundle` ran", name, m)
	}
	return fmt.Errorf("%s carries no Tauri bundle-type marker, so it isn't a Cmdr build this tool knows", name)
}

// stageResources copies what `tauri.conf.json`'s `bundle.resources` puts beside the exe in the
// NSIS install (`resources/ai/*`), at the same relative paths, so the app finds them the same way.
func stageResources(root, stage string) error {
	raw, err := os.ReadFile(filepath.Join(root, srcTauriDir, "tauri.conf.json"))
	if err != nil {
		return err
	}
	var conf struct {
		Bundle struct {
			Resources []string `json:"resources"`
		} `json:"bundle"`
	}
	if err := json.Unmarshal(raw, &conf); err != nil {
		return fmt.Errorf("tauri.conf.json: %w", err)
	}
	for _, pattern := range conf.Bundle.Resources {
		matches, err := filepath.Glob(filepath.Join(root, srcTauriDir, filepath.FromSlash(pattern)))
		if err != nil {
			return err
		}
		if len(matches) == 0 {
			return fmt.Errorf("tauri.conf.json's resource %q matches nothing: run the build's `beforeBuildCommand` first (it fetches resources/ai)", pattern)
		}
		for _, m := range matches {
			rel, err := filepath.Rel(filepath.Join(root, srcTauriDir), m)
			if err != nil {
				return err
			}
			if err := copyFile(m, filepath.Join(stage, rel)); err != nil {
				return err
			}
		}
	}
	return nil
}

// renderManifest fills the committed manifest's identity and version, XML-escaped.
func renderManifest(root string, id identity, version string) ([]byte, error) {
	src, err := os.ReadFile(filepath.Join(root, msixDir, "Package.appxmanifest"))
	if err != nil {
		return nil, err
	}
	tmpl, err := template.New("manifest").Option("missingkey=error").Parse(string(src))
	if err != nil {
		return nil, err
	}
	esc := func(s string) string {
		var b strings.Builder
		_ = xml.EscapeText(&b, []byte(s))
		return b.String()
	}
	var b bytes.Buffer
	err = tmpl.Execute(&b, map[string]string{
		"Name": esc(id.Name), "Publisher": esc(id.Publisher), "PublisherDisplayName": esc(id.PublisherDisplayName), "Version": version,
	})
	return b.Bytes(), err
}

// refuseRegistry fails on any registry hive in the staged folder (`registryHives`).
func refuseRegistry(stage string) error {
	return filepath.WalkDir(stage, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if slices.Contains(registryHives, strings.ToLower(d.Name())) {
			return fmt.Errorf("%s would ship registry keys inside the package, which a packaged app's merged registry view mixes into admins' policies", path)
		}
		return nil
	})
}

// winappPack runs Microsoft's winapp CLI: `--no-sign` for the Store, which signs the package
// itself, and telemetry off.
func winappPack(stage, manifest, out string) error {
	cmd := exec.Command("winapp", "pack", stage, "--manifest", manifest, "--executable", exeName, "--no-sign", "--output", out)
	cmd.Env = append(os.Environ(), "WINAPP_CLI_TELEMETRY_OPTOUT=1")
	cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("winapp pack: %w", err)
	}
	return nil
}

// appxManifest is the part of `AppxManifest.xml` the checks read.
type appxManifest struct {
	Identity struct {
		Name                  string `xml:"Name,attr"`
		Publisher             string `xml:"Publisher,attr"`
		Version               string `xml:"Version,attr"`
		ProcessorArchitecture string `xml:"ProcessorArchitecture,attr"`
	} `xml:"Identity"`
	Applications struct {
		Application []struct {
			Executable string `xml:"Executable,attr"`
		} `xml:"Application"`
	} `xml:"Applications"`
}

// msixPackage is a built package's files, by lowercased `/`-separated name.
type msixPackage struct {
	path  string
	files map[string]*zip.File
}

func (p msixPackage) read(name string) ([]byte, error) {
	f, ok := p.files[strings.ToLower(name)]
	if !ok {
		return nil, fmt.Errorf("%s has no %s", p.path, name)
	}
	rc, err := f.Open()
	if err != nil {
		return nil, err
	}
	defer func() { _ = rc.Close() }()
	return io.ReadAll(rc)
}

// verifyMSIX opens a built package and holds it to what the Store should get: this identity
// and version for x64, an `Application` running `Cmdr.exe`, that exe unstamped (its updater
// off), the logos and the bundle's resources present, and no registry hive.
func verifyMSIX(path string, id identity, version string) error {
	zr, err := zip.OpenReader(path)
	if err != nil {
		return fmt.Errorf("%s isn't an MSIX package: %w", path, err)
	}
	defer func() { _ = zr.Close() }()
	pkg := msixPackage{path: path, files: map[string]*zip.File{}}
	for _, f := range zr.File {
		name := strings.ReplaceAll(f.Name, "\\", "/")
		pkg.files[strings.ToLower(name)] = f
		if slices.Contains(registryHives, strings.ToLower(filepath.Base(name))) {
			return fmt.Errorf("%s ships %s, registry keys a packaged app's merged view would mix into admins' policies", path, name)
		}
	}
	raw, err := pkg.read("AppxManifest.xml")
	if err != nil {
		return err
	}
	problems, err := manifestProblems(raw, id, version)
	if err != nil {
		return fmt.Errorf("%s's AppxManifest.xml: %w", path, err)
	}
	problems = append(problems, contentProblems(pkg)...)
	if len(problems) > 0 {
		return fmt.Errorf("%s isn't the Store package of Cmdr %s:\n  - %s", path, version, strings.Join(problems, "\n  - "))
	}
	fmt.Printf("%s: Cmdr %s for x64 as %s, updater off, no registry keys\n", filepath.Base(path), version, id.Name)
	return nil
}

// manifestProblems checks the packed `AppxManifest.xml`: identity, version, architecture, the one
// application, and no `Policies` registry path.
func manifestProblems(raw []byte, id identity, version string) ([]string, error) {
	var m appxManifest
	if err := xml.Unmarshal(raw, &m); err != nil {
		return nil, err
	}
	var problems []string
	if m.Identity.Name != id.Name || m.Identity.Publisher != id.Publisher {
		problems = append(problems, fmt.Sprintf("its identity is %q by %q, want %q by %q", m.Identity.Name, m.Identity.Publisher, id.Name, id.Publisher))
	}
	if m.Identity.Version != version {
		problems = append(problems, fmt.Sprintf("its version is %q, want %q", m.Identity.Version, version))
	}
	if m.Identity.ProcessorArchitecture != "x64" {
		problems = append(problems, fmt.Sprintf("its architecture is %q, want x64", m.Identity.ProcessorArchitecture))
	}
	if len(m.Applications.Application) != 1 || m.Applications.Application[0].Executable != exeName {
		problems = append(problems, "it should hold one application, running "+exeName)
	}
	if strings.Contains(string(raw), `\Policies\`) {
		problems = append(problems, "its manifest names a Policies registry path")
	}
	return problems, nil
}

// contentProblems checks the packed files: the exe unstamped, the logos, and the resources.
func contentProblems(pkg msixPackage) []string {
	var problems []string
	if exe, err := pkg.read(exeName); err != nil {
		problems = append(problems, err.Error())
	} else if err := checkUnstamped(exe, exeName); err != nil {
		problems = append(problems, err.Error())
	}
	for _, logo := range logos {
		if _, ok := pkg.files[strings.ToLower("Assets/"+logo)]; !ok {
			problems = append(problems, "it has no Assets/"+logo)
		}
	}
	if !slices.ContainsFunc(slices.Collect(maps.Keys(pkg.files)), func(n string) bool { return strings.HasPrefix(n, "resources/") }) {
		problems = append(problems, "it has none of the bundle's resources/")
	}
	return problems
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer func() { _ = in.Close() }()
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return err
	}
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
		_ = out.Close()
		return err
	}
	return out.Close()
}
