/**
 * Line-surgical edits to one `"@key": { … }` twin in a catalog file, for the screenshot
 * coupler (`couple-screenshots.ts`). Every other byte, the blank lines that group the
 * catalog included, stays as it was: a `JSON.parse` → `JSON.stringify` round-trip would
 * reflatten every file on every run.
 */

/**
 * Index of the `}` matching the `{` at `braceStart`, skipping braces inside
 * strings (`placeholders` nests, so a naive "first }" is wrong). Returns -1 if
 * the object never closes.
 */
function matchingBraceIndex(text: string, braceStart: number): number {
  let depth = 0
  let inStr = false
  let esc = false
  for (let i = braceStart; i < text.length; i++) {
    const c = text[i]
    if (inStr) {
      if (esc) esc = false
      else if (c === '\\') esc = true
      else if (c === '"') inStr = false
      continue
    }
    if (c === '"') inStr = true
    else if (c === '{') depth++
    else if (c === '}') {
      depth--
      if (depth === 0) return i
    }
  }
  return -1
}

/**
 * Sets, replaces, or REMOVES one top-level string `field` of a single
 * `"@key": { … }` object in the catalog text, touching only that object. With a
 * non-null `value`, replaces an existing `"field": "…"` line if present, else
 * inserts the field as the LAST property (appending a comma to the previous last
 * line). With `value === null`, removes the field's line entirely (and its comma,
 * keeping the object's comma structure valid); a no-op if the field is absent.
 * Returns the new text, or null if the twin block can't be located/parsed
 * (caller then skips, never corrupting the file).
 *
 * Used for both `screenshot` and `screenshotNote` (representative-coupling note).
 * @param metaKey e.g. `@common.ok`
 * @param field e.g. `screenshot` or `screenshotNote`
 * @param value the JSON string value, or null to remove the field
 */
export function setTwinField(text: string, metaKey: string, field: string, value: string | null): string | null {
  // The twin is an oxfmt'd object opening on its own line: `  "@key": {`.
  const open = `  ${JSON.stringify(metaKey)}: {`
  const openIdx = text.indexOf(open)
  if (openIdx === -1) return null
  const braceStart = openIdx + open.length - 1 // index of the `{`
  const closeIdx = matchingBraceIndex(text, braceStart)
  if (closeIdx === -1) return null

  const body = text.slice(braceStart + 1, closeIdx) // between the braces
  const before = text.slice(0, braceStart + 1)
  const after = text.slice(closeIdx)

  // Matches the existing top-level `"field": "…"` line (4-space indent, the
  // twin's own field indent) plus a trailing comma if it has one (`$1`).
  const fieldName = field.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const existingRe = new RegExp(`\\n {4}"${fieldName}": "(?:[^"\\\\]|\\\\.)*"(,?)`)
  const m = existingRe.exec(body)

  if (value === null) {
    // Remove the field. If it had a trailing comma, dropping the line + comma
    // keeps the rest valid. If it was the LAST field (no trailing comma), also
    // drop the comma on the now-last preceding field.
    if (!m) return before + body + after // already absent
    if (m[1] === ',') return before + body.replace(existingRe, '') + after
    // Last field: remove it AND the preceding field's trailing comma.
    const withoutField = body.replace(existingRe, '')
    const trimmedComma = withoutField.replace(/,(\n {2})$/, '$1') // last `,` before `\n  }`
    return before + trimmedComma + after
  }

  if (m) {
    const replaced = body.replace(existingRe, `\n    "${field}": ${JSON.stringify(value)}${m[1]}`)
    return before + replaced + after
  }

  // Insert as the last field: append a comma to the current last property line,
  // then add the new field line before the closing brace. `body` ends with the
  // last field then a newline + the close brace's indent; trim that trailing
  // newline/indent, add `,\n    "field": "…"\n  ` back.
  const trimmed = body.replace(/\n {2}$/, '') // drop the "\n  " before `}`
  return before + trimmed + `,\n    "${field}": ${JSON.stringify(value)}\n  ` + after
}
