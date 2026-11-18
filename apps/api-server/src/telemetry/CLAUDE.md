# Telemetry

Everything the app sends home: `telemetry.ts` (`/crash-report`, `/update-check`), `download.ts` (`/download`),
`heartbeat.ts` (plus its event relay), the `error-report*` quartet, and `feedback.ts`. File map: DETAILS § Files.

## Must-knows

- **The `anal_` analytics id and the `diag_` diagnostics id never co-occur on a request.** A crash report carries a
  `diag_` id only (an `anal_`-shaped `diagId` is a 400), feedback neither. That keeps analytics unjoinable to identity.
- **Optional fields from the Rust client arrive as `null` OR `undefined`** (serde `Option::None` → JSON `null`): a
  `!== undefined`-only validator silently drops the upgrade-window reports we want. Pattern:
  `value !== undefined && value !== null && <shape check>` (`validateCrashReportShape` is the canonical form).
- **`top_function` is the only crash-grouping key and must skip the panic machinery** (`extractTopFunction`), else every
  panic groups under `install_panic_hook` and the nightly email can't tell unrelated bugs apart.
- **`/feedback` and `/heartbeat` AWAIT their D1 writes** (failure → soft 502, the app retries); the rest are
  fire-and-forget. ❌ Don't flip either. A beat's 2xx lets the app clear its spool, so beat and events commit in ONE
  `batch`, and a malformed EVENT is dropped, never a 400 (it would block the spool forever). DETAILS § Heartbeat.
- **Relayed events stay in D1**: ❌ never forward them to a third party; the privacy policy promises it.
- **Eviction spares bundles under `EVICTION_MIN_AGE_DAYS` (60) and is all-or-nothing** (it pauses intake rather than
  half-evicting, resuming at the LOW watermark). Drop either and unauthenticated `/error-report` becomes a delete
  primitive. DETAILS § Eviction.
- **Capped bodies go through `readCappedBody` (`../types.ts`), ❌ never `c.req.parseBody()` / `c.req.text()`**:
  `content-length` is advisory, so those buffer up to 100 MB in a 128 MB isolate before any cap looks.
- **Intake only queues a D1 row; the cron files the card and mails the digest** (`../report-outbox.ts`,
  `../report-digest.ts`): `/error-report` writes `error_reports` and `/error-report/:id/amend` writes
  `error_report_amendments`, both awaited before the response. ❌ Never file a GitHub issue from a route.
  `apps/api-server/DETAILS.md` § The reports repo.
- **The `report:{id}` KV index write is AWAITED before the 200**, alone among this route's side effects: that same
  response hands out the amend credential, so a later index opens nothing. ❌ Never move it to `postUploadWork`; a
  failed put answers 200 with `amendKey: null`.
- **Only an amend key's SHA-256 is stored, and `ERR-XXXXX` is never proof of ownership** (31^5 values, shown to the
  user). Compare via `constantTimeEqual`. An `.amend.json` sidecar is evicted with its bundle, ❌ never on its own.
  DETAILS §§ The report index, Amendments.
- **The error-report id comes from the client and is used as-is** (validated against `^ERR-[23456789A-Z]{5}$`); on an R2
  key collision retry with a fresh UUID, ❌ never a fresh id — the user already read it in the preview dialog.
- **A download's UA family is computed at WRITE time, against the file's OS**, outliving the raw `user_agent` the
  retention sweep clears at 90 days.
- **Windows and Linux downloads stay private until `PUBLIC_DOWNLOAD_OSES` (`wrangler.toml`) lists them** (macOS is
  always public): off the list, `/download` answers 404 before any lookup or row. ❌ Never route around it. The app's
  `os` field is an enrichment: unknown is NULL, ❌ never a 400.
- **`/download` never stores a guessed version.** `latest` resolves through `latest.json` (GitHub as fallback); when
  neither answers it 302s and writes NO row, because a guess corrupts the per-version breakdown.
- **`sanitizeRef` (`[a-z0-9._:-]`) is a cross-repo contract** with the website's normalizer, and `sanitizeRefererHost`
  keeps the HOST only, so a referring page's query string can't leak.

Payloads, columns, R2 keys, eviction, intake, and email: `DETAILS.md`. Read it before any non-trivial work here:
editing, planning, reorganizing, or advising.
