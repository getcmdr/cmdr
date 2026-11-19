package main

import (
	"strings"
	"testing"
)

// The rules the app (`cmdr`, tier 4) added: its `Volume::root()` reads in std code, and its
// many same-named test doubles.

func compareDiag(t *testing.T, text, op, after, left, right string) diagnostic {
	t.Helper()
	d := diag("E0277", "can't compare `"+left+"` with `"+right+"`", spanOf(t, text, op, after, "no implementation for `"+left+" == "+right+"`", true))
	return d
}

func withNote(d diagnostic, note string) diagnostic {
	d.Children = append(d.Children, diagnostic{Message: note})
	return d
}

func TestAPlainComparisonDropsItsTypedOperandToStd(t *testing.T) {
	cases := []struct{ text, after, left, right, note, want string }{
		{
			"fn t() { (volume.root() == path).then_some(id) }\n", "root()", "cmdr_fs::app_path::AppPath", "std::path::Path",
			"required for `&AppPath` to implement `PartialEq<&std::path::Path>`",
			"(&*volume.root().as_std_legacy() == path).then_some(id)",
		},
		{
			"fn t() { self.roots.iter().find(|r| r.path == active).is_none_or(f) }\n", "r.path", "std::path::PathBuf", "&AppPath", "",
			"find(|r| r.path == *active.as_std_legacy())",
		},
		{
			"fn t() { x.find(|(zip, _)| ok(zip) && zip.as_path() != view.root())?; }\n", "as_path()", "std::path::Path", "AppPath",
			"required for `&std::path::Path` to implement `PartialEq<&AppPath>`",
			"&& zip.as_path() != &*view.root().as_std_legacy())?;",
		},
		{
			"fn t() {\n    if let Some(v) = f()\n        && v.root() == std::path::Path::new(p)\n    {\n    }\n}\n", "root()", "AppPath", "std::path::Path",
			"required for `&AppPath` to implement `PartialEq<&std::path::Path>`",
			"&& &*v.root().as_std_legacy() == std::path::Path::new(p)",
		},
	}
	for _, c := range cases {
		op := "=="
		if strings.Contains(c.text, "!=") {
			op = "!="
		}
		d := compareDiag(t, c.text, op, c.after, c.left, c.right)
		if c.note != "" {
			d = withNote(d, c.note)
		}
		if got := run(t, c.text, d); !strings.Contains(got, c.want) {
			t.Errorf("want %q in:\n%s", c.want, got)
		}
	}
}

func TestAComparisonWhoseOperandIsntAWholeChainStaysResidue(t *testing.T) {
	for _, text := range []string{
		"fn t() { a | v.root() == p }\n",        // bit-or binds tighter than `==`
		"fn t() { *v.root() == p }\n",           // a unary operator on the operand
		"fn t() { v.root() == p as &Path }\n",   // a cast binds tighter
		"fn t() { v.root() == &p.as_path() }\n", // a borrowed right operand
	} {
		d := withNote(compareDiag(t, text, "==", "", "AppPath", "std::path::Path"), "required for `&AppPath` to implement `PartialEq<&std::path::Path>`")
		if got := run(t, text, d); got != text {
			t.Errorf("must stay residue:\n%s", got)
		}
	}
}

func TestATypedValueIntoAGenericStdBoundDropsToStd(t *testing.T) {
	cases := []struct{ text, needle, message, want string }{
		{"fn t() { row.starts_with(volume.root()) }\n", "volume.root()", "the trait bound `AppPath: AsRef<std::path::Path>` is not satisfied", "row.starts_with(&*volume.root().as_std_legacy())"},
		{"fn t() { LogPrivate::dir_path(self.mount.root()) }\n", "self.mount.root()", "the trait bound `AppPath: AsRef<OsStr>` is not satisfied", "LogPrivate::dir_path(&*self.mount.root().as_std_legacy())"},
		{"fn t() { MountRoot::new(volume.root()) }\n", "volume.root()", "the trait bound `&AppPath: Into<PathBuf>` is not satisfied", "MountRoot::new(&*volume.root().as_std_legacy())"},
	}
	for _, c := range cases {
		body := c.text[strings.Index(c.text, "{ ")+2:]
		call := body[:strings.Index(body, "(")]
		d := diag("E0277", c.message,
			spanOf(t, c.text, c.needle, "", "the trait is not implemented", true),
			spanOf(t, c.text, call, "", "required by a bound introduced by this call", false))
		if got := run(t, c.text, d); !strings.Contains(got, c.want) {
			t.Errorf("want %q in:\n%s", c.want, got)
		}
	}
	text := "fn t() { row.starts_with(volume.root()) }\n"
	d := diag("E0277", "the trait bound `AppPath: AsRef<std::path::Path>` is not satisfied", spanOf(t, text, "volume.root()", "", "", true))
	if got := run(t, text, d); got != text {
		t.Fatalf("without the call's bound label the span may be no argument:\n%s", got)
	}
}

func TestABorrowedReturnRetypesItsOwnFilesStruct(t *testing.T) {
	other := "crates/demo/src/other.rs"
	otherText := "struct Demo {\n    root: PathBuf,\n}\n"
	text := "struct Demo {\n    root: PathBuf,\n}\n" + implHead + "    fn root(&self) -> &AppPath {\n        &self.root\n    }\n}\n"
	d := diag("E0308", "mismatched types", spanOf(t, text, "&self.root", "fn root", "expected `&AppPath`, found `&PathBuf`", true))
	r := fixture(map[string]string{other: otherText, fixtureFile: text})
	r.crateRust = []string{other, fixtureFile}
	edits, err := r.editsFor(&d)
	if err != nil {
		t.Fatal(err)
	}
	for _, e := range edits {
		if e.file != fixtureFile {
			t.Fatalf("retyped another file's same-named struct: %+v", e)
		}
	}
	if got := string(applyToText([]byte(text), chooseEdits(edits))); !strings.Contains(got, "root: BridgedPath,") {
		t.Fatalf("own struct not retyped:\n%s", got)
	}

	// Declared only elsewhere, twice: ambiguous, so no field is retyped (the value rows bridge
	// the return, and the E0515 that follows is residue).
	impl := implHead + "    fn root(&self) -> &AppPath {\n        &self.root\n    }\n}\n"
	d = diag("E0308", "mismatched types", spanOf(t, impl, "&self.root", "fn root", "expected `&AppPath`, found `&PathBuf`", true))
	third := "crates/demo/src/third.rs"
	r = fixture(map[string]string{other: otherText, third: otherText, fixtureFile: impl})
	r.crateRust = []string{other, third, fixtureFile}
	if edits, err = r.editsFor(&d); err != nil {
		t.Fatal(err)
	}
	for _, e := range edits {
		if e.file != fixtureFile || e.rule == ruleBorrowedRet {
			t.Fatalf("an ambiguous struct must not be retyped: %+v", e)
		}
	}
}

func TestAFlippedBodysStdReadOfItsParamGetsThePrelude(t *testing.T) {
	text := implHead + "    fn get_metadata<'a>(&'a self, path: &'a AppPath) -> Fut<'a> {\n        Box::pin(async move {\n            if path != self.file {\n                return Err(e);\n            }\n            ok(self.root.starts_with(path))\n        })\n    }\n}\n"
	d := withNote(compareDiag(t, text, "!=", "if path ", "AppPath", "std::path::PathBuf"), "required for `&AppPath` to implement `PartialEq<std::path::PathBuf>`")
	got := run(t, text, d)
	if strings.Contains(got, "as_std_legacy() !=") || !strings.Contains(got, "let path: &Path = &path_std;") {
		t.Fatalf("a stale comparison of the param must get the prelude:\n%s", got)
	}
	d = diag("E0277", "the trait bound `AppPath: AsRef<std::path::Path>` is not satisfied",
		spanOf(t, text, "path", "starts_with(", "", true), spanOf(t, text, "self.root.starts_with", "", "required by a bound introduced by this call", false))
	if got := run(t, text, d); strings.Contains(got, "&*path") || !strings.Contains(got, "let path_std") {
		t.Fatalf("a bound error on the param must get the prelude:\n%s", got)
	}
}
