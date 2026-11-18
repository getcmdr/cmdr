package main

import (
	"regexp"
	"strings"
)

// The rule names, as the report counts them.
const (
	ruleSignature     = "E0053 signature"
	rulePrelude       = "body prelude"
	ruleImport        = "import"
	ruleLiteral       = "E0308 literal to app_path!"
	ruleBridgeArg     = "E0308 arg to legacy_unchecked"
	ruleBridgeBatch   = "E0308 batch to legacy_unchecked"
	ruleBridgeOwned   = "E0308 owned to legacy_unchecked"
	ruleToStd         = "E0308 typed value to as_std_legacy"
	ruleReceiver      = "receiver to as_std_legacy"
	ruleBorrowedRet   = "borrowed return to BridgedPath"
	ruleBridgedWrap   = "E0308 field to BridgedPath::new"
	ruleBridgedApp    = "E0308 BridgedPath to app()"
	ruleBridgedNative = "E0308 BridgedPath to native"
	ruleUnusedImport  = "unused import"
	ruleTypedParam    = "typed param handed back"
	ruleCompare       = "comparison operand to std"
	ruleAsyncHelper   = "async helper call moved into its future"
	ruleRoundTrip     = "bridge round trip collapsed"
	ruleHoist         = "bridge temporary hoisted to a let"
)

var (
	mismatchLabel   = regexp.MustCompile("^expected `(.+?)`, found `(.+)`$")
	lifetimePattern = regexp.MustCompile(`'\w+\s*`)
	// A lifetime as a generic argument (`Cow<'_, Path>`) goes with its comma.
	lifetimeArgPattern = regexp.MustCompile(`'\w+\s*,\s*`)
	qualifiers         = []string{
		"std::path::", "cmdr_fs::app_path::", "crate::app_path::", "app_path::", "std::option::Option", "std::vec::Vec",
		"std::borrow::", "alloc::vec::Vec", "core::option::Option",
	}
)

// normalizeType strips lifetimes, crate qualifiers, and spaces from a rustc type, so
// `&'a std::path::Path` reads `&Path` and `std::option::Option<&'a Path>` reads `Option<&Path>`.
func normalizeType(t string) string {
	t = lifetimeArgPattern.ReplaceAllString(t, "")
	t = lifetimePattern.ReplaceAllString(t, "")
	for _, q := range qualifiers {
		switch {
		case strings.HasSuffix(q, "Option"):
			t = strings.ReplaceAll(t, q, "Option")
		case strings.HasSuffix(q, "Vec"):
			t = strings.ReplaceAll(t, q, "Vec")
		default:
			t = strings.ReplaceAll(t, q, "")
		}
	}
	return strings.ReplaceAll(t, " ", "")
}

// mismatch is an E0308 label's two types, normalized.
func mismatch(label string) (expected, found string, ok bool) {
	m := mismatchLabel.FindStringSubmatch(label)
	if m == nil {
		return "", "", false
	}
	return normalizeType(m[1]), normalizeType(m[2]), true
}

// The std-path reference types a typed slot can receive through the bridge. `TestDir` and
// `MetadataFixture` (`cmdr_fs::testing`) deref to `Path`, so `&dir` coerced into a `&Path`
// slot and coerces into `legacy_unchecked`'s the same way (the workspace's only other
// `Deref<Target = Path>` is `BridgedPath`, which has rows of its own).
func isStdPathRef(t string) bool {
	inner := strings.TrimLeft(t, "&")
	if i := strings.LastIndex(inner, "::"); i >= 0 && !strings.Contains(inner, "<") {
		inner = inner[i+2:]
	}
	switch inner {
	case "Path", "PathBuf", "Cow<Path>", "Cow<'_,Path>", "Box<Path>", "Arc<Path>", "TestDir", "MetadataFixture":
		return strings.HasPrefix(t, "&")
	}
	return false
}

func isAppPathRef(t string) bool {
	switch strings.TrimLeft(t, "&") {
	case "AppPath", "AppPathBuf", "Cow<AppPath>":
		return strings.HasPrefix(t, "&")
	}
	return false
}

func isStdPathBatch(t string) bool {
	inner := strings.TrimLeft(t, "&")
	if !strings.HasPrefix(t, "&") {
		return false
	}
	return inner == "[PathBuf]" || inner == "Vec<PathBuf>" || regexp.MustCompile(`^\[PathBuf;\d+\]$`).MatchString(inner)
}

// sameNamedMethods are the AppPath methods `Path` also has: a typed receiver calling one
// of them drops to `as_std_legacy()`, so `Path` answers as today.
var sameNamedMethods = map[string]bool{
	"starts_with": true, "strip_prefix": true, "relative_to": true, "join": true, "parent": true, "file_name": true,
	"extension": true, "file_stem": true, "with_file_name": true, "ends_with": true, "components": true, "rebase": true,
	"is_on_volume": true, "same_root": true, "names": true, "with_extension": true,
}

// isCanonicalLiteral answers whether `app_path!(text)` accepts text on every host, judged
// conservatively: a POSIX path with plain names, or a scheme path with an authority and at
// least one plain name. Anything else (`"/a/"`, `"/a/../b"`, `""`, `"."`, `"photos"`, a drive
// or UNC literal) stays on the bridge, as the plan's landmine says.
func isCanonicalLiteral(text string) bool {
	if text == "/" {
		return true
	}
	if strings.HasPrefix(text, "/") && !strings.HasPrefix(text, "//") {
		return plainNames(strings.Split(text[1:], "/"))
	}
	scheme, rest, ok := strings.Cut(text, "://")
	if !ok || len(scheme) < 2 || !regexp.MustCompile(`^[a-z][a-z0-9+.-]*$`).MatchString(scheme) {
		return false
	}
	authority, tail, ok := strings.Cut(rest, "/")
	if !ok || authority == "" || tail == "" {
		return false
	}
	return plainNames(strings.Split(tail, "/"))
}

func plainNames(names []string) bool {
	for _, name := range names {
		if name == "" || name == "." || name == ".." || strings.ContainsAny(name, "\\:\x00") {
			return false
		}
		for _, r := range name {
			if r >= 0x100000 || r < 0x20 {
				return false
			}
		}
	}
	return true
}

// needsParens answers whether appending `.method()` to expr would bind to less than all of
// it: a leading `&`, `*`, `!`, or `-`, or an operator, `as`, or space at the top level.
func needsParens(expr string) bool {
	expr = strings.TrimSpace(expr)
	if expr == "" || strings.ContainsAny(expr[:1], "&*!-|") {
		return true
	}
	depth := 0
	for i := 0; i < len(expr); i++ {
		c := expr[i]
		switch c {
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
		case '<':
			if i >= 2 && expr[i-2:i] == "::" {
				depth++
			} else if depth == 0 {
				return true
			}
		case '>':
			if depth > 0 {
				depth--
			} else {
				return true
			}
		default:
			if depth == 0 && strings.IndexByte(" +-*/%&|^!=\n", c) >= 0 {
				return true
			}
		}
	}
	return false
}

// postfix makes expr safe to call a method on.
func postfix(expr string) string {
	if needsParens(expr) {
		return "(" + expr + ")"
	}
	return expr
}
