package checks

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// A reason quoting code (`Gathering<FactKind, Fact>`, `&Path`, `-> T`) must land in
// the rewritten allowlist as written. Each entry type with its own `MarshalJSON`
// used to go through `json.Marshal`, which escapes `<`, `>`, and `&` to `\u003c`
// and friends regardless of the writer's `SetEscapeHTML(false)`, so every rewrite
// churned unrelated rows.
func TestWriteJSONAllowlistKeepsReasonsVerbatim(t *testing.T) {
	const reason = "`Gathering<FactKind, Fact>` takes `&Path` -> `Option<T>` && more"
	entries := map[string]any{
		"jscpd":     jscpdPairLimit{Lines: 3, Reason: reason},
		"fileLen":   fileLengthLimit{Lines: 900, Reason: reason},
		"claudeMd":  claudeMdWordLimit{Words: 700, Reason: reason},
		"e2eBare":   e2eDurationEntry{Reason: reason},
		"e2eTimed":  e2eDurationEntry{MaxMs: 10, Reason: reason},
		"plainText": reason,
	}
	path := filepath.Join(t.TempDir(), "allowlist.json")
	if err := writeJSONAllowlist(path, entries); err != nil {
		t.Fatalf("write: %v", err)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	text := string(raw)
	if strings.Contains(text, `\u00`) {
		t.Errorf("the rewrite escaped HTML characters:\n%s", text)
	}
	if got := strings.Count(text, reason); got != len(entries) {
		t.Errorf("want the reason verbatim %d times, got %d:\n%s", len(entries), got, text)
	}
}
