package main

import (
	"fmt"
	"strconv"
	"strings"
	"unicode/utf8"
)

// A small JSON writer that prints exactly what `jq --indent 2` prints, which wrote every
// `latest.json` up to v0.61.0: members in the order given, two-space indents, and jq's string
// escaping (Go's `encoding/json` escapes U+2028, U+2029, and `<>&`, and leaves DEL bare, so its
// bytes would differ). The manifest every installed copy polls stays byte-stable across the move.

type member struct {
	key   string
	value any // string, int64, object, or []any
}

type object []member

func writeJSON(b *strings.Builder, v any, indent int) error {
	switch v := v.(type) {
	case string:
		writeString(b, v)
	case int64:
		b.WriteString(strconv.FormatInt(v, 10))
	case object:
		if len(v) == 0 {
			b.WriteString("{}")
			return nil
		}
		b.WriteString("{\n")
		for i, m := range v {
			b.WriteString(strings.Repeat("  ", indent+1))
			writeString(b, m.key)
			b.WriteString(": ")
			if err := writeJSON(b, m.value, indent+1); err != nil {
				return err
			}
			if i < len(v)-1 {
				b.WriteString(",")
			}
			b.WriteString("\n")
		}
		b.WriteString(strings.Repeat("  ", indent) + "}")
	case []any:
		if len(v) == 0 {
			b.WriteString("[]")
			return nil
		}
		b.WriteString("[\n")
		for i, e := range v {
			b.WriteString(strings.Repeat("  ", indent+1))
			if err := writeJSON(b, e, indent+1); err != nil {
				return err
			}
			if i < len(v)-1 {
				b.WriteString(",")
			}
			b.WriteString("\n")
		}
		b.WriteString(strings.Repeat("  ", indent) + "]")
	default:
		return fmt.Errorf("can't write %T as JSON", v)
	}
	return nil
}

// writeString escapes the way jq's `jvp_dump_string` does: `"` and `\`, the short forms for
// \b \t \n \f \r, `\u00XX` for the other control characters and DEL, everything else as UTF-8.
// Invalid UTF-8 becomes U+FFFD, as `jq -R` reads it.
func writeString(b *strings.Builder, s string) {
	b.WriteByte('"')
	for len(s) > 0 {
		r, size := utf8.DecodeRuneInString(s)
		s = s[size:]
		switch {
		case r == '"':
			b.WriteString(`\"`)
		case r == '\\':
			b.WriteString(`\\`)
		case r == '\b':
			b.WriteString(`\b`)
		case r == '\t':
			b.WriteString(`\t`)
		case r == '\n':
			b.WriteString(`\n`)
		case r == '\f':
			b.WriteString(`\f`)
		case r == '\r':
			b.WriteString(`\r`)
		case r < 0x20 || r == 0x7f:
			fmt.Fprintf(b, `\u%04x`, r)
		default:
			b.WriteRune(r)
		}
	}
	b.WriteByte('"')
}
