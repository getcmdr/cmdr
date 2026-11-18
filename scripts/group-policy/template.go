package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
)

// The policy definition, as `policies.json` holds it. The app's tests read the same file
// (`managed_policy/definition.rs`).
type definition struct {
	WindowsPolicyKey string      `json:"windowsPolicyKey"`
	GroupPolicy      groupPolicy `json:"groupPolicy"`
	Policies         []policy    `json:"policies"`
}

type groupPolicy struct {
	// Revision is the template's `revision` (major.minor). Raise it when a policy is added or changed.
	Revision        string `json:"revision"`
	VendorCategory  string `json:"vendorCategory"`
	ProductCategory string `json:"productCategory"`
	ProductExplain  string `json:"productExplain"`
	SupportedOn     string `json:"supportedOn"`
}

type policy struct {
	Name        string `json:"name"`
	Kind        string `json:"kind"`
	DisplayName string `json:"displayName"`
	Explain     string `json:"explain"`
	// Label is the text beside the input box; only a `string` or `stringList` policy has one.
	Label string `json:"label"`
}

const (
	kindBool       = "bool"
	kindString     = "string"
	kindStringList = "stringList"
)

// What a policy, category, or string id may be: the ASCII half of the ADMX schema's `itemName`
// (letters, digits, `_`), so the generator can quote names as they are.
var itemName = regexp.MustCompile(`^[A-Za-z0-9_]+$`)

var revision = regexp.MustCompile(`^[0-9]{1,4}\.[0-9]{1,5}$`)

func parseDefinition(raw []byte) (definition, error) {
	var def definition
	decoder := json.NewDecoder(bytes.NewReader(raw))
	if err := decoder.Decode(&def); err != nil {
		return def, err
	}
	return def, def.validate()
}

func (def definition) validate() error {
	var problems []string
	gp := def.GroupPolicy
	if !strings.HasPrefix(strings.ToUpper(def.WindowsPolicyKey), `SOFTWARE\POLICIES\`) {
		problems = append(problems, fmt.Sprintf("windowsPolicyKey %q isn't under SOFTWARE\\Policies", def.WindowsPolicyKey))
	}
	if !revision.MatchString(gp.Revision) {
		problems = append(problems, fmt.Sprintf("groupPolicy.revision %q isn't major.minor", gp.Revision))
	}
	for _, field := range [][2]string{{"vendorCategory", gp.VendorCategory}, {"productCategory", gp.ProductCategory}} {
		if !itemName.MatchString(field[1]) {
			problems = append(problems, fmt.Sprintf("groupPolicy.%s %q isn't a plain name", field[0], field[1]))
		}
	}
	if gp.ProductExplain == "" || gp.SupportedOn == "" {
		problems = append(problems, "groupPolicy needs productExplain and supportedOn")
	}
	if len(def.Policies) == 0 {
		problems = append(problems, "no policies")
	}
	seen := map[string]bool{}
	for _, p := range def.Policies {
		lower := strings.ToLower(p.Name)
		if seen[lower] {
			problems = append(problems, fmt.Sprintf("policy %s is listed twice (registry names ignore case)", p.Name))
		}
		seen[lower] = true
		problems = append(problems, p.problems()...)
	}
	if len(problems) > 0 {
		return fmt.Errorf("%s", strings.Join(problems, "; "))
	}
	return nil
}

// problems lists what keeps one policy out of the template.
func (p policy) problems() []string {
	var out []string
	if !itemName.MatchString(p.Name) {
		out = append(out, fmt.Sprintf("policy name %q isn't a plain name", p.Name))
	}
	if p.DisplayName == "" || p.Explain == "" {
		out = append(out, fmt.Sprintf("policy %s needs a displayName and an explain", p.Name))
	}
	switch p.Kind {
	case kindBool:
		if p.Label != "" {
			out = append(out, fmt.Sprintf("policy %s is a bool, which has no input box to label", p.Name))
		}
	case kindString, kindStringList:
		if p.Label == "" {
			out = append(out, fmt.Sprintf("policy %s needs a label for its input box", p.Name))
		}
	default:
		out = append(out, fmt.Sprintf("policy %s has the unknown kind %q", p.Name, p.Kind))
	}
	return out
}
