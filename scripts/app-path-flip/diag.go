package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
)

// diagnostic is the part of a rustc JSON diagnostic the rules read.
type diagnostic struct {
	Message  string       `json:"message"`
	Code     *diagCode    `json:"code"`
	Level    string       `json:"level"`
	Spans    []diagSpan   `json:"spans"`
	Children []diagnostic `json:"children"`
	Rendered string       `json:"rendered"`
}

type diagCode struct {
	Code string `json:"code"`
}

type diagSpan struct {
	FileName                string         `json:"file_name"`
	ByteStart               int            `json:"byte_start"`
	ByteEnd                 int            `json:"byte_end"`
	LineStart               int            `json:"line_start"`
	IsPrimary               bool           `json:"is_primary"`
	Label                   *string        `json:"label"`
	SuggestedReplacement    *string        `json:"suggested_replacement"`
	SuggestionApplicability *string        `json:"suggestion_applicability"`
	Expansion               *diagExpansion `json:"expansion"`
	// Macro is set when this span was lifted to a macro call site in the crate (`assert_eq`):
	// rustc pointed inside the macro's own definition.
	Macro string `json:"-"`
}

type cargoMessage struct {
	Reason  string     `json:"reason"`
	Message diagnostic `json:"message"`
}

func (d *diagnostic) code() string {
	if d.Code == nil {
		return ""
	}
	return d.Code.Code
}

// primary is the diagnostic's first primary span, or an empty span (no file) when it has none.
func (d *diagnostic) primary() *diagSpan {
	for i := range d.Spans {
		if d.Spans[i].IsPrimary {
			return &d.Spans[i]
		}
	}
	return &diagSpan{}
}

func (s *diagSpan) label() string {
	if s.Label == nil {
		return ""
	}
	return *s.Label
}

// parseDiagnostics reads cargo's `--message-format=json` stream and keeps the compiler
// messages whose primary span lies in one of the files `keep` accepts, deduplicated (the lib
// and its test target report a lib file's error twice) and in a stable order.
func parseDiagnostics(stream []byte, keep func(file string) bool) ([]diagnostic, error) {
	seen := map[string]bool{}
	var out []diagnostic
	scanner := bufio.NewScanner(bytes.NewReader(stream))
	scanner.Buffer(make([]byte, 1024*1024), 64*1024*1024)
	for scanner.Scan() {
		line := scanner.Bytes()
		if !bytes.Contains(line, []byte(`"compiler-message"`)) {
			continue
		}
		var msg cargoMessage
		if err := json.Unmarshal(line, &msg); err != nil {
			return nil, fmt.Errorf("reading a cargo message: %w", err)
		}
		d := msg.Message
		p := d.primary()
		if p.FileName == "" {
			continue
		}
		if !keep(p.FileName) {
			site := callSiteIn(p, keep)
			if site == nil {
				continue
			}
			*p = *site
		}
		key := fmt.Sprintf("%s|%d|%d|%s|%s|%s", p.FileName, p.ByteStart, p.ByteEnd, d.code(), d.Message, p.label())
		if seen[key] {
			continue
		}
		seen[key] = true
		out = append(out, d)
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	sort.SliceStable(out, func(i, j int) bool {
		a, b := out[i].primary(), out[j].primary()
		if a.FileName != b.FileName {
			return a.FileName < b.FileName
		}
		return a.ByteStart < b.ByteStart
	})
	return out, nil
}

type diagExpansion struct {
	Span          diagSpan `json:"span"`
	MacroDeclName string   `json:"macro_decl_name"`
}

// callSiteIn follows a span that lies inside a macro's definition (a std macro's file) out
// through its expansions to the first call site in a file `keep` accepts: the
// `assert_eq!(a, b)` a comparison error is really about. The returned span is primary, keeps
// the original label, and names the macro.
func callSiteIn(p *diagSpan, keep func(string) bool) *diagSpan {
	for e := p.Expansion; e != nil; e = e.Span.Expansion {
		if keep(e.Span.FileName) {
			site := e.Span
			site.IsPrimary = true
			site.Label = p.Label
			site.Macro = strings.TrimSuffix(e.MacroDeclName, "!")
			site.Expansion = nil
			return &site
		}
	}
	return nil
}
