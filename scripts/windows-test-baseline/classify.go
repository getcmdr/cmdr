package main

import (
	"regexp"
	"strings"
)

// cluster is one cause of a Windows failure.
type cluster struct {
	Key   string
	Label string
}

var (
	clusterNativeTempdir  = cluster{"N", "N, a native tempdir where an app path belongs"}
	clusterFinalize       = cluster{"F", "F, the local engine's finalize hands a native path"}
	clusterStdArithmetic  = cluster{"S", "S, `std::path` arithmetic on an app path"}
	clusterIdentity       = cluster{"task-35", "Not this plan: mounted-inside and node identity"}
	clusterOpenFile       = cluster{"1226", "Not this plan: a file another handle holds open"}
	clusterRootVolume     = cluster{"task-34", "Not this plan: root-volume lookups"}
	clusterRemoteErrno    = cluster{"errno", "Not this plan: a Linux errno read through the host's codes"}
	clusterWindowsOnly    = cluster{"windows-only", "Not this plan: a Windows-only test expectation"}
	clusterProductGap     = cluster{"product", "Not this plan: Windows behavior the product hasn't met yet"}
	clusterNativeHome     = cluster{"H", "H, a native home path where an app path belongs"}
	clusterPosixFixture   = cluster{"P", "P, a POSIX fixture path the host reads in its own flavor"}
	clusterSymlink        = cluster{"symlink", "A link test without the right to make links"}
	clusterAppendTruncate = cluster{"append-truncate", "A truncate on an append-only handle"}
	clusterWindowsNames   = cluster{"windows-names", "A fixture name Windows refuses to create"}
	clusterHomeEnv        = cluster{"home-env", "A test reading `HOME`, which Windows doesn't set"}
	clusterZipCreator     = cluster{"zip-creator", "A zip mode written under the host's DOS creator"}
	clusterTimeout        = cluster{"timeout", "Timed out"}
	clusterUnread         = cluster{"unread", "Cause unread"}
)

// tag is one failure with its cluster and owner.
type tag struct {
	ID      testID
	Cluster cluster
	Owner   string
	// Unread is set when the output named no cause and the cluster came from the nearest sibling.
	Unread bool
}

// A test named here has a known cause its output doesn't spell (from the plan's § "Windows failures, mapped").
var namedCauses = []struct {
	crate, contains string
	cluster         cluster
}{
	{"cmdr-s3", "nanosecond", clusterWindowsOnly},
	{"cmdr-nfs", "errors_test", clusterRemoteErrno},
	{"cmdr-adb", "an_unlisted_device_errno_arrives_as_its_linux_class", clusterRemoteErrno},
	{"cmdr-index", "path_resolution", clusterWindowsOnly},
	{"cmdr-index", "legacy_split", clusterOpenFile},
	{"cmdr-index", "building_splits", clusterOpenFile},
	{"cmdr", "data_wipe", clusterWindowsOnly},
	{"cmdr", "tools::sensitive", clusterWindowsOnly},
	{"cmdr", "crash_reporter", clusterWindowsOnly},
	{"cmdr", "subprocess", clusterWindowsOnly},
	{"cmdr", "instance_lock", clusterProductGap},
	{"cmdr", "logging::rotating_file", clusterProductGap},
	{"cmdr", "resolve_location", clusterRootVolume},
	{"cmdr", "a_home_relative_path_resolves_to_root", clusterRootVolume},
	// A mode the `zip` crate wrote under its host's DOS creator reads back as DOS's stand-in.
	{"cmdr-archive", "unix_mode_in_the_external_attributes", clusterZipCreator},
	{"cmdr-archive", "no_recorded_mode", clusterZipCreator},
	{"cmdr-operations", "producer_preserves_names_empty_entries_metadata_and_level", clusterZipCreator},
}

var (
	nativeTempdirRE = regexp.MustCompile(`not an absolute Windows app path|[A-Za-z]:\\Users\\[^\\"]+\\AppData\\Local\\Temp|\\crates\\cmdr-index\\cmdr`)
	finalizeRE      = regexp.MustCompile(`rename_local_exclusive`)
	// A `\` right after a `/`-separated path or a scheme root: `/srv/data\x`, `sftp://h:22\srv`, or doubled as
	// `Debug` prints it.
	stdArithmeticRE = regexp.MustCompile(`(?:^|[\s"'(\[{=:])(?:/[^\s"'\\]*|[a-z][a-z0-9+.-]*://[^\s"'\\]*)\\{1,2}[^\s"'\\]`)
	// A path that starts at a bare `\`: a POSIX or volume-relative path `std::path` re-rooted (`NotFound("\\sdcard")`).
	bareRootRE = regexp.MustCompile(`(?:^|[\s"'(=])\\{1,2}[A-Za-z0-9_][^\s"']*`)
	// An NFS export matched by a `std::path` prefix on Windows (the plan's 15 `ExportNotFound` cells).
	exportRE = regexp.MustCompile(`ExportNotFound`)
	// The boot volume answering `/` for a drive path: task 34's root-volume lookup.
	rootVolumeRE = regexp.MustCompile(`PathOutsideVolume[^\n]*root: Some\("/"\)`)
	identityRE   = regexp.MustCompile(`MountedVolumeInside|mounted_volume_inside|Unverifiable`)
	// A sharing violation, as `Display` and as `Debug` (`Os { code: 32, … }`) print it.
	openFileRE = regexp.MustCompile(`os error 32\b|code: 32\b|ERROR_SHARING_VIOLATION`)
	// Windows' ERROR_PRIVILEGE_NOT_HELD: a symlink without Developer Mode or elevation.
	symlinkRE = regexp.MustCompile(`code: 1314\b|os error 1314\b|A required privilege is not held`)
	// The in-flight ledger's `set_len(0)` on its append handle (`FILE_APPEND_DATA` alone).
	appendTruncateRE = regexp.MustCompile(`start the test ledger empty: Os \{ code: 5\b`)
	// ERROR_INVALID_NAME: a fixture creating `a?b.txt` or `12:00`.
	windowsNamesRE = regexp.MustCompile(`code: 123\b|os error 123\b`)
	homeEnvRE      = regexp.MustCompile(`HOME is set: NotPresent`)
	// A fixture's POSIX absolute path, which a Windows host reads as relative or foreign.
	posixFixtureRE = regexp.MustCompile(`"/(?:Users|Volumes|srv|tmp|home|sdcard|mnt)/`)
	// The home folder spelled natively (`Debug` doubles the `\`) where an app path was expected.
	nativeHomeRE = regexp.MustCompile(`[A-Za-z]:\\\\Users\\\\`)
	timeoutRE    = regexp.MustCompile(`(?i)timed out|test timeout|TIMEOUT`)
)

// classifyText reads a cause from a failure's output, or `clusterUnread`.
func classifyText(text string) cluster {
	switch {
	case symlinkRE.MatchString(text):
		return clusterSymlink
	case appendTruncateRE.MatchString(text):
		return clusterAppendTruncate
	case windowsNamesRE.MatchString(text):
		return clusterWindowsNames
	case homeEnvRE.MatchString(text):
		return clusterHomeEnv
	case identityRE.MatchString(text):
		return clusterIdentity
	case openFileRE.MatchString(text):
		return clusterOpenFile
	case rootVolumeRE.MatchString(text):
		return clusterRootVolume
	case finalizeRE.MatchString(text):
		return clusterFinalize
	case nativeTempdirRE.MatchString(text):
		return clusterNativeTempdir
	case nativeHomeRE.MatchString(text):
		return clusterNativeHome
	case stdArithmeticRE.MatchString(text), bareRootRE.MatchString(text), exportRE.MatchString(text):
		return clusterStdArithmetic
	case timeoutRE.MatchString(text):
		return clusterTimeout
	case posixFixtureRE.MatchString(text):
		return clusterPosixFixture
	}
	return clusterUnread
}

func classifyOne(f failure) cluster {
	// nextest's own verdict: a test that ran out of time says nothing about the cause its name is known for.
	if strings.Contains(f.Text, "test timeout") {
		return clusterTimeout
	}
	for _, named := range namedCauses {
		if f.ID.Crate == named.crate && strings.Contains(f.ID.Test, named.contains) {
			return named.cluster
		}
	}
	return classifyText(f.Text)
}

// ownCause marks the causes one test's own fixture or call has, which say nothing about a sibling's failure.
var ownCause = map[cluster]bool{clusterSymlink: true, clusterAppendTruncate: true, clusterWindowsNames: true, clusterHomeEnv: true, clusterZipCreator: true}

// classifyAll tags every failure; an unread one takes the cluster of its nearest classified sibling (the longest
// shared module path in the same crate), marked as unread. A timeout is no cause and an `ownCause` is that test's alone, so nothing inherits either.
func classifyAll(failures []failure) []tag {
	tags := make([]tag, len(failures))
	for i, f := range failures {
		tags[i] = tag{ID: f.ID, Cluster: classifyOne(f)}
	}
	for i := range tags {
		if tags[i].Cluster != clusterUnread {
			continue
		}
		best, bestShared := -1, -1
		for j := range tags {
			if j == i || tags[j].ID.Crate != tags[i].ID.Crate || tags[j].Cluster == clusterUnread || tags[j].Cluster == clusterTimeout || tags[j].Unread || ownCause[tags[j].Cluster] {
				continue
			}
			if shared := sharedModules(tags[i].ID.Test, tags[j].ID.Test); shared > bestShared {
				best, bestShared = j, shared
			}
		}
		tags[i].Unread = true
		if best >= 0 {
			tags[i].Cluster = tags[best].Cluster
		}
	}
	for i := range tags {
		tags[i].Owner = owner(tags[i].ID, tags[i].Cluster)
	}
	return tags
}

// sharedModules counts the leading `::` segments two test paths share.
func sharedModules(a, b string) int {
	as, bs := strings.Split(a, "::"), strings.Split(b, "::")
	n := 0
	for n < len(as)-1 && n < len(bs)-1 && as[n] == bs[n] {
		n++
	}
	return n
}

// The app modules the app path conversion's file system and listings half owns; the rest of the app is its
// operations and tools half (`docs/specs/app-path-plan.md` names the two).
var appFileSystemHalf = []string{"file_system::", "listing_host::", "volumes::", "commands::", "go_to_path::", "favorites::", "recents::", "file_viewer::", "native_drag::", "about::", "network::"}

// fixedOwners names the owner of each cluster that has one whatever the crate.
var fixedOwners = map[cluster]string{
	clusterIdentity:       "Windows plan task 35",
	clusterOpenFile:       "#1226",
	clusterRootVolume:     "Windows plan task 34",
	clusterRemoteErrno:    "the Windows build state's backend errno sentence",
	clusterWindowsOnly:    "#1232 and the Windows build state",
	clusterProductGap:     "its Windows plan task",
	clusterFinalize:       "`cmdr-local`'s typed rename, then `cmdr-operations`' conversion",
	clusterSymlink:        "a `can_create_symlinks()` skip in the test",
	clusterAppendTruncate: "the in-flight ledger's emptied open",
	clusterWindowsNames:   "a Unix-only gate on the test",
	clusterHomeEnv:        "the test reading the home the code reads",
	clusterZipCreator:     "a Unix creator on a zip entry carrying a mode",
}

// owner names who turns a failure green. A path cause belongs to its crate's app path conversion (the crate heading
// says which); the app's splits in two halves.
func owner(id testID, c cluster) string {
	if fixed, ok := fixedOwners[c]; ok {
		return fixed
	}
	if id.Crate == "cmdr" {
		for _, prefix := range appFileSystemHalf {
			if strings.HasPrefix(id.Test, prefix) {
				return "the app's file system and listings conversion"
			}
		}
		return "the app's operations and tools conversion"
	}
	return "this crate's app path conversion"
}
