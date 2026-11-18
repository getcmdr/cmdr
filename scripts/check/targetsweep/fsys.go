package targetsweep

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// The `debug/` subdirs whose entries are build units, and the incremental dir.
var unitDirs = []string{"deps", ".fingerprint", "build", "examples"}

const incrementalDir = "incremental"

// Mode says which handoff a sweep runs in, which decides where its seed boundary comes from.
type Mode string

const (
	// ModeSeed sweeps a new worktree's copy: nothing in it is the worktree's yet, so the
	// seed is now, and the sweep stamps it.
	ModeSeed Mode = "seed"
	// ModePromote sweeps a merged worktree's copy on its way to the main clone: the
	// worktree's stamp is the seed.
	ModePromote Mode = "promote"
)

// Report is one sweep's outcome.
type Report struct {
	Plan     Plan
	Took     time.Duration
	Failures int
}

// Run sweeps the target dir at root: reads the streams and the stamp, lists the tree,
// plans, and deletes. Every deletion is safe for cargo on its own (a missing output or
// fingerprint only makes cargo rebuild that unit), so a sweep cut short leaves a usable
// tree; failed deletions are counted, not fatal.
func Run(root string, mode Mode, now time.Time) (Report, error) {
	start := time.Now()
	files, err := readStreams(root)
	if err != nil {
		return Report{}, err
	}
	live, superseded := assembleLive(files, Lanes)

	policy := Policy{Now: now}
	switch mode {
	case ModeSeed:
		policy.Seed, policy.SeedKnown = now, true
	case ModePromote:
		policy.Seed, policy.SeedKnown = readStamp(root)
	default:
		return Report{}, fmt.Errorf("unknown sweep mode %q", mode)
	}

	listing, err := list(root)
	if err != nil {
		return Report{}, err
	}
	p := plan(listing, live, policy)

	r := Report{Plan: p}
	if p.Reset {
		r.Failures += emptyDir(root)
	} else {
		for _, rel := range p.Delete {
			if err := removeUnder(root, rel); err != nil {
				r.Failures++
			}
		}
		for _, name := range superseded {
			if err := os.Remove(filepath.Join(root, LiveDir, name)); err != nil {
				r.Failures++
			}
		}
	}
	if mode == ModeSeed {
		if err := writeStamp(root, now); err != nil {
			return r, err
		}
	}
	r.Took = time.Since(start)
	return r, nil
}

// removeUnder deletes one planned path, refusing anything outside `debug/`'s swept dirs:
// the plan only ever names those (an entry of one, or a build-dir layout unit
// `debug/build/<package>/<16 hex>`), and this keeps a bug from reaching further.
func removeUnder(root, rel string) error {
	clean := path.Clean(rel)
	parts := strings.Split(clean, "/")
	entry := len(parts) == 3 && parts[0] == "debug" && parts[2] != ".." && isSweptDir(parts[1])
	buildDirUnit := len(parts) == 4 && parts[0] == "debug" && parts[1] == "build" && parts[2] != ".." &&
		bareHashRE.MatchString(parts[3])
	if !entry && !buildDirUnit {
		return fmt.Errorf("refusing to delete %q: not an entry of a swept dir", rel)
	}
	return os.RemoveAll(filepath.Join(root, filepath.FromSlash(clean)))
}

func isSweptDir(name string) bool {
	if name == incrementalDir {
		return true
	}
	for _, d := range unitDirs {
		if name == d {
			return true
		}
	}
	return false
}

// emptyDir removes every entry of root (the reset), returning how many it couldn't.
func emptyDir(root string) int {
	entries, err := os.ReadDir(root)
	if err != nil {
		return 1
	}
	failures := 0
	for _, e := range entries {
		if err := os.RemoveAll(filepath.Join(root, e.Name())); err != nil {
			failures++
		}
	}
	return failures
}

func readStreams(root string) ([]StreamFile, error) {
	dir := filepath.Join(root, LiveDir)
	entries, err := os.ReadDir(dir)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var files []StreamFile
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".jsonl") {
			continue
		}
		info, err := e.Info()
		if err != nil {
			return nil, err
		}
		content, err := os.ReadFile(filepath.Join(dir, e.Name()))
		if err != nil {
			return nil, err
		}
		files = append(files, StreamFile{Name: e.Name(), ModTime: info.ModTime(), Content: content})
	}
	return files, nil
}

// readStamp reads the seed stamp: a Unix second. Missing or unreadable reads as unknown.
func readStamp(root string) (time.Time, bool) {
	data, err := os.ReadFile(filepath.Join(root, StampFile))
	if err != nil {
		return time.Time{}, false
	}
	secs, err := strconv.ParseInt(strings.TrimSpace(string(data)), 10, 64)
	if err != nil || secs <= 0 {
		return time.Time{}, false
	}
	return time.Unix(secs, 0), true
}

// writeStamp writes the seed stamp atomically.
func writeStamp(root string, at time.Time) error {
	tmp := filepath.Join(root, StampFile+".tmp")
	if err := os.WriteFile(tmp, []byte(strconv.FormatInt(at.Unix(), 10)+"\n"), 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, filepath.Join(root, StampFile))
}

// list reads the target dir into a Listing. It never follows a symlink.
//
// It reads both of cargo's layouts, and a tree holding both (a volume an older toolchain
// built into, then a newer one): the classic one, a unit per `<name>-<16 hex>` entry of
// `deps/`, `.fingerprint/`, and `build/`; and the build-dir layout cargo 1.100 writes, one
// `build/<package>/<16 hex>/` dir per unit holding its `fingerprint/` and `out/`.
func list(root string) (Listing, error) {
	var l Listing
	debug := filepath.Join(root, "debug")
	swept := map[string]bool{}

	_, classicErr := os.Lstat(filepath.Join(debug, ".fingerprint"))
	uplifted, err := upliftedFiles(debug)
	if err != nil {
		return Listing{}, err
	}
	for _, d := range unitDirs {
		found, err := listUnits(&l, debug, d, uplifted)
		if err != nil {
			return Listing{}, err
		}
		swept[d] = found
	}
	if classicErr != nil && !l.buildDirLayout && l.LayoutProblem == "" {
		l.LayoutProblem = "there's neither a debug/.fingerprint nor a debug/build/<package>/<16 hex> unit, so this isn't a cargo layout the sweep knows"
	}
	found, err := listIncremental(&l, debug)
	if err != nil {
		return Listing{}, err
	}
	swept[incrementalDir] = found

	other, err := otherBytes(root, swept)
	if err != nil {
		return Listing{}, err
	}
	l.OtherBytes += other
	return l, nil
}

// listUnits adds the entries of one unit dir, and flags a fingerprint entry that isn't a
// `<name>-<16 hex>` dir as a changed layout. It reports whether the dir exists.
func listUnits(l *Listing, debug, name string, uplifted uplifts) (bool, error) {
	dir := filepath.Join(debug, name)
	entries, err := os.ReadDir(dir)
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	for _, e := range entries {
		if name == "build" {
			units, isPackage, err := readPackageDir(l, dir, e, uplifted)
			if err != nil {
				return false, err
			}
			if isPackage {
				l.Units = append(l.Units, units...)
				continue
			}
		}
		u, err := readUnit(dir, e)
		if err != nil {
			return false, err
		}
		u.Path = "debug/" + name + "/" + e.Name()
		u.Fingerprint = name == ".fingerprint"
		if u.Fingerprint && (!u.Hashed || !e.IsDir()) && l.LayoutProblem == "" {
			l.LayoutProblem = fmt.Sprintf("the fingerprint entry %s isn't a <name>-<16 hex> dir, so cargo's layout changed", e.Name())
		}
		l.Units = append(l.Units, u)
	}
	return true, nil
}

// readPackageDir reads a `build/` entry as a build-dir layout package dir: an unhashed dir
// whose entries are `<16 hex>` unit dirs. It says false for anything else (a classic
// `<name>-<16 hex>` entry, or an unhashed one with no unit dirs, which stays as one unit the
// sweep never deletes), and flags a package dir mixing unit dirs with anything else as a
// changed layout. A unit dir needn't hold a `fingerprint/`: cargo 1.100 leaves a bare one behind
// for a rustdoc unit the sweep removed (seen in `TestCargoLayoutMatchesTheSweepsAssumptions`).
func readPackageDir(l *Listing, buildDir string, e fs.DirEntry, uplifted uplifts) ([]Unit, bool, error) {
	if !e.IsDir() {
		return nil, false, nil
	}
	if _, hashed := unitHash(e.Name()); hashed {
		return nil, false, nil
	}
	pkgDir := filepath.Join(buildDir, e.Name())
	children, err := os.ReadDir(pkgDir)
	if err != nil {
		return nil, false, err
	}
	var units []Unit
	var stray string
	for _, c := range children {
		if !c.IsDir() || !bareHashRE.MatchString(c.Name()) {
			stray = c.Name()
			continue
		}
		unitDir := filepath.Join(pkgDir, c.Name())
		u, err := readBuildDirUnit(unitDir, c.Name(), uplifted)
		if err != nil {
			return nil, false, err
		}
		u.Path = "debug/build/" + e.Name() + "/" + c.Name()
		units = append(units, u)
	}
	if len(units) == 0 {
		return nil, false, nil
	}
	if stray != "" && l.LayoutProblem == "" {
		l.LayoutProblem = fmt.Sprintf("the build package dir %s holds %s beside its <16 hex> unit dirs, so cargo's layout changed", e.Name(), stray)
	}
	l.buildDirLayout = true
	return units, true, nil
}

// fileIdentity is what two hard links of one file share.
type fileIdentity struct{ dev, ino uint64 }

// nameSize is a file's base name and size.
type nameSize struct {
	name string
	size int64
}

// uplifts are the files cargo uplifted, directly under `debug/` and `debug/examples/`: by
// identity, where the uplift is a hard link, and by name and size, where it's a copy (cargo
// 1.100 on macOS copies a build-dir layout bin; verified with the tiny workspace of
// `TestCargoLayoutMatchesTheSweepsAssumptions`, 2026-11-18). A dead generation whose output
// happens to match a live copy's name and size stays, the safe side.
type uplifts struct {
	ids   map[fileIdentity]bool
	names map[nameSize]bool
}

// holds says whether the unit file `info`, named `name`, is the source of an uplift.
func (u uplifts) holds(name string, info fs.FileInfo) bool {
	if id, ok := fileID(info); ok && u.ids[id] {
		return true
	}
	return u.names[nameSize{name, info.Size()}]
}

// upliftedFiles reads every file cargo uplifts to.
func upliftedFiles(debug string) (uplifts, error) {
	out := uplifts{ids: map[fileIdentity]bool{}, names: map[nameSize]bool{}}
	for _, dir := range []string{debug, filepath.Join(debug, "examples")} {
		entries, err := os.ReadDir(dir)
		if errors.Is(err, fs.ErrNotExist) {
			continue
		}
		if err != nil {
			return uplifts{}, err
		}
		for _, e := range entries {
			if !e.Type().IsRegular() {
				continue
			}
			info, err := e.Info()
			if err != nil {
				return uplifts{}, err
			}
			if id, ok := fileID(info); ok {
				out.ids[id] = true
			}
			out.names[nameSize{e.Name(), info.Size()}] = true
		}
	}
	return out, nil
}

// readBuildDirUnit reads one `build/<package>/<h>/` unit. Its newest mtime is the latest of
// every entry at any depth (cargo rewrites `fingerprint/` files and a build script its `out/`
// files in place, which doesn't touch their dirs, and the walk lstats each entry anyway); it
// counts as hard-linked (`Links` 2) when a file in it is the source of one of `uplifted`, cargo's
// uplift of a bin or example (a bare link count won't do: the `.o` files in `out/` are hard links
// of the incremental cache's); and it counts as a fingerprint when nothing outside `fingerprint/` holds
// a byte: rustdoc's units, whose output lands in `doc/` under no hash.
func readBuildDirUnit(dir, hash string, uplifted uplifts) (Unit, error) {
	u := Unit{Hash: hash, Hashed: true, Links: 1}
	var outside int64
	fingerprintDir := "fingerprint" + string(filepath.Separator)
	err := filepath.WalkDir(dir, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(dir, p)
		if err != nil {
			return err
		}
		info, err := os.Lstat(p)
		if err != nil {
			return err
		}
		if info.ModTime().After(u.Newest) {
			u.Newest = info.ModTime()
		}
		if d.IsDir() {
			return nil
		}
		u.Size += info.Size()
		if uplifted.holds(d.Name(), info) {
			u.Links = 2
		}
		if !strings.HasPrefix(rel, fingerprintDir) {
			outside += info.Size()
		}
		return nil
	})
	u.Fingerprint = outside == 0
	return u, err
}

// listIncremental adds each crate dir of `incremental/`; a stray file there counts as
// other bytes. It reports whether the dir exists.
func listIncremental(l *Listing, debug string) (bool, error) {
	dir := filepath.Join(debug, incrementalDir)
	entries, err := os.ReadDir(dir)
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	for _, e := range entries {
		full := filepath.Join(dir, e.Name())
		size, err := treeStats(full)
		if err != nil {
			return false, err
		}
		if !e.IsDir() {
			l.OtherBytes += size
			continue
		}
		newest, err := newestOf(full)
		if err != nil {
			return false, err
		}
		l.Incremental = append(l.Incremental, Incremental{Path: "debug/" + incrementalDir + "/" + e.Name(), Newest: newest, Size: size})
	}
	return true, nil
}

func readUnit(dir string, e fs.DirEntry) (Unit, error) {
	full := filepath.Join(dir, e.Name())
	h, ok := unitHash(e.Name())
	u := Unit{Hash: h, Hashed: ok, Links: 1}
	if e.IsDir() {
		newest, err := newestOf(full)
		if err != nil {
			return Unit{}, err
		}
		size, err := treeStats(full)
		if err != nil {
			return Unit{}, err
		}
		u.Newest, u.Size = newest, size
		return u, nil
	}
	info, err := os.Lstat(full)
	if err != nil {
		return Unit{}, err
	}
	u.Newest, u.Size, u.Links = info.ModTime(), info.Size(), linkCount(info)
	return u, nil
}

// newestOf is the latest mtime of a dir and its direct children.
func newestOf(dir string) (time.Time, error) {
	info, err := os.Lstat(dir)
	if err != nil {
		return time.Time{}, err
	}
	newest := info.ModTime()
	entries, err := os.ReadDir(dir)
	if err != nil {
		return time.Time{}, err
	}
	for _, e := range entries {
		ci, err := e.Info()
		if err != nil {
			return time.Time{}, err
		}
		if ci.ModTime().After(newest) {
			newest = ci.ModTime()
		}
	}
	return newest, nil
}

// treeStats sums the apparent size of everything under p.
func treeStats(p string) (int64, error) {
	var size int64
	err := filepath.WalkDir(p, func(_ string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		size += info.Size()
		return nil
	})
	return size, err
}

// otherBytes sums everything outside the swept dirs.
func otherBytes(root string, swept map[string]bool) (int64, error) {
	var total int64
	entries, err := os.ReadDir(root)
	if err != nil {
		return 0, err
	}
	for _, e := range entries {
		full := filepath.Join(root, e.Name())
		if e.Name() != "debug" || !e.IsDir() {
			size, err := treeStats(full)
			if err != nil {
				return 0, err
			}
			total += size
			continue
		}
		debugEntries, err := os.ReadDir(full)
		if err != nil {
			return 0, err
		}
		for _, de := range debugEntries {
			if de.IsDir() && swept[de.Name()] {
				continue
			}
			size, err := treeStats(filepath.Join(full, de.Name()))
			if err != nil {
				return 0, err
			}
			total += size
		}
	}
	return total, nil
}
