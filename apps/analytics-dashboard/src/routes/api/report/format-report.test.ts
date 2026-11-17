import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest'
import { formatReport } from './format-report.js'
import { allFailedData, emptyData, mixedData, richData } from './report-fixtures.js'

// A characterization test: the report is consumed by agents and by David, so the exact text is the
// contract. Each case pins the whole output string, not a sample of it; the inputs and the branches
// they reach are in `report-fixtures.ts`.

const richReport = `# Cmdr analytics report (90d)

Generated: 2026-08-02T03:04:05.000Z

## Daily funnel: the last 30 days, one row per UTC day

day | visitors | dl clicks | server dls | installs | D7 | signups | purchases
2026-08-01 | 250 | 25 | 20 | 9 | 33% | 2 | 2
2026-07-31 | - | 0 | 0 | 0 | - | - | 0
2026-07-30 | 100 | 10 | 8 | 4 | 50% | 3 | 1

Channels (last 30 days), downloads by first-touch ref:
- twitter: 15
- reddit: 5
- (none): 3

Download referrers (last 30 days), by the /download hit's Referer host:
- news.ycombinator.com: 10
- (none): 4

Downloads by client (last 30 days), by User-Agent family:
- Human installs: 24 (of 28 raw server downloads)
- human (Mac browser, Homebrew, or curl/wget): 21
- bot / impossible install (Windows, Android, Linux, or X11 UA, excluded): 4
- unknown (no or unrecognized UA, kept in the count): 3

## Awareness: how many people see Cmdr content?

- Total page views: 6,720 (+5.0% vs prior period)
- veszelovszki.com views: 1,200 (+20.0% vs prior period)
- getcmdr.com views: 5,400 (+0.0% vs prior period)
- getprvw.com views: 120
- veszelovszki.com visitors: 800 (-11.1% vs prior period)
- getcmdr.com visitors: 3,200
- getprvw.com visitors: 90 (+100.0% vs prior period)

GitHub stars: 310 total
  getcmdr/cmdr: 300 (last 7d: +10, last 30d: +50)
  vdavid/mtp-rs: 10 (last 7d: +0, last 30d: +0)

Top referrers (getcmdr.com):
  news.ycombinator.com: 900 (69.2%)
  (direct): 400 (30.8%)

Top referrers (getprvw.com):
  google.com: 60 (100.0%)

Top referrers (veszelovszki.com):
  linkedin.com: 30 (100.0%)

## Interest: how many engage with the product page?

- getcmdr.com page views: 5,400 (+0.0% vs prior period)
- Unique visitors: 3,200
- Bounce rate: 50.0%

Download button clicks:
  download-mac: 240

Top pages:
  /: 3,000 views
  /pricing: 700 views

Website visitors by country:
  United States (US): 1,800 (72.0%)
  Sweden (SE): 600 (24.0%)
  (unknown): 100 (4.0%)

Daily page views (PostHog):
  2026-07-31: 400
  2026-08-01: 500

## Download: how many actually download?

(New installs = DMG downloads via getcmdr.com, deduplicated to distinct people per day by daily-hashed IP, bot/link-preview hits dropped by user agent, in-app auto-updates excluded. Raw = every download request.)
- New installs (deduped): 62
- Download requests (raw): 80
- Downloads (GitHub, all-time): 1,234

New installs by source (deduped):
  website: 50 (80.6%)
  homebrew: 9 (14.5%)
  other: 3 (4.8%)

Daily new installs (deduped):
  2026-08-01: 23
  2026-07-31: 39

By version:
  1.10.0: 28 (35.0%)
  1.2.0: 52 (65.0%)

By architecture:
  aarch64: 68 (85.0%)
  x86_64: 12 (15.0%)

By country:
  United States (US): 52 (65.0%)
  Sweden (SE): 25 (31.3%)
  (unknown): 3 (3.8%)

Daily downloads:
  2026-08-01: 28
  2026-07-31: 52

Top countries by architecture:
  United States (US): aarch64: 40, x86_64: 12
  Sweden (SE): aarch64: 25
  (unknown): aarch64: 3

Top countries by version:
  United States (US): 1.2.0: 52
  Sweden (SE): 1.10.0: 25
  (unknown): 1.10.0: 3

Daily downloads by version (top 2):
  2026-07-31: 1.2.0: 52
  2026-08-01: 1.10.0: 28

GitHub releases (all-time):
  v1.10.0: 800 downloads (published 2026-08-01)
  v1.2.0: 434 downloads (published 2026-07-01)

## Active use: how many run the app?

Yesterday, 2026-08-01 (UTC), the last complete day. Today is partial and in no number here.
- Weekly active: 70 (+75% vs. 90 days earlier)
- Monthly active: 90 (+80% vs. 90 days earlier)
- Stickiness: – (– vs. previous 90 days)
- Hours per active day: – (– vs. previous 90 days)
- D7 retention: – (– vs. previous 90 cohorts)

Counts installs with usage stats on; an install that opted out sends nothing. Weekly and monthly
active count installs with a beat in the 7 and 28 days ending yesterday, against the same count 90 days
earlier. Stickiness is daily active over monthly active, and hours per active day is all app hours over
all daily actives, each pooled over the last 90 complete days against the 90 before. A beat from a build
too old to report its runtime counts as an hour. D7 pools the newest 90 cohorts old enough to have one.
The last week can still hold a few synthetic installs the weekly sweep removes later.
"Checked for updates" is context, not a ceiling: distinct addresses that checked for updates, a separate
consent installs with analytics off still ride. A shared connection counts once, a changing address
counts more than once, and anyone with automatic update checks off never appears.

Daily active installs (newest first):
  2026-08-02 (today, partial): 12 active, 1 new, 30 app hours, no update-check data
  2026-08-01: 55 active, 9 new, 500 app hours, 62 checked for updates
  2026-07-31: 40 active, 5 new, 320 app hours, 30 checked for updates

## Release rollout: how far does the newest release spread?

- 82% of active installs ran v1.10.0 yesterday, its release day

Installs by version per day (each install at the highest version it ran that day; newest first):
  2026-08-02 (today, partial): 12 total (v1.10.0: 12)
  2026-08-01: 55 total (v1.10.0: 45, v1.2.0: 10)
  2026-07-31: 40 total (v1.2.0: 40)

## Money: who pays?

- Revenue, last 90 complete days: €89.00 (– vs. previous 90 days), $39.00 (±0% vs. previous 90 days)
- All-time revenue: €89.00, $78.00
- Paid licenses (first purchases through yesterday): 2
- Paying share of monthly active: 2.2% (+0.2 pp vs. 90 days earlier)
- License activations: 42 (7 active devices)
- Saw the upgrade nudge: 2 of 3 (too few to say)

Revenue counts complete days and stays per currency: amounts in two currencies are never added. A
transaction counts in its payout currency when Paddle reports one, else its checkout currency. It's gross
of refunds: a refunded purchase stays completed in Paddle. Purchases are first purchases (checkout or API);
renewals, card updates, and plan changes aren't. Paying share is a trend signal, not a conversion rate:
refunded purchases still count, one license can cover several Macs, and a licensed install with analytics
off is paid but not in monthly active.

Purchases, last 90 days (first purchases, newest first):
  2026-08-02 01:30 UTC (today): $39.00
  2026-08-01 10:00 UTC: $39.00

## Onboarding: how far do installs get?

- Finished onboarding: 77% (20 of 26)

Discovered (highest share first):
  Saw the quick filter intro (fileExplorer.quickFilterIntroSeen): 38% (10 of 26)
  Saw the text editor hint* (behavior.textEditorHintSeen): 1 of 4 (too few to say)

Closed by hand:
  Closed the USB debugging hint (behavior.adbHintDismissed): 10% (1 of 10)

Each install counts once, at its latest heartbeat; a flag off its default means the install saw, did, or
closed that thing. Each share is over the installs whose build has the flag. Under 5 such installs a row
shows the count only. A * marks a hint that only shows where macOS has the feature behind it, so its share
also counts installs that could never see it.

## Feature usage: which features do people use?

- Window: 2026-05-04 – 2026-08-01, complete days
- Active installs (beat at least once in the window): 40

An install counts toward a feature once it fired any of that feature's events in the window; the share is of active installs. "No data yet" means no release has sent the event yet, so it isn't a zero. The change is in percentage points against the window before, and a dash when there is no honest comparison (the feature's events weren't stored yet then).

Per feature (id from feature-status.json, most-used first):
  archives (Archives): 30 installs (75%, +25 pp vs. previous 90 days), 4,321 events; pane_navigated 4,321 (30 installs)
  ask-cmdr (Ask Cmdr): 3 installs (7.5%, – vs. previous 90 days), 12 events; answered 10 (3 installs), failed 2 (1 install)
  git-browser (Git browser): 1 install (2.5%, −2.5 pp vs. previous 90 days), 1 event
  tabs (Tabs): 0 installs (0%, ±0 pp vs. previous 90 days), 0 events
  multi-rename (Multi-rename tool): no data yet

Counted from:
  tabs: tab_opened
  ask-cmdr: ask_cmdr_turn
  multi-rename: multi_rename_applied
  archives: pane_navigated[volume_kind=archive], archive_edit_completed
  git-browser: pane_navigated[volume_kind=git-portal]

## Settings adoption: what do people actually turn on?

- Installs we can read: 26
- Not counted: 4 on a version older than the settings history goes back
- Drive indexing on: 92%
- Image search on: 20%
- AI switched on: 23%

Each install counts once, at its latest heartbeat. A setting is scored only against installs whose build actually had it, so a young setting shows a smaller total than an old one. Only preferences show here; remembered UI and onboarding state are in the Onboarding section.

Settings people change (most-moved first; the other 1 setting sits at its default everywhere):
  ai.provider: default off, 26 installs, 77% on default, most common change: cloud (6)
  mediaIndex.enabled: default off, 10 installs, 80% on default, most common change: on (2)
  ui.density: default compact (moved in v1.10.0), 26 installs, 81% on default, most common change: roomy (5)
  indexing.enabled: default on, 26 installs, 92% on default, most common change: off (2)

## Feedback & errors: what are users telling us?

- Feedback messages: 3
- Awaiting reply (a reply-to email, not marked answered, however old): 2
- Error reports: 3
- Error reports since v1.10.0 came out: 2

Error reports by kind:
  auto: 2
  user: 1

Error reports per 100 installs, by version (2026-05-04 – 2026-08-01, complete days; an install that upgraded counts under both; under 5 installs, no rate):
  1.10.0: 2 reports, 45 installs, 4.4 per 100
  1.2.0: 1 report, 50 installs, 2 per 100
  1.1.0: 1 report, 3 installs, too few installs for a rate

Error reports by day:
  2026-07-31: 1
  2026-08-01: 2

Recent feedback, and every message awaiting a reply:
  2026-08-01 (v1.10.0) [reply-to: someone@example.com]: Love it, especially the preview pane.
  2026-07-31 (v1.2.0) [logs: ERR-AB7PV]: ${'x'.repeat(280)}
  2026-07-30 (v1.2.0) [reply-to: answered@example.com, answered 2026-07-30]: Already got a reply.
  2026-04-01 (v1.1.0) [reply-to: waiting@example.com]: Still waiting on this one.`

const allFailedReport = `# Cmdr analytics report (7d)

Generated: 2026-08-02T03:04:05.000Z

## Daily funnel: the last 30 days, one row per UTC day

Couldn't load: Funnel: timed out after 20s

## Awareness: how many people see Cmdr content?

Couldn't load: Umami: not configured (missing env vars)

## Interest: how many engage with the product page?

Couldn't load: Umami: not configured (missing env vars); PostHog: not configured (missing env vars)

## Download: how many actually download?

Couldn't load: Cloudflare: timed out after 20s; GitHub: offline

## Active use: how many run the app?

Yesterday, 2026-08-01 (UTC), the last complete day. Today is partial and in no number here.
- Couldn't load the heartbeat: Active installs: timed out after 20s
- D7 retention: couldn't load (Funnel: timed out after 20s)

## Release rollout: how far does the newest release spread?

Couldn't load: Active installs: timed out after 20s

## Money: who pays?

Couldn't load: Paddle: not configured (missing env vars)

## Onboarding: how far do installs get?

Couldn't load: Settings adoption: timed out after 20s

## Feature usage: which features do people use?

Couldn't load: Feature usage: timed out after 20s

## Settings adoption: what do people actually turn on?

Couldn't load: Settings adoption: timed out after 20s

## Feedback & errors: what are users telling us?

Couldn't load: Feedback & errors: timed out after 20s`

const emptyReport = `# Cmdr analytics report (30d)

Generated: 2026-08-02T03:04:05.000Z

## Daily funnel: the last 30 days, one row per UTC day

day | visitors | dl clicks | server dls | installs | D7 | signups | purchases

## Awareness: how many people see Cmdr content?

- Total page views: 0
- veszelovszki.com views: 0
- getcmdr.com views: 0
- getprvw.com views: 0
- veszelovszki.com visitors: 0
- getcmdr.com visitors: 0
- getprvw.com visitors: 0

GitHub stars: 0 total

## Interest: how many engage with the product page?

- getcmdr.com page views: 0
- Unique visitors: 0
- Bounce rate: N/A

## Download: how many actually download?

(New installs = DMG downloads via getcmdr.com, deduplicated to distinct people per day by daily-hashed IP, bot/link-preview hits dropped by user agent, in-app auto-updates excluded. Raw = every download request.)
- New installs (deduped): 0
- Download requests (raw): 0
- Downloads (GitHub, all-time): 0

## Active use: how many run the app?

Yesterday, 2026-08-01 (UTC), the last complete day. Today is partial and in no number here.
- Weekly active: – (– vs. 30 days earlier)
- Monthly active: – (– vs. 30 days earlier)
- Stickiness: – (– vs. previous 30 days)
- Hours per active day: – (– vs. previous 30 days)
- D7 retention: – (– vs. previous 30 cohorts)

Counts installs with usage stats on; an install that opted out sends nothing. Weekly and monthly
active count installs with a beat in the 7 and 28 days ending yesterday, against the same count 30 days
earlier. Stickiness is daily active over monthly active, and hours per active day is all app hours over
all daily actives, each pooled over the last 30 complete days against the 30 before. A beat from a build
too old to report its runtime counts as an hour. D7 pools the newest 30 cohorts old enough to have one.
The last week can still hold a few synthetic installs the weekly sweep removes later.
"Checked for updates" is context, not a ceiling: distinct addresses that checked for updates, a separate
consent installs with analytics off still ride. A shared connection counts once, a changing address
counts more than once, and anyone with automatic update checks off never appears.

## Release rollout: how far does the newest release spread?

- No release to track yet.

## Money: who pays?

- Revenue, last 30 complete days: 0
- All-time revenue: 0
- Paid licenses (first purchases through yesterday): 0
- Paying share of monthly active: – (– vs. 30 days earlier)
- License activations: 0 (– active devices)
- Saw the upgrade nudge: not available yet

Revenue counts complete days and stays per currency: amounts in two currencies are never added. A
transaction counts in its payout currency when Paddle reports one, else its checkout currency. It's gross
of refunds: a refunded purchase stays completed in Paddle. Purchases are first purchases (checkout or API);
renewals, card updates, and plan changes aren't. Paying share is a trend signal, not a conversion rate:
refunded purchases still count, one license can cover several Macs, and a licensed install with analytics
off is paid but not in monthly active.

## Onboarding: how far do installs get?

- Finished onboarding: not available yet

Each install counts once, at its latest heartbeat; a flag off its default means the install saw, did, or
closed that thing. Each share is over the installs whose build has the flag. Under 5 such installs a row
shows the count only. A * marks a hint that only shows where macOS has the feature behind it, so its share
also counts installs that could never see it.

## Feature usage: which features do people use?

- Window: since 2026-07-03, today included (the api-server predates complete-day windows)
- Active installs (beat at least once in the window): 0

An install counts toward a feature once it fired any of that feature's events in the window; the share is of active installs. "No data yet" means no release has sent the event yet, so it isn't a zero. The change is in percentage points against the window before, and a dash when there is no honest comparison (the feature's events weren't stored yet then).

Per feature (id from feature-status.json, most-used first):
  search (Search): 0 installs (–, – vs. previous 30 days), 0 events

Counted from:
  search: search_used

## Settings adoption: what do people actually turn on?

- No installs to read yet (settings adoption fills as beta testers run a build that reports a config).

## Feedback & errors: what are users telling us?

- Feedback messages: 0
- Awaiting reply (a reply-to email, not marked answered, however old): 0
- Error reports: 0`

// The doubled blank line under Interest and Download is the current shape: the section heading already
// emits one, and the first sub-block that survives adds its own separator.
const mixedReport = `# Cmdr analytics report (7d)

Generated: 2026-08-02T03:04:05.000Z

## Daily funnel: the last 30 days, one row per UTC day

Couldn't load: Funnel: offline

## Awareness: how many people see Cmdr content?

Couldn't load: Umami: offline

## Interest: how many engage with the product page?


Daily page views (PostHog):
  2026-07-31: 400
  2026-08-01: 500

## Download: how many actually download?


GitHub releases (all-time):
  v1.10.0: 800 downloads (published 2026-08-01)
  v1.2.0: 434 downloads (published 2026-07-01)

## Active use: how many run the app?

Yesterday, 2026-08-01 (UTC), the last complete day. Today is partial and in no number here.
- Not available yet: the api-server doesn't answer /admin/active-installs yet.
- D7 retention: couldn't load (Funnel: offline)

## Release rollout: how far does the newest release spread?

- Not available yet: the api-server doesn't answer /admin/active-installs yet.

## Money: who pays?

- Revenue, last 7 complete days: €89.00 (– vs. previous 7 days), $39.00 (– vs. previous 7 days)
- All-time revenue: €89.00, $78.00
- Paid licenses (first purchases through yesterday): 2
- Paying share of monthly active: – (– vs. 7 days earlier)
- License activations: couldn't load (License server: offline)
- Saw the upgrade nudge: couldn't load (Settings adoption: offline)

Revenue counts complete days and stays per currency: amounts in two currencies are never added. A
transaction counts in its payout currency when Paddle reports one, else its checkout currency. It's gross
of refunds: a refunded purchase stays completed in Paddle. Purchases are first purchases (checkout or API);
renewals, card updates, and plan changes aren't. Paying share is a trend signal, not a conversion rate:
refunded purchases still count, one license can cover several Macs, and a licensed install with analytics
off is paid but not in monthly active.

Purchases, last 7 days (first purchases, newest first):
  2026-08-02 01:30 UTC (today): $39.00
  2026-08-01 10:00 UTC: $39.00

## Onboarding: how far do installs get?

Couldn't load: Settings adoption: offline

## Feature usage: which features do people use?

Couldn't load: Feature usage: offline

## Settings adoption: what do people actually turn on?

Couldn't load: Settings adoption: offline

## Feedback & errors: what are users telling us?

- Feedback messages: 0
- Awaiting reply (a reply-to email, not marked answered, however old): 0
- Error reports: 3
- Error reports since v1.10.0 came out: 2

Error reports by kind:
  auto: 2
  user: 1

Error reports by version:
  1.10.0: 2
  1.2.0: 1

Error reports by day:
  2026-07-31: 1
  2026-08-01: 2`

describe('formatReport', () => {
  // The GitHub-stars block measures its 7- and 30-day windows against the wall clock.
  beforeAll(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-08-02T00:00:00Z'))
  })
  afterAll(() => {
    vi.useRealTimers()
  })

  it('renders every section when every source loaded', () => {
    expect(formatReport(richData)).toBe(richReport)
  })

  it('gives each section its own "Couldn\'t load" line when every source is down', () => {
    expect(formatReport(allFailedData)).toBe(allFailedReport)
  })

  it('skips the breakdown blocks when the sources loaded but hold nothing', () => {
    expect(formatReport(emptyData)).toBe(emptyReport)
  })

  it('keeps the surviving half of a section when one of its two sources is down', () => {
    expect(formatReport(mixedData)).toBe(mixedReport)
  })
})
