/**
 * The report's line collector and its list helper, shared by both halves of `GET /api/report`
 * (`format-report.ts` for the acquisition stages, `product-report.ts` for the Product page's story).
 */

/**
 * Collects the report's lines. `section()` emits the blank line that separates it from whatever came
 * before, so no section has to remember a trailing blank and the sections stay reorderable.
 */
export class ReportWriter {
  private readonly lines: string[] = []

  title(text: string): void {
    this.lines.push(`# ${text}`, '')
  }

  section(text: string): void {
    this.lines.push('', `## ${text}`, '')
  }

  line(text: string): void {
    this.lines.push(text)
  }

  blank(): void {
    this.lines.push('')
  }

  text(): string {
    return this.lines.join('\n')
  }
}

/** A blank line, a heading, then one `  key: value` line per entry. Nothing at all when empty. */
export function writeList<T>(
  w: ReportWriter,
  heading: string,
  items: T[],
  limit: number,
  render: (item: T) => string,
): void {
  if (items.length === 0) return
  w.blank()
  w.line(heading)
  for (const item of items.slice(0, limit)) w.line(render(item))
}
