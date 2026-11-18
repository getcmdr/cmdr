package main

import (
	"fmt"
	"os"
	"regexp"
	"strings"
)

// ---- Receivers ----

var noMethodOnAppPath = regexp.MustCompile("^no method named `(\\w+)` found for .*`[^`]*\\b(AppPath|AppPathBuf)\\b[^`]*`")

// methodCallAt finds the method call a diagnostic is about: the method's name rustc names
// (the primary span of an E0599, the "arguments to this method are incorrect" span of an
// E0308) and the `.` before it.
func methodCallAt(d *diagnostic, src *source, p *diagSpan) (int, string, bool) {
	nameSpan := (*diagSpan)(nil)
	switch d.code() {
	case "E0599":
		nameSpan = p
	case "E0308":
		if d.Message == "arguments to this method are incorrect" {
			nameSpan = p
		}
		for i := range d.Spans {
			if d.Spans[i].label() == "arguments to this method are incorrect" {
				nameSpan = &d.Spans[i]
			}
		}
	}
	if nameSpan == nil || nameSpan.FileName != p.FileName || nameSpan.ByteEnd > len(src.text) {
		return 0, "", false
	}
	dot := nameSpan.ByteStart - 1
	for dot >= 0 && isSpace(src.masked[dot]) {
		dot--
	}
	if dot < 0 || src.masked[dot] != '.' {
		return 0, "", false
	}
	return dot, string(src.text[nameSpan.ByteStart:nameSpan.ByteEnd]), true
}

// definedInAppPath answers whether the callee rustc points at lives in the app path module.
func definedInAppPath(d *diagnostic) bool {
	for _, child := range d.Children {
		if !strings.HasSuffix(child.Message, "defined here") {
			continue
		}
		for _, s := range child.Spans {
			if strings.Contains(s.FileName, "cmdr-fs/src/app_path/") {
				return true
			}
		}
	}
	return false
}

// receiverEdits makes a typed receiver answer as `Path` does: a `Path` method `AppPath`
// lacks (E0599), or a same-named `AppPath` method fed a `std::path` argument (E0308), drops
// the RECEIVER to `.as_std_legacy()` and leaves the argument native. ❌ Never the reverse:
// the typed twin answers otherwise on bridge values (`flip_shapes_test.rs`).
func receiverEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	dot, method, ok := methodCallAt(d, src, p)
	if !ok {
		return nil
	}
	switch d.code() {
	case "E0599":
		if !noMethodOnAppPath.MatchString(d.Message) {
			return nil
		}
	case "E0308":
		if !definedInAppPath(d) || !sameNamedMethods[method] {
			return nil
		}
	}
	return []edit{{file: p.FileName, start: dot, end: dot, text: ".as_std_legacy()", rule: ruleReceiver}}
}

// receiverStart walks back from a method call's `.` over the receiver chain
// (`self.inner.root()`, `paths[0]`, `a::b(x)?`) to where it starts.
func receiverStart(masked []byte, dot int) int {
	k := dot
	for {
		segStart, j := segmentBefore(masked, k)
		if segStart == k {
			return k
		}
		switch {
		case j >= 0 && masked[j] == '.':
			k = j
		case j >= 1 && masked[j] == ':' && masked[j-1] == ':':
			k = j - 1
		default:
			return segStart
		}
	}
}

// segmentBefore reads one chain segment ending just before k (`name`, `name(args)?`,
// `[i]`): where it starts (k when there's none) and the offset just before it.
func segmentBefore(masked []byte, k int) (int, int) {
	j := k - 1
	for j >= 0 && (isSpace(masked[j]) || masked[j] == '?') {
		j--
	}
	segStart := k
	for j >= 0 && (masked[j] == ')' || masked[j] == ']') {
		open := matchBackward(masked, j)
		if open < 0 {
			return k, j
		}
		segStart = open
		j = open - 1
	}
	for j >= 0 && isIdentByte(masked[j]) {
		segStart = j
		j--
	}
	return segStart, j
}

func matchBackward(masked []byte, closeAt int) int {
	pairs := map[byte]byte{')': '(', ']': '['}
	open := pairs[masked[closeAt]]
	depth := 0
	for k := closeAt; k >= 0; k-- {
		switch masked[k] {
		case masked[closeAt]:
			depth++
		case open:
			depth--
			if depth == 0 {
				return k
			}
		}
	}
	return -1
}

// ---- A comparison between a typed and a std value ----

var (
	cantCompare   = regexp.MustCompile("^can't compare `(.+)` with `(.+)`$")
	binaryOnType  = regexp.MustCompile("^binary operation `(?:==|!=)` cannot be applied to type `(.+)`$")
	assertMacros  = map[string]bool{"assert_eq": true, "assert_ne": true, "debug_assert_eq": true, "debug_assert_ne": true}
	appValueTypes = map[string]string{"AppPath": ".as_std_legacy()", "AppPathBuf": ".as_std_legacy()", "BridgedPath": ".as_path()"}
)

// compareEdits answers `assert_eq!(echo.stripped, Path::new("/photos"))` and `a == b` where
// one side is now typed: the TYPED operand drops to `std::path` (`.as_std_legacy()`, or
// `.as_path()` for a `BridgedPath`), so the comparison is `Path`'s, as today, and the
// expected value is untouched.
func compareEdits(d *diagnostic, src *source, p *diagSpan) []edit {
	left, right, ok := comparedTypes(d)
	if !ok {
		return nil
	}
	var operands [][2]int
	if assertMacros[p.Macro] {
		operands = macroArgs(src.masked, p.ByteStart, p.ByteEnd)
	} else if d.code() == "E0369" {
		for _, s := range d.Spans {
			if !s.IsPrimary && s.FileName == p.FileName && s.label() != "" {
				operands = append(operands, [2]int{s.ByteStart, s.ByteEnd})
			}
		}
	}
	if len(operands) < 2 {
		return nil
	}
	var edits []edit
	for i, typ := range []string{left, right} {
		suffix, typed := appValueTypes[strings.TrimLeft(normalizeType(typ), "&")]
		if !typed {
			continue
		}
		expr := strings.TrimSpace(string(src.text[operands[i][0]:operands[i][1]]))
		start := operands[i][0] + strings.Index(string(src.text[operands[i][0]:operands[i][1]]), expr)
		edits = append(edits, edit{file: p.FileName, start: start, end: start + len(expr), text: postfix(expr) + suffix, rule: ruleCompare})
	}
	return edits
}

func comparedTypes(d *diagnostic) (string, string, bool) {
	if m := cantCompare.FindStringSubmatch(d.Message); m != nil && d.code() == "E0277" {
		return stripModules(m[1]), stripModules(m[2]), true
	}
	if m := binaryOnType.FindStringSubmatch(d.Message); m != nil && d.code() == "E0369" {
		var labels []string
		for _, s := range d.Spans {
			if !s.IsPrimary && s.label() != "" {
				labels = append(labels, s.label())
			}
		}
		if len(labels) == 2 {
			return stripModules(labels[0]), stripModules(labels[1]), true
		}
	}
	return "", "", false
}

var modulePrefix = regexp.MustCompile(`\b(?:app_path|bridge|std::path|path)::`)

func stripModules(t string) string {
	return modulePrefix.ReplaceAllString(t, "")
}

// macroArgs splits a macro call's arguments (`assert_eq!(a, b, "msg")`) at top-level commas,
// as [start, end) offsets.
func macroArgs(masked []byte, from, to int) [][2]int {
	open := -1
	for k := from; k < to; k++ {
		if masked[k] == '(' || masked[k] == '[' || masked[k] == '{' {
			open = k
			break
		}
	}
	if open < 0 {
		return nil
	}
	var args [][2]int
	depth, start := 0, open+1
	for k := open + 1; k < to; k++ {
		switch masked[k] {
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			if depth == 0 {
				return append(args, [2]int{start, k})
			}
			depth--
		case ',':
			if depth == 0 {
				args = append(args, [2]int{start, k})
				start = k + 1
			}
		}
	}
	return args
}

// ---- A typed param handed back to a typed slot ----

// typedParamBackEdits answers a prelude'd body handing its (now `std::path`) param to
// something typed, the delegation test doubles do (`self.inner.get_metadata(path)`): the
// slot gets the ORIGINAL typed param, kept as `P_app` before the prelude shadows it. Going
// through `legacy_unchecked` instead would return a future borrowing a temporary (E0515).
func typedParamBackEdits(src *source, s *diagSpan, expected string) []edit {
	m := src.methodAt(s.ByteStart)
	if m == nil {
		return nil
	}
	name := strings.TrimSpace(string(src.text[s.ByteStart:s.ByteEnd]))
	for _, prm := range m.params {
		kind := typedParamKind(prm.typ)
		if prm.name != name || kind == "" || !hasPrelude(src, m, name) {
			continue
		}
		if (kind == "one" && expected != "&AppPath") || (kind == "batch" && expected != "&[AppPathBuf]") {
			return nil
		}
		edits := []edit{{file: src.path, start: s.ByteStart, end: s.ByteEnd, text: name + "_app", rule: ruleTypedParam}}
		// The prelude this use sees: the last one before it (each async block can hold one).
		before := string(src.text[m.bodyStart:s.ByteStart])
		at := strings.LastIndex(before, "let "+name+"_std")
		if at < 0 {
			return nil
		}
		keep := "let " + name + "_app = " + name + ";"
		if !strings.HasSuffix(strings.TrimRight(before[:at], " \t\n"), keep) {
			edits = append(edits, edit{file: src.path, start: m.bodyStart + at, end: m.bodyStart + at, text: keep + "\n", rule: ruleTypedParam})
		}
		return edits
	}
	return nil
}

// ---- E0308 by type pair ----

var stdLiteral = regexp.MustCompile(`^&*\s*(?:std::path::)?(?:Path::new|PathBuf::from)\(\s*"([^"\\]*)"\s*\)(?:\.as_path\(\))?$`)

// mismatchEdits is the type-pair table: each row a type-preserving rewrite through the
// bridge, so no value changes in transit (the bridge's byte-identity property).
func mismatchEdits(src *source, p *diagSpan, expected, found string) []edit {
	expr := string(src.text[p.ByteStart:p.ByteEnd])
	text, rule := rewriteValue(expr, expected, found)
	if text == "" {
		return nil
	}
	if name, ok := shorthandField(src, p, expr); ok {
		text = name + ": " + text
	}
	return []edit{{file: p.FileName, start: p.ByteStart, end: p.ByteEnd, text: text, rule: rule}}
}

// valueRule is one row of the type-pair table: a slot expecting one type got the other, and
// the rewrite carries the value across with its bytes unchanged.
type valueRule struct {
	expected string
	found    func(string) bool
	rewrite  func(expr, found string) string
	rule     string
}

func is(t string) func(string) bool { return func(found string) bool { return found == t } }

func isBridged(found string) bool { return strings.TrimLeft(found, "&") == "BridgedPath" }

func isAppPathOrRef(found string) bool { return found == "AppPathBuf" || isAppPathRef(found) }

const (
	batchBridge = ".iter().map(|p| AppPath::legacy_unchecked(p).into_owned()).collect::<Vec<_>>()"
	vecBridge   = ".into_iter().map(|p| AppPath::legacy_unchecked(&p).into_owned()).collect::<Vec<_>>()"
)

var valueRules = []valueRule{
	{"&AppPath", isStdPathRef, bridgeArg, ruleBridgeArg},
	{"&AppPath", isBridged, func(e, _ string) string { return postfix(strings.TrimLeft(e, "&")) + ".app()" }, ruleBridgedApp},
	{"&[AppPathBuf]", isStdPathBatch, func(e, _ string) string { return "&" + postfix(strings.TrimLeft(e, "&")) + batchBridge }, ruleBridgeBatch},
	{"AppPathBuf", is("PathBuf"), func(e, _ string) string {
		return "AppPath::legacy_unchecked(&" + postfixBorrowable(e) + ").into_owned()"
	}, ruleBridgeOwned},
	{"AppPathBuf", isStdPathRef, func(e, f string) string {
		return "AppPath::legacy_unchecked(" + dropExtraBorrows(e, f) + ").into_owned()"
	}, ruleBridgeOwned},
	{"Option<AppPathBuf>", is("Option<PathBuf>"), func(e, _ string) string { return postfix(e) + ".map(|p| AppPath::legacy_unchecked(&p).into_owned())" }, ruleBridgeOwned},
	{"Vec<AppPathBuf>", is("Vec<PathBuf>"), func(e, _ string) string { return postfix(e) + vecBridge }, ruleBridgeOwned},
	{"&Path", isAppPathRef, func(e, _ string) string { return "&" + postfix(strings.TrimLeft(e, "&")) + ".as_std_legacy()" }, ruleToStd},
	{"PathBuf", isAppPathOrRef, func(e, _ string) string { return postfix(e) + ".as_std_legacy().into_owned()" }, ruleToStd},
	{"PathBuf", is("BridgedPath"), func(e, _ string) string { return postfix(e) + ".into_path_buf()" }, ruleBridgedNative},
	{"BridgedPath", is("PathBuf"), func(e, _ string) string { return "BridgedPath::new(" + e + ")" }, ruleBridgedWrap},
	{"BridgedPath", isStdPathRef, func(e, _ string) string { return "BridgedPath::new(" + postfix(e) + ".to_path_buf())" }, ruleBridgedWrap},
	{"Option<BridgedPath>", is("Option<PathBuf>"), func(e, _ string) string { return postfix(e) + ".map(BridgedPath::new)" }, ruleBridgedWrap},
	{"Option<&AppPath>", is("Option<&BridgedPath>"), func(e, _ string) string { return postfix(e) + ".map(BridgedPath::app)" }, ruleBridgedApp},
}

// bridgeArg is a std path handed to a typed slot: a canonical literal becomes `app_path!`,
// anything else crosses through `legacy_unchecked`.
func bridgeArg(expr, found string) string {
	if m := stdLiteral.FindStringSubmatch(strings.TrimSpace(expr)); m != nil && isCanonicalLiteral(m[1]) {
		return fmt.Sprintf("app_path!(%q)", m[1])
	}
	return "&AppPath::legacy_unchecked(" + dropExtraBorrows(expr, found) + ")"
}

func rewriteValue(expr, expected, found string) (string, string) {
	for _, row := range valueRules {
		if row.expected != expected || !row.found(found) {
			continue
		}
		text := row.rewrite(expr, found)
		if strings.HasPrefix(text, "app_path!(") {
			return text, ruleLiteral
		}
		return text, row.rule
	}
	return "", ""
}

// dropExtraBorrows keeps one level of reference: `&p` where p is already `&Path` (found
// `&&Path`) becomes `p`, or clippy's `needless_borrow` reddens the rewrite.
func dropExtraBorrows(expr, found string) string {
	extra := len(found) - len(strings.TrimLeft(found, "&")) - 1
	trimmed := strings.TrimSpace(expr)
	for ; extra > 0 && strings.HasPrefix(trimmed, "&") && !strings.HasPrefix(trimmed, "&&"); extra-- {
		trimmed = strings.TrimSpace(trimmed[1:])
	}
	return trimmed
}

// postfixBorrowable is expr ready for a leading `&`.
func postfixBorrowable(expr string) string {
	if strings.HasPrefix(strings.TrimSpace(expr), "&") {
		return "(" + expr + ")"
	}
	return postfix(expr)
}

// shorthandField answers whether expr is a struct literal's shorthand field (`Foo { root }`),
// which a rewrite must spell out as `root: <rewrite>`.
func shorthandField(src *source, p *diagSpan, expr string) (string, bool) {
	if !isIdent(expr) {
		return "", false
	}
	before := p.ByteStart - 1
	for before >= 0 && isSpace(src.masked[before]) {
		before--
	}
	after := p.ByteEnd
	for after < len(src.masked) && isSpace(src.masked[after]) {
		after++
	}
	if before < 0 || after >= len(src.masked) || !strings.ContainsRune("{,", rune(src.masked[before])) || !strings.ContainsRune(",}", rune(src.masked[after])) {
		return "", false
	}
	open := enclosingOpen(src.masked, p.ByteStart)
	if open < 0 || src.masked[open] != '{' {
		return "", false
	}
	head := strings.TrimRight(string(src.masked[max(0, open-80):open]), " \n\t")
	if !regexp.MustCompile(`(?:^|[^\w])(?:[A-Z]\w*|Self)(?:::\w+)*(?:<[^{}]*>)?$`).MatchString(head) {
		return "", false
	}
	return expr, true
}

// enclosingOpen is the offset of the innermost unclosed `(`, `[`, or `{` before offset.
func enclosingOpen(masked []byte, offset int) int {
	depth := 0
	for k := offset - 1; k >= 0; k-- {
		switch masked[k] {
		case ')', ']', '}':
			depth++
		case '(', '[', '{':
			if depth == 0 {
				return k
			}
			depth--
		}
	}
	return -1
}

// ---- A borrowed return: `root`, `stands_in_for_own_disk`, `local_backing_file` ----

var (
	borrowedReturnMethods = map[string]bool{"root": true, "stands_in_for_own_disk": true, "local_backing_file": true}
	selfFieldRef          = regexp.MustCompile(`(?:&\*?)?self\.(\w+)(?:\.as_path\(\)|\.as_ref\(\)|\.as_deref\(\)|\.deref\(\))?`)
	literalRoot           = regexp.MustCompile(`^&?\s*(?:std::path::)?Path::new\(\s*("[^"\\]*")\s*\)$`)
)

// borrowedReturnEdits answers `&AppPath::legacy_unchecked(&self.root)` being a temporary
// (E0515): the FIELD the method returns becomes a `BridgedPath` (both spellings, made
// once), and the method returns `.app()` of it. A literal root becomes a `'static` app path.
func (r *rewriter) borrowedReturnEdits(src *source, p *diagSpan, expected string) []edit {
	m := src.methodAt(p.ByteStart)
	if m == nil || !borrowedReturnMethods[m.name] || (expected != "&AppPath" && expected != "Option<&AppPath>") {
		return nil
	}
	expr := string(src.text[p.ByteStart:p.ByteEnd])
	if lit := literalRoot.FindStringSubmatch(strings.TrimSpace(expr)); lit != nil && expected == "&AppPath" {
		text := "AppPath::from_normalized(" + lit[1] + ").expect(\"a canonical literal\")"
		return []edit{{file: p.FileName, start: p.ByteStart, end: p.ByteEnd, text: text, rule: ruleBorrowedRet}}
	}
	loc := fieldRefIn(expr)
	if loc == nil {
		return nil
	}
	field := expr[loc[2]:loc[3]]
	optional, retype, err := r.retypeField(m.selfType, field)
	if err != nil || (retype == nil && optional == fieldUnknown) {
		return nil
	}
	access := "self." + field + ".app()"
	if optional == fieldOptional {
		access = "self." + field + ".as_ref().map(BridgedPath::app)"
	}
	edits := []edit{{file: p.FileName, start: p.ByteStart + loc[0], end: p.ByteStart + loc[1], text: access, rule: ruleBorrowedRet}}
	if retype != nil {
		edits = append(edits, *retype)
	}
	return edits
}

// fieldRefIn finds the first `self.FIELD` in expr that is a field, ❗ not a method call of
// the same name (`self.own_disk_home()`, an accessor beside its field).
func fieldRefIn(expr string) []int {
	for _, loc := range selfFieldRef.FindAllStringSubmatchIndex(expr, -1) {
		if loc[1] == loc[3] && loc[1] < len(expr) && (expr[loc[1]] == '(' || expr[loc[1]] == ':') {
			continue
		}
		return loc
	}
	return nil
}

type fieldShape int

const (
	fieldUnknown fieldShape = iota
	fieldPlain
	fieldOptional
)

var fieldDecl = regexp.MustCompile(`(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*(Option<\s*)?((?:std::path::)?(?:PathBuf|BridgedPath))\s*(>)?\s*,`)

// retypeField finds `field` in the declaration of `selfType` among the crate's files and
// returns its shape plus the edit retyping it from `PathBuf` to `BridgedPath` (nil when it
// already is one).
func (r *rewriter) retypeField(selfType, field string) (fieldShape, *edit, error) {
	base := selfType
	if i := strings.IndexAny(base, "<"); i >= 0 {
		base = base[:i]
	}
	if i := strings.LastIndex(base, "::"); i >= 0 {
		base = base[i+2:]
	}
	decl := regexp.MustCompile(`\bstruct\s+` + regexp.QuoteMeta(base) + `\b[^{;]*\{`)
	for _, file := range r.crateRust {
		src, err := r.load(file)
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}
			return fieldUnknown, nil, err
		}
		loc := decl.FindIndex(src.masked)
		if loc == nil {
			continue
		}
		open := loc[1] - 1
		close := matchBrace(src.masked, open)
		for _, f := range fieldDecl.FindAllSubmatchIndex(src.masked[open:close+1], -1) {
			if string(src.masked[open+f[2]:open+f[3]]) != field {
				continue
			}
			shape := fieldPlain
			if f[4] >= 0 {
				shape = fieldOptional
			}
			typeStart, typeEnd := open+f[6], open+f[7]
			if strings.HasSuffix(string(src.text[typeStart:typeEnd]), "BridgedPath") {
				return shape, nil, nil
			}
			return shape, &edit{file: file, start: typeStart, end: typeEnd, text: "BridgedPath", rule: ruleBorrowedRet}, nil
		}
	}
	return fieldUnknown, nil, nil
}
