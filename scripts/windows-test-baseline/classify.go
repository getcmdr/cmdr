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
	clusterNativeTempdir = cluster{"N", "N, a native tempdir where an app path belongs"}
	clusterFinalize      = cluster{"F", "F, the local engine's finalize hands a native path"}
	clusterStdArithmetic = cluster{"S", "S, `std::path` arithmetic on an app path"}
	clusterIdentity      = cluster{"task-35", "Not this plan: mounted-inside and node identity"}
	clusterOpenFile      = cluster{"1226", "Not this plan: a file another handle holds open"}
	clusterRootVolume    = cluster{"task-34", "Not this plan: root-volume lookups"}
	clusterRemoteErrno   = cluster{"errno", "Not this plan: a Linux errno read through the host's codes"}
	clusterWindowsOnly   = cluster{"windows-only", "Not this plan: a Windows-only test expectation"}
	clusterProductGap    = cluster{"product", "Not this plan: Windows behavior the product hasn't met yet"}
	clusterTimeout       = cluster{"timeout", "Timed out"}
	clusterUnread        = cluster{"unread", "Cause unread"}
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
	identityRE   = regexp.MustCompile(`MountedVolumeInside|Unverifiable`)
	// A sharing violation, as `Display` and as `Debug` (`Os { code: 32, … }`) print it.
	openFileRE = regexp.MustCompile(`os error 32\b|code: 32\b|ERROR_SHARING_VIOLATION`)
	timeoutRE  = regexp.MustCompile(`(?i)timed out|test timeout|TIMEOUT`)
)

// classifyText reads a cause from a failure's output, or `clusterUnread`.
func classifyText(text string) cluster {
	switch {
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
	case stdArithmeticRE.MatchString(text), bareRootRE.MatchString(text), exportRE.MatchString(text):
		return clusterStdArithmetic
	case timeoutRE.MatchString(text):
		return clusterTimeout
	}
	return clusterUnread
}

func classifyOne(f failure) cluster {
	for _, named := range namedCauses {
		if f.ID.Crate == named.crate && strings.Contains(f.ID.Test, named.contains) {
			return named.cluster
		}
	}
	return classifyText(f.Text)
}

// classifyAll tags every failure; an unread one takes the cluster of its nearest classified sibling (the longest
// shared module path in the same crate), marked as unread. A timeout is no cause, so nothing inherits it.
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
			if j == i || tags[j].ID.Crate != tags[i].ID.Crate || tags[j].Cluster == clusterUnread || tags[j].Cluster == clusterTimeout || tags[j].Unread {
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

// owner names who turns a failure green. A path cause belongs to its crate's app path conversion (the crate heading
// says which); the app's splits in two halves.
func owner(id testID, c cluster) string {
	switch c {
	case clusterIdentity:
		return "Windows plan task 35"
	case clusterOpenFile:
		return "#1226"
	case clusterRootVolume:
		return "Windows plan task 34"
	case clusterRemoteErrno:
		return "the Windows build state's backend errno sentence"
	case clusterWindowsOnly:
		return "#1232 and the Windows build state"
	case clusterProductGap:
		return "its Windows plan task"
	case clusterFinalize:
		return "`cmdr-local`'s typed rename, then `cmdr-operations`' conversion"
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
