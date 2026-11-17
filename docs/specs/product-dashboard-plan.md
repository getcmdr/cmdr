# Product dashboard rework: plan

Branch: worktree `.claude/worktrees/product-dashboard`. Route `/product`
(`apps/analytics-dashboard/src/routes/product/`). Model to copy: the Acquisition rework
(`AcquisitionTrendsSection.svelte`, `TrendRow`, `StatsGrid`, `DailyChart`, `InfoPopover`, `#lib/acquisition-stats.ts`).

## Lead's resolutions of the open questions (2026-11-17)

David approved the layout and said: move D7/D28 (not duplicate), reach as a dotted line, onboarding funnel now, fold the
Paddle fix in, one land, adjacent clear wins welcome, and the version chart shows more versions with no grey "Older".
The lead resolved the planner's open questions as below; they're reported to David and are one-constant swaps if he
disagrees.

1. MAU = 28 days (four whole weeks, so the weekday mix never shifts it).
2. Rollout chart switches to heartbeat `byVersion` (one population with the headline, 181 days of history), same look.
3. "Paid licenses" = first purchases (Paddle `origin` not `subscription_recurring`).
4. Funnel: no `fdaGranted` for now (needs a raw config-shape read; list it as a follow-up). Friction and Dismissed
   groups as proposed; the upgrade nudge goes to the Money card; `advanced.oldMacosNoticeShown` excluded.

5. On 90d, D7/D28 "vs previous" must not read `–`: `/product` fetches the funnel at the length the 90d comparison needs
   (about 210 days; the funnel source already caches per fetch length, see `funnelDaysFor`), so 7d/30d keep sharing
   `/`'s 181-day entry.
6. The `/` funnel's purchases column (`fetchPaddlePurchasesByDay`) uses the same first-purchase `origin` allowlist as
   D6, so renewals and card updates stop counting as purchases there too (one shared predicate in `paddle.ts`).

## Loud rules (read before every milestone)

1. **Complete days only.** Today (UTC) is partial: drawn faded, never a headline, excluded from every stat. Every stat
   takes an exclusive `end` day (`#lib/acquisition-stats.ts:6-7`, `windowOf` at :67).
2. **Ratios of sums**, never averages of daily ratios (`acquisition-stats.ts:8-10`, `pooled` :111).
3. **`null` is "unknown", `0` is a real zero.** A null renders as `–`. A missing worker endpoint or field renders "Not
   available yet", never zeros.
4. **Install ids never leave the Worker.** Fold per-install rows into counts in TypeScript before responding (pattern:
   `assembleFeatureUsage`).
5. **D1 bills rows READ, temp tables included.** Each new query reads each heartbeat row in its window once. ❌ No
   `GROUP BY anal_id` over all of `heartbeat`, no self-joins of a per-install-day set against a date list, no
   `VALUES`-map joins.
6. **No name/string matching on setting ids.** UI-state settings get a TYPED tag in the desktop registry; the dashboard
   reads the tag from the manifest. Explicit id lists are fine; regexes or suffix checks on `*Seen`/`*Shown` are not.
7. **One color means one property** (DETAILS § Color coding). Gold `#ffc206` = getcmdr.com/the product, purple = mtp-rs,
   green = veszelovszki.com, cyan = getprvw.com, blue = Homebrew, grey = unknown.
8. **(i) popovers: a gist `<p>`, then a `<ul>` of bullets, then `Source:`** (`InfoPopover.svelte`). No
   `SectionDescription`/`Methodology` text walls on `/product`.
9. **`{#if source.ok}` first, error in `{:else}`.** Never import `#lib/server/*` as a runtime value into components; run
   `pnpm build` after touching imports across that boundary.
10. **Additive worker contracts only.** Dashboard (`deploy-dashboard.yml`) and Worker (`deploy-api-server.yml`) deploy
    in parallel on push to `main`, so the dashboard briefly runs against the old Worker. Every new endpoint or param
    must degrade to "Not available yet" there.
11. Strip milestone tags (`M1`, `Milestone 2`…) from code, comments, test names, and docs before reporting done. This
    plan file keeps them.

## What the current code does (fresh read, 2026-11-17)

- `routes/product/+page.svelte:17-28`: 3-column grid of six cards. `+page.server.ts:12`: `resolveSelection(range)` only.
  `fetchProductData` (`fetch-all.ts`) loads cloudflare, paddle, license, feedbackAndErrors, settingsAdoption,
  featureUsage.
- Problem 1, today mixed in: `ActiveUseSection.svelte:48-49` `latestBound` = last element (`active-installs.ts:44-46`),
  `/admin/heartbeat-dau` includes today. Feature usage window is `now - days*86400000`
  (`api-server/src/admin/feature-usage.ts:324`), rolling to now.
- Problem 2: `formatBound` (`active-installs.ts:53-57`) prints `floor–reach`.
- Problem 4: `mostChanged` (`#lib/settings-adoption.ts:28-36`) lists every moved setting. "changed by version" =
  `defaultLabel === null`.
- Problem 5: Payment/Retention thin Paddle cards; churn = canceled ÷ all (`RetentionSection.svelte:34`).
- Problem 6: D7/D28 rows in `AcquisitionTrendsSection.svelte:666-745`.
- Problem 8, Paddle (verified against Paddle docs 2026-11-17): (a) operators are uppercase (`created_at[GTE]`);
  `paddle.ts:118` and `:144` send `[gte]`, ignored, so all transactions come back. (b) Latent: `meta.pagination.next` is
  a full URL; `paddleFetchAll` (`paddle.ts:69-71`) appends `after=<whole URL>`, so page 2 is malformed. The funnel's
  `fetchPaddlePurchasesByDay` (`paddle.ts:114-127`) has the same filter bug.
- Adjacent bug: "Awaiting reply" reads feedback for the picker's range (`sources/feedback-and-errors.ts:30-34`), so on
  7d an older unanswered message vanishes from the count.
- `/admin/funnel` (181 days, cached `funnel-v4`) carries per-day `dau`, `newInstalls`, `d7*`, `d28*`, "so far" fields.
  Returning = dau − newInstalls. Lacks app hours, rolling distinct (WAU/MAU), per-version actives.
- Settings registry: `apps/desktop/src/lib/settings/definitions/*.ts`, typed by `SettingDefinition`
  (`apps/desktop/src/lib/settings/types.ts:201`). Generator: `apps/desktop/scripts/gen-analytics-defaults-lib.ts`
  (`parseFile`). Manifest `{ note, promotedThrough, versions, next }`. Check:
  `scripts/check/checks/analytics-settings-defaults.go` (regen-and-diff whole file). `scripts/release.sh:183` runs
  `--promote`.
- Version chart: `updateVersionSeries` (`chart-helpers.ts:71-97`) takes six newest versions from `VERSION_PALETTE`
  (`:23`, reuses property colors) and folds the rest into grey "Older" (`COLOR_OLDER`, `:24`). Drawn by
  `StackedBarChart.svelte`.

## Decisions

- **D1. Active-install definitions.** DAU = distinct heartbeat installs per UTC day. WAU(D) = distinct installs with any
  beat in [D−6, D]. MAU(D) = same over [D−27, D]. Stickiness = Σ DAU ÷ Σ MAU over a window. Hours per active day = Σ
  appHours ÷ Σ DAU.
- **D2. Headline tiles.** Value at yesterday. "vs previous" uses the picker's N (7/30/90). Point metrics (WAU, MAU)
  compare against the value N days earlier; pooled metrics (stickiness, hours, D7) compare last N complete days (or
  final cohorts) vs the N before. Paying share: now vs N days ago. Each tile gets an (i). Coverage at N = 90 with the
  shared 181-day funnel: D7's final cohorts end at today−8, so 90 + 90 cohorts need 188 days and D28 needs 209; both "vs
  previous" read `–` on 90d unless the product composer asks the funnel for more days (a separate cache entry from `/`).
  Active-installs stats fit exactly (180 complete days).
- **D3. Reach** survives only as a faint DOTTED line on the DAU chart. `formatBound`, `latestBound`,
  `largestUnseenShare` go; `boundsByDay` stays or folds into the new helper. DETAILS § "Active installs as a range"
  becomes a Decision/Why: real heartbeat numbers, reach as context, David's call 2026-11-17.
- **D4. Release rollout.** Headline "X% of active installs on vLatest, N days after release", heartbeat-based
  (`byVersion` ÷ DAU, yesterday). Stacked chart uses the same heartbeat `byVersion` via one
  `versionStack(days, perDayByVersion)` helper. "Latest" = newest stable GitHub release (`GitHubRelease`,
  `sources/github.ts:126-131`; add `fetchGitHubSource` to the product composer), fallback newest version seen.
  `parseRelease` drops `draft`/`prerelease` today, so parse both and skip either; strip the tag's leading `v` before
  matching heartbeat `app_version`. N = yesterday − publish day.
- **D5. Version colors.** Color follows the version, never its rank (`country-colors.ts` precedent). New client-safe
  `#lib/version-colors.ts`: `versionColor(version)` OKLCH from a golden-angle hue walk keyed on a stable version
  ordinal, so consecutive versions sit far apart in hue; lightness alternates across three bands (~0.62/0.72/0.82),
  chroma ~0.13, hues within ±18° of reserved property hues (gold ≈ 85°, blue ≈ 255°) skipped. Gold is never a version.
  Validate with the dataviz skill's `validate_palette.js --mode dark --surface #1a1917` over the last 20 versions in
  stack order, run recorded in the file header. A test pins adjacent pairs of the last 20 ≥ the validator's adjacent
  floor, plus ≥ 3:1 on the surface. No "Older" bucket: every version with ≥1 install in the window draws its own layer.
  Legend newest first. DETAILS § Color coding gains a "Versions" bullet.
- **D6. Paying share** = paid licenses ÷ MAU (yesterday). Paid licenses = completed Paddle transactions with `origin`
  `web` or `api` (the checkout or API-created first purchase). An allowlist, because "not `subscription_recurring`"
  would also count `subscription_payment_method_change` (a card update, often a zero-value completed transaction),
  `subscription_update`, and `subscription_charge` (Paddle docs, transaction origin, checked 2026-11-17). Parse `origin`
  in `parseTransaction`; an unknown origin isn't a first purchase.
- **D7. Paddle fetch shape.** Fetch ALL completed transactions once (no `created_at` filter), cache under one key,
  window in TypeScript (revenue in period, all-time, purchases list, paid-license series). Fix `[GTE]` in
  `fetchPaddlePurchasesByDay`; fix pagination to follow `next` as a full URL (assert it starts with
  `https://api.paddle.com/`). Drop the `/subscriptions` fetch and churn. Currency: today `PaymentSection.svelte:28-29`
  sums `details.totals.total` across transactions and labels the sum with the first one's currency, so mixed-currency
  checkouts add up wrong. Sum one currency: parse `details.payout_totals` (`total`/`earnings` + `currency_code`, the
  payout currency) and sum those, or group by `currencyCode` and print one line per currency; never add amounts across
  currencies. A refunded purchase stays `completed` (refunds are adjustments), so revenue is gross of refunds; say so in
  the (i).
- **D8. Settings tag** `kind?: 'state'` on `SettingDefinition` (absent = preference). Manifest gets top-level sorted
  `stateKeys: string[]`, built from the registry plus any previously recorded state key no longer in the registry
  (retired flags still show up in old installs); a key in the registry always follows the registry. `resolveAdoption`
  (server-side) stamps `kind: 'preference' | 'state'` on each `SettingAdoption`; the manifest never reaches the browser.
- **D9. Onboarding funnel.** Explicit ordered id list in client-safe `#lib/onboarding-funnel.ts`, with a test that every
  id is a `state` key in the manifest.
  - Step: readable installs on a build that has the flag → `onboarding.completed`.
  - Discovered (sorted by share): `fileExplorer.quickFilterIntroSeen`, `fileExplorer.git.portalIntroSeen`,
    `behavior.doubleClickOnPaneNotificationSeen`, `network.firstTriggerDone`, `behavior.serversPinHintSeen`,
    `advanced.exitFullScreenOnEscapeHintShown`, `behavior.textEditorHintSeen`, `behavior.openTerminalHereToastSeen`,
    `behavior.revealActivationNoticeSeen`, `behavior.fileSystemWatching.globalGoToLatestShortcut.acknowledged`,
    `indexing.firstStaleDialogShown`.
  - Hit friction: `fileExplorer.quickFilterTypingHintSeen`.
  - Dismissed: `behavior.adbHintDismissed`, `aboutThis.aiHintDismissed`.
  - Money card: `onboarding.upgradeNudgeShown` ("saw the upgrade nudge").
  - Excluded: `advanced.oldMacosNoticeShown`.
  - Capability-gated flags (`needsCapability`) carry a popover caveat about the denominator.
- **D10. State-tagged keys** (executing agent re-greps): once-ever hints in `advanced.ts:240-415` plus
  `fileExplorer.quickFilterTypingHintSeen` (:200), `advanced.oldMacosNoticeShown` (:664), `onboarding.upgradeNudgeShown`
  (:674), `ai.cloudConsentRevokePending`, `askCmdr.consentRevokePending`,
  `onboarding.completed`/`fullDiskAccessChoice`/`termsAcceptedVersion`/`termsAcceptedAt` (:719-760),
  `behavior.switcherFavoritesExpanded` (`behavior.ts:42`), `behavior.dockPinNudgeOfferedAt`,
  `behavior.fileSystemWatching.downloadsToastCollapsed` (:518), `network.firstTriggerDone`, `network.nearbyServersGroup`
  (`file-systems.ts`), `whatsNew.lastSeenVersion`, `updates.unwritableNoticeVersion` (`updates-privacy.ts`). NOT state
  (user's chosen behavior): `fileExplorer.suppressQuickLookHint`, `suggestedOps.suppressApprovalNotice`,
  `fileOperations.showAlreadyGoneToast`, `fileViewer.suppressBinaryWarning`, `fileOperations.keepProgressOpenWhenDone`,
  `aboutThis.aiVerdict`.

---

## Milestone 1: api-server endpoints and tests

**Scope:** `apps/api-server/src/admin/`: new `active-installs.ts` + route mounted beside `admin`/`funnel`;
`feature-usage.ts` gains `?end=`; tests; admin CLAUDE.md + DETAILS.md.

**Intentions:**

- `GET /admin/active-installs?days=N` (reuse `parseDays` from `funnel.ts`, 1..400, default 181, 400 on bad input).
  Returns `{ days: ActiveInstallsDay[], windows: VersionWindow[] }`.
  - `ActiveInstallsDay = { date, dau, newInstalls, appHours, wau, mau, byVersion: Record<string, number> }`, oldest
    first, last N UTC days INCLUDING today.
  - `VersionWindow = { days: 7|30|90, since, until, installs, byVersion }`: distinct installs over complete-day windows
    ending yesterday. `byVersion[v]` = distinct installs with any beat on `v` in the window (the "installs exposed to v"
    denominator errors-per-100 needs), so the values can sum past `installs`; say so in the type's doc comment. Per-day
    `byVersion` is the other rule (highest version that day) and sums to `dau`.
  - Query 1 (each heartbeat row from `min(since−27d, today−90d)` on, read once via `idx_heartbeat_created`; the
    `today−90d` floor keeps the 90-day window whole when `days` < 91). Bind the bound as a date-only `YYYY-MM-DD`, which
    compares correctly against `created_at`'s `YYYY-MM-DD HH:MM:SS`:
    `SELECT date(created_at) AS date, anal_id, app_version AS version, SUM(COALESCE(uptime_seconds,3600)) AS seconds FROM heartbeat WHERE created_at >= ?1 GROUP BY date, anal_id, app_version`.
  - Query 2:
    `SELECT date(first_beat_at) AS date, COUNT(*) AS count FROM install_summary WHERE first_beat_at >= ?1 GROUP BY date`.
  - Pure `assembleActiveInstalls(rows, newRows, dates)`: DAU and appHours per day; an install's version that day = its
    highest semver that day by numeric compare (❌ never string `MAX`; and not the dashboard's `compareSemverDesc` as
    is, which returns `NaN` for a non-numeric part like `0.62.0-beta.1` and breaks the sort: use a total order where a
    non-numeric part sorts below any number, ties broken by string compare); WAU/MAU by per-install interval coverage
    (merge overlapping [a, a+6] / [a, a+27] intervals, +1/−1 difference array); windows by per-install sets; ids
    dropped.
  - appHours shares the expression with `heartbeatDauSql` (`admin.ts:249-255`); don't fork it.
- `/admin/feature-usage?range=…&end=YYYY-MM-DD` (inclusive last day; malformed or future → 400). Window = [end−days+1
  00:00, end+1 00:00). Upper bound added to `featureUsageSql` (ISO strings) and `activeInstallsSql` (SQLite format),
  around `feature-usage.ts:295-326`. Response gains `until`. Without `end`, byte-identical. "Future" means later than
  today (UTC); `end=today` is allowed, as on `/admin/funnel/countries`. Bounds as strings:
  `occurred_at >= '<start>T00:00:00.000Z' AND occurred_at < '<end+1>T00:00:00.000Z'`,
  `created_at >= '<start> 00:00:00' AND created_at < '<end+1> 00:00:00'`.
  - Previous-period honesty: a previous window that starts before a feature's events were ever stored reads `0`, which
    the change column would show as growth from nothing. With `end`, the response also gains per-feature `firstSeenAt`
    (min `occurred_at` over its mapped event names; one indexed lookup each on `idx_analytics_event_event_occurred`,
    folded into `seenEventsSql`'s `json_each` probe). The dashboard prints `–` for the change when `firstSeenAt` is
    after the previous window's start. It's a lower bound for prop-filtered matches (`pane_navigated` predates
    `volume_kind=archive`), which is acceptable.

**Landmines:** measure `meta.rows_read` in a local `wrangler dev` run on the seeded DB and record it in DETAILS; if
GROUP BY temp rows double reads, compare against raw rows and pick the cheaper. `created_at` vs `occurred_at` formats. A
day with no beats still appears with zeros. Clip WAU/MAU intervals to the output range so a beat yesterday doesn't spill
coverage past today. The synthetic sweep only deletes an install's beats after seven quiet days, so the last week's
DAU/WAU can still hold not-yet-swept synthetic installs that later vanish; note it in the (i) and DETAILS rather than
"fixing" it here (same caveat as `/admin/heartbeat-dau`).

**Test plan:** pure tests (interval merge, interval clipped at today, highest semver incl. a pre-release string, window
`byVersion` counts an install that upgraded under both versions, zero-fill, today included, no ids); SQL tests on real
SQLite seeded via `/heartbeat` INSERTs (`dau` equals `/admin/heartbeat-dau`, `newInstalls` equals `/admin/funnel`);
route auth + 400; `feature-usage.test.ts` for `end` bounds (an event at `end+1 00:00:00.000Z` excluded, one at
`start 00:00:00.000Z` included), `until`, `firstSeenAt`, unchanged without `end`. Gate:
`pnpm check api-server-tests api-server-typecheck api-server-eslint` (check exact names via `pnpm check --help`).

**DONE:** both endpoints return the shapes, tests green, rows_read recorded, admin C+D.md updated (endpoint + "highest
semver, never MAX").

## Milestone 2: desktop registry tag, manifest, and check

**Scope:** `apps/desktop/src/lib/settings/types.ts` (`kind?: 'state'`, doc comment: remembered UI or onboarding state,
not a preference; drives analytics only), tags on D10 entries, `gen-analytics-defaults-lib.ts` + `.ts` (parse `kind`,
build `stateKeys` per D8; `DefaultsManifest`, `readManifest`, `emptyManifest`), `gen-analytics-defaults.test.ts`,
regenerated `settings-defaults.gen.json`, `MANIFEST_NOTE` line. Docs: settings DETAILS.md, scripts DETAILS.md,
`docs/guides/adding-a-new-setting.md` step.

**Landmines:** read `kind` as a string literal only; keep it out of config-shape logic; Go check struct unchanged; run
`pnpm check analytics-settings-defaults` twice (idempotent); `SettingDefinitionSource` `Omit` keeps it.

**Test plan:** `kind: 'state'` collected; non-literal ignored; retired state key survives; reclassified key follows
registry; sorted and stable. Gate: `pnpm check analytics-settings-defaults`, `pnpm check desktop-svelte-check` (`--fast`
runs no typechecker, and a `kind: 'state'` literal can widen to `string` in a definitions array), `pnpm check --fast`,
`pnpm check svelte-tests-related`.

**DONE:** manifest has `stateKeys`, check green and idempotent.

## Milestone 3: dashboard data layer

**Scope:** `src/lib/server/sources/` (new `active-installs.ts`; `paddle.ts` rework; `feature-usage.ts` with previous
period; feedback `all` for awaiting), `settings-defaults.ts` (`kind`, `latestDefaultLabel` for a "moved in vX" note),
`fetch-all.ts` (product composer adds `funnel`, no country, per resolution 5: `funnelFetchDays` (181) on 7d/30d,
`funnelMaxFetchDays` (400) on 90d, the two lengths `funnelDaysFor` already caches, so no new cache key;
`activeInstalls`; `github`), `+page.server.ts`, `fetch-all.subset.test.ts`. New client-safe pure modules with Vitest:
`#lib/product-stats.ts`, `#lib/money.ts`, `#lib/rollout.ts`, `#lib/version-colors.ts`, `#lib/onboarding-funnel.ts`;
extend `#lib/settings-adoption.ts` (`preferencesOnly`, `topMoved(n=15)`), `#lib/feature-usage.ts` (change vs previous,
share-bar width, `foldNoData`), `#lib/feedback-and-errors.ts` (`errorsPer100`, `errorsSinceRelease`).

**Intentions:** `fetchActiveInstallsData` → `SourceResult<ActiveInstallsData | null>`, null on 404 (older Worker) → "Not
available yet". `days=181`, cache `active-installs-v1`, 1 h. Feature usage: two calls (current `end=yesterday`, previous
`end=yesterday−N`), `previous: null` when `until` missing. An old Worker ignores `end`, so without `until` the CURRENT
numbers are also the legacy rolling window that includes today: show them with the legacy window line, never labeled as
ending yesterday. Paddle per D7, ADDITIVELY: keep `transactions`, `activeSubscriptions`, `subscriptionsByStatus`, and
the `/subscriptions` fetch until Milestone 6, because `PaymentSection`/`RetentionSection` (deleted in Milestone 4) and
`format-report.ts` plus its fixtures (rewritten in Milestone 6) still read them; new transaction fields that are
required get added to the report fixtures mechanically, with no golden change. "Awaiting reply": fetch
`/admin/feedback?range=all` once and window the feedback list to the picker's range in TypeScript, so the count and the
list come from one response. Error normalization: error rows filtered to the same complete-day window, ÷
`windows[N].byVersion[v]`; fewer than 5 installs prints the raw count, no rate.

**Landmines:** everything behind `guardedFetch`/`withTimeout`; no `#lib/server` runtime values in components
(`pnpm build`); `stateKeys` server-side only; `fetchDashboardData`, `format-report.ts`, and the report goldens must stay
compiling and green unchanged (that's why the Paddle change is additive); don't touch `funnel.ts` cache keys. The new
pure modules have no component consumer until Milestones 4–5; their tests are their only importers, and if
`dashboard-knip` still flags them, move that module into the milestone that renders it, never add a knip ignore.

**Test plan:** Vitest per pure module; `paddle.test.ts` (`[GTE]`, full-URL `next`, foreign host refused, `origin`
parsed; `subscription_payment_method_change`/`subscription_update` not a paid license; two currencies never summed into
one figure); subset test; `settings-defaults.test.ts` (`kind` stamped, retired key still state). Gate: the dashboard
checks (`pnpm check dashboard`) + `pnpm build`.

**DONE:** `ProductData` carries the new shapes; helpers tested; build green.

## Milestone 4: UI sections, layout, and Acquisition retention move

**Scope:** `routes/product/+page.svelte` (single column of full-width sections), new/reworked sections,
`DailyChart.svelte` + `#lib/daily-chart.ts` (column-mode dotted `overlay` line, legend-named, tooltip row,
`aria-valuetext`), `AcquisitionTrendsSection.svelte` (remove D7/D28 TrendRows :666-745 and orphaned imports; the `/`
country table keeps its pooled D7/D28 columns).

Render order (top to bottom; Milestone 5's sections slot in where named): `NeedsYouStrip`, `ProductHeadline`,
`UsageTrendsSection`, `ReleaseRolloutSection` (M5), `RetentionCohortsSection`, `MoneySection`, `OnboardingFunnelSection`
(M5), `FeatureUsageSection`, `SettingsAdoptionSection` (M5, the existing component stays in place until then),
`FeedbackErrorsSection`.

Sections: `NeedsYouStrip` (one line: the "Awaiting reply" count from the `range=all` feedback, plus "N new error reports
since vLatest" via `errorsSinceRelease` (D4's latest release publish time), each part linking to `#inbox`; a part at
zero or on a failed source is omitted, and the strip renders nothing when both are); `ProductHeadline` (six tiles per
D2: WAU, MAU, stickiness, hours per active day, D7, paying share; change + (i), null → "Not available yet");
`UsageTrendsSection` (DAU stacked new gold + returning gold tint via `tint()` from `#lib/country-colors.ts`, reach
dotted overlay in a neutral grey that isn't `COLOR_UNKNOWN` (that grey means "unknown value") or purple; StatsGrid on
DAU; hours per active install as rate mode; no WAU/MAU chart rows, the tiles carry them); `RetentionCohortsSection`
(D7/D28 TrendRows moved, global only; shared pure helpers moved into a `.ts` both import); `FeatureUsageSection` (share
bar, change in pp, "No data yet: a, b, c" line, window line); `MoneySection` (replaces Payment + Retention, delete both
files: revenue in period with change, all-time, purchases list, activations/active devices, paying share, upgrade-nudge
share); `FeedbackErrorsSection` (`id="inbox"`, errors per 100 column, keeps Mark answered).

Removals: delete `ActiveUseSection.svelte`, `PaymentSection.svelte`, and `RetentionSection.svelte`; with
`ActiveUseSection` gone, `updateVersionSeries`/`VERSION_PALETTE`/`COLOR_OLDER` lose their only consumer, so retire them
here (knip fails otherwise). `StackedBarChart` stays (`DownloadSection` uses it). `#lib/active-installs.ts`'s bound
helpers stay until Milestone 6 (`format-report.ts` imports them).

**Landmines:** rule 9; tint ≥ 3:1; knip/eslint after removals; `buildTrendInputs` stays; `pathsWithSteps` stays `/`
only. The overlay: column-mode `ticks` are computed from stack totals only (`DailyChart.svelte` `ticks`), so include the
overlay's values or a reach above DAU draws off the top; a `null` overlay day breaks the line (`lineRuns`), never drops
to 0.

**Test plan:** `daily-chart.test.ts` overlay geometry; local QA per DETAILS § Local QA, screenshots of `/product` and
`/`; old-Worker path shows "Not available yet". Gate: dashboard checks + build.

**DONE:** `/product` renders these sections in the order above; `/` has no D7/D28 rows; no
`SectionDescription`/`Methodology` in the sections this milestone writes (Settings adoption loses its own in Milestone
5); the three deleted sections are gone with no orphaned exports.

## Milestone 5: rollout chart, onboarding funnel, settings trim

**Scope:** `ReleaseRolloutSection` (D4 headline, stacked DailyChart by `versionColor`, no Older, today faded; layers
stack oldest at the bottom, newest on top, and the legend goes through DailyChart's `legend` prop, newest first, listing
only versions with an install in the view window); `OnboardingFunnelSection` (D9: step bar, then
discovered/friction/dismissed share bars with eligible counts; (i) explains latest heartbeat per install, eligible =
builds that have the flag); `SettingsAdoptionSection` (preferences only, top 15, `<details>` "Show all (N)", tiles kept,
"changed by version" → latest default + "moved in vX"); DETAILS § Color coding (Versions).

**Landmines:** real validator run recorded; tiny version layers don't fold (leave `foldInto` unset); a flag absent from
a build is out of the denominator, never 0. The version ordinal is a pure function of the version string (for example
from major/minor/patch), never its index among the versions in view: that index shifts as old versions leave the window
and would recolor every layer between loads. A test pins that `versionColor('0.61.0')` is the same whatever other
versions exist.

**Test plan:** Vitest for `versionColor`, `onboarding-funnel`, `topMoved`/`preferencesOnly`; screenshots.

**DONE:** rollout, funnel, and settings render.

## Milestone 6: report endpoint and docs

**Scope:** `routes/api/report/format-report.ts` + goldens (all four fixtures, deliberately), `fetchDashboardData`, docs.
Report: Active use as yesterday's WAU/MAU/stickiness/hours with vs-previous, daily lines with today marked partial,
reach as context; Payment → "Money" (window + all-time, purchases, paying share), Retention (subscriptions) removed;
Settings preferences top 15; new lines for onboarding funnel, rollout headline, errors per 100 by version; Daily funnel
keeps D7. Once the report stops reading them, remove `activeSubscriptions`/`subscriptionsByStatus` and the
`/subscriptions` fetch from `paddle.ts` (D7) and `formatBound`/`latestBound`/`largestUnseenShare` from
`#lib/active-installs.ts` (D3), with their tests.

Docs: dashboard CLAUDE.md (Pages line, "Product stats skip today like Acquisition", version colors rule), DETAILS.md
(structure, data sources, § Active installs rewrite, § Settings adoption with `stateKeys`/`kind`, § Feature usage
previous period, new § Product page, Key decisions "David's call 2026-11-17"; rewrite the Key decisions that now
contradict the page: "Every section carries a … blurb" (no longer on `/product`: (i) popovers there), "Active use
daily-active count", and "New installs and Got the latest release" (the update-check chart is replaced by the heartbeat
rollout); `paddle.ts` line under § Data sources), `docs/tooling/analytics-dashboard.md` if relevant, admin docs
verified.

**Landmines:** goldens pin spacing; read the diff. Milestone-tag sweep:
`rg -n '\b(M[0-9][a-z]?|Milestone\s*[0-9]|Phase\s*[0-9])\b' apps/analytics-dashboard apps/api-server/src/admin apps/desktop/scripts apps/desktop/src/lib/settings docs/guides`.

**DONE:** goldens green and reviewed; docs accurate; `pnpm check --fast`, dashboard, api-server, and
`analytics-settings-defaults` checks green.

## Deploy order (lead)

Both apps deploy on the same push; the dashboard tolerates the old Worker. One batched land. After the land, the lead
confirms `/admin/active-installs` answers on prod and reads `rows_read` once against the DETAILS estimate. Then: file
the follow-ups as issues (`fdaGranted` funnel step, plus anything QA turned up), and wipe this spec per
`docs/specs/DETAILS.md` § "Wiping a shipped spec", removing its entry from § "The specs". Before the first milestone,
the lead files the tracking issue and links it from that entry.

## Adjacent clear wins

- "Awaiting reply" read over `range=all`.
- Paddle pagination `next`-URL bug and the funnel's `[GTE]`.
- Drop the unused `/subscriptions` fetch.
- Version palette stops reusing property colors.
- Optional: rename `acquisition-stats.ts` → `trend-stats.ts`; skip if it bloats the diff.

## Invariants register

1. No stat, tile, or headline reads today; today is faded on every chart.
2. Every rate is a ratio of sums; every "vs previous" compares fully covered windows, else `–`.
3. null ≠ 0 end to end; a missing endpoint/field reads "Not available yet".
4. No install id in any admin response; new SQL reads each heartbeat row in its window once; rows_read recorded.
5. Worker contracts additive; `/admin/feature-usage` without `end` byte-identical.
6. State vs preference decided only by the registry's typed `kind`, via `stateKeys`; no id pattern matching.
7. Manifest stays server-side; `settings-defaults.gen.json` generator-owned; check green and idempotent.
8. A version's color depends only on the version; no Older bucket; gold is never a version.
9. Paddle money windowed in TypeScript from one all-time fetch; no revenue figure includes a transaction outside its
   window.
10. D7/D28 cohort charts on `/product` only; report and `/` country table keep their per-day/per-country retention
    numbers.
11. Every source behind `withTimeout`; no `#lib/server` runtime import in browser code; `pnpm build` green.
