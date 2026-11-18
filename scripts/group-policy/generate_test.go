package main

import (
	"encoding/xml"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const fixture = `{
  "windowsPolicyKey": "SOFTWARE\\Policies\\TestVendor\\TestApp",
  "groupPolicy": {
    "revision": "1.0",
    "vendorCategory": "TestVendor",
    "namespaceVendor": "TestVendorLongName",
    "productCategory": "TestApp",
    "productExplain": "Policies for the test app.",
    "supportedOn": "The test app"
  },
  "policies": [
    { "name": "DisableThing", "kind": "bool", "displayName": "Turn off the thing", "explain": "No thing & no <other>.\n\nSecond paragraph." },
    { "name": "MaxThing", "kind": "string", "displayName": "Highest thing", "label": "Highest:", "explain": "A ceiling." },
    { "name": "AllowedThings", "kind": "stringList", "displayName": "Allowed things", "label": "Things:", "explain": "A list." }
  ]
}`

func fixtureFiles(t *testing.T) map[string]string {
	t.Helper()
	def, err := parseDefinition([]byte(fixture))
	if err != nil {
		t.Fatalf("the fixture is valid: %v", err)
	}
	files, err := generate(def)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	out := map[string]string{}
	for _, f := range files {
		out[f.path] = string(f.content)
	}
	return out
}

func TestGenerateWritesTheVendorAndProductFilesInThePolicyDefinitionsLayout(t *testing.T) {
	files := fixtureFiles(t)
	for _, path := range []string{"testvendor.admx", "en-US/testvendor.adml", "testapp.admx", "en-US/testapp.adml"} {
		content, ok := files[path]
		if !ok {
			t.Fatalf("no %s among %v", path, keys(files))
		}
		decoder := xml.NewDecoder(strings.NewReader(content))
		for {
			if _, err := decoder.Token(); err == io.EOF {
				break
			} else if err != nil {
				t.Fatalf("%s isn't well-formed XML: %v", path, err)
			}
		}
	}
	if len(files) != 4 {
		t.Fatalf("want 4 files, got %v", keys(files))
	}
}

func TestEachKindWritesWhatTheAppReads(t *testing.T) {
	admx := fixtureFiles(t)["testapp.admx"]
	wants := []string{
		// The product hangs under the vendor's shared category, in its own namespace.
		`<target prefix="testapp" namespace="TestVendorLongName.Policies.TestApp" />`,
		`<using prefix="testvendor" namespace="TestVendorLongName.Policies" />`,
		`<parentCategory ref="testvendor:TestVendor" />`,
		`<supportedOn ref="SUPPORTED_TestApp_Windows" />`,
		// A bool: a REG_DWORD named after the policy, 1 or 0.
		`<policy name="DisableThing" class="Both" displayName="$(string.DisableThing)" explainText="$(string.DisableThing_Explain)" key="SOFTWARE\Policies\TestVendor\TestApp" valueName="DisableThing">`,
		"<enabledValue>\n        <decimal value=\"1\" />",
		"<disabledValue>\n        <decimal value=\"0\" />",
		// A string: a REG_SZ under the policy key.
		`<text id="MaxThing" valueName="MaxThing" required="true" />`,
		// A list: values 1, 2, … in a subkey named after the policy.
		`<list id="AllowedThings" key="SOFTWARE\Policies\TestVendor\TestApp\AllowedThings" valuePrefix="" />`,
	}
	for _, want := range wants {
		if !strings.Contains(admx, want) {
			t.Errorf("the ADMX lacks %s", want)
		}
	}
	if strings.Contains(admx, `valueName="AllowedThings"`) {
		t.Error("a list policy must not also write a plain value: the app would read it instead of the list")
	}
}

func TestTheADMLEscapesTextAndKeepsParagraphs(t *testing.T) {
	adml := fixtureFiles(t)["en-US/testapp.adml"]
	want := "<string id=\"DisableThing_Explain\">No thing &amp; no &lt;other&gt;.\n\nSecond paragraph.\n\nRegistry: SOFTWARE\\Policies\\TestVendor\\TestApp, value DisableThing (REG_DWORD), 1 when enabled and 0 when disabled.</string>"
	if !strings.Contains(adml, want) {
		t.Errorf("the explain text should read\n%s\nin\n%s", want, adml)
	}
	for _, want := range []string{`<textBox refId="MaxThing">`, `<listBox refId="AllowedThings">Things:</listBox>`} {
		if !strings.Contains(adml, want) {
			t.Errorf("the ADML lacks %s", want)
		}
	}
}

func TestValidateRefusesADefinitionTheTemplateCantCarry(t *testing.T) {
	cases := map[string]string{
		"an unknown kind":                 `"kind": "bool", "displayName": "Turn off the thing"` + "|" + `"kind": "number", "displayName": "Turn off the thing"`,
		"a list without a label":          `"label": "Things:", ` + "|" + ``,
		"a label on a bool":               `"kind": "bool", "displayName"` + "|" + `"kind": "bool", "label": "x", "displayName"`,
		"a name twice, ignoring case":     `"name": "MaxThing"` + "|" + `"name": "disablething"`,
		"a key outside Policies":          `SOFTWARE\\Policies\\TestVendor` + "|" + `SOFTWARE\\TestVendor`,
		"a name the schema doesn't allow": `"name": "MaxThing"` + "|" + `"name": "Max Thing"`,
		"no namespace vendor":             `"namespaceVendor": "TestVendorLongName",` + "|" + ``,
		"a namespace vendor with a dot":   `"TestVendorLongName"` + "|" + `"Test.Vendor"`,
	}
	for what, edit := range cases {
		from, to, _ := strings.Cut(edit, "|")
		broken := strings.Replace(fixture, from, to, 1)
		if broken == fixture {
			t.Fatalf("%s: the edit didn't apply", what)
		}
		if _, err := parseDefinition([]byte(broken)); err == nil {
			t.Errorf("%s should be refused", what)
		}
	}
}

func TestCheckReferencesCatchesAMissingOrUnusedString(t *testing.T) {
	files := fixtureFiles(t)
	admx := file{"testapp.admx", []byte(files["testapp.admx"])}
	missing := strings.Replace(files["en-US/testapp.adml"], `<string id="MaxThing">`, `<string id="Renamed">`, 1)
	err := checkReferences(admx, file{"en-US/testapp.adml", []byte(missing)})
	if err == nil || !strings.Contains(err.Error(), "$(string.MaxThing)") || !strings.Contains(err.Error(), "Renamed") {
		t.Fatalf("want both the dangling reference and the unused string named, got %v", err)
	}
}

func TestRunWritesThenReportsOnlyWhatDrifted(t *testing.T) {
	root := t.TempDir()
	defPath := filepath.Join(root, definitionPath)
	if err := os.MkdirAll(filepath.Dir(defPath), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(defPath, []byte(fixture), 0o644); err != nil {
		t.Fatal(err)
	}
	written, err := run(root, false)
	if err != nil || len(written) != 4 {
		t.Fatalf("a first run writes all four files: %v, %v", written, err)
	}
	if stale, err := run(root, true); err != nil || len(stale) != 0 {
		t.Fatalf("a fresh template is in sync: %v, %v", stale, err)
	}
	edited := filepath.Join(root, outputDir, "testapp.admx")
	if err := os.WriteFile(edited, []byte("hand-edited"), 0o644); err != nil {
		t.Fatal(err)
	}
	stale, err := run(root, true)
	if err != nil || len(stale) != 1 || stale[0] != outputDir+"/testapp.admx" {
		t.Fatalf("a hand edit is stale: %v, %v", stale, err)
	}
	if content, _ := os.ReadFile(edited); string(content) != "hand-edited" {
		t.Fatal("-check must not write")
	}
}

func keys(m map[string]string) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}
