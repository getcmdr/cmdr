# Analytics dashboard

Private SvelteKit dashboard at `https://analdash.getcmdr.com`. Consolidates metrics from Umami, Cloudflare, Paddle,
GitHub, PostHog, and the API server, organized by acquisition stage across four pages.

- **Code**: `apps/analytics-dashboard/`
- **Architecture docs**: `apps/analytics-dashboard/CLAUDE.md`

❌ **The report below covers the metric pages only, never the license ledger.** Which licenses exist, who holds the
codes, and which purchases are stuck lives on the `/licenses` page (`apps/analytics-dashboard/DETAILS.md` § The licenses
list), straight from `GET /admin/licenses`. A report with no license section is not evidence that no licenses were
issued.

## Agent-readable report

The `/api/report` endpoint returns a plain-text report with all dashboard data, including breakdowns that are only
visible via hover tooltips on the visual dashboard. This is the recommended way for agents to read analytics data.

```
GET /api/report?range=7d
```

Accepts `range` query param: `7d`, `30d` (default), `90d`.

❌ **"New installs" means two different things in this report; read the right one.** The **Daily funnel**'s `installs`
column counts install ids whose first-ever heartbeat landed that day, so it's people who actually launched Cmdr, and
it's the number to quote for adoption (~110/month as of 2026-09-08). The **Download** section's "New installs (deduped)"
is deduplicated download _requests_ (1,855 over the same 30 days), inflated by mirrors and scrapers the user-agent
filter misses. Same words, ~10x apart. Background: `apps/analytics-dashboard/DETAILS.md`.

The report includes:

- **Awareness**: page views, visitors, referrers (with percentages and deltas vs prior period)
- **Interest**: product page views, bounce rate, download button clicks, top pages, visitors by country
- **Download**: "new installs" (deduped same-day-distinct, bots dropped, auto-updates excluded) and raw request totals;
  by source (website/Homebrew/other) and per day; totals by version, architecture, and country; daily breakdown;
  cross-breakdowns (country × architecture, country × version, daily × version)
- **Daily funnel**: the last 30 UTC days, one row each (visitors, clicks, server downloads, installs, D7, signups,
  purchases), plus channels, referrers, and User-Agent families
- The rest follows the `/product` page, from the same helpers, so the two never disagree. Every Product number reads
  complete days: today (UTC) is partial, in no stat, and shows only as a line marked "today, partial".
- **Active use**: yesterday's weekly and monthly active, stickiness, hours per active day, and D7, each with its change
  against the range's N days; then one line per day with active and new installs, app hours, and how many addresses
  checked for updates (context for what the heartbeat misses, never a ceiling)
- **Release rollout**: the share of yesterday's active installs on the newest release and how many days after its
  release; installs by version per day
- **Money**: revenue over the range's complete days per currency (never added across currencies) with its change,
  all-time revenue, paid licenses (first purchases), paying share of monthly active, license activations, who saw the
  upgrade nudge, and the range's purchases
- **Onboarding**: finished onboarding, then the hints discovered, the one that means a snag, and the ones closed by
  hand, each as a share of the installs whose build has that flag
- **Feature usage**: per `feature-status.json` feature, distinct installs (and their share of the window's active
  installs, with its change in points against the window before) and event count, with a breakdown (by event, or an Ask
  Cmdr `outcome`, a search `mode`); a feature whose events no release has sent yet reads "no data yet", not 0%. The
  section ends with what each row counts. This is the per-release feature-status review's input. The mapping:
  `apps/api-server/src/admin/DETAILS.md` § Feature usage.
- **Settings adoption**: what installs run with, resolved against each version's defaults; preferences only, the 15
  most-moved
- **Feedback & errors**: messages awaiting a reply (however old), error reports since the newest release, error reports
  per 100 installs by version, and the recent feedback plus every message still owed a reply

A section whose api-server endpoint isn't deployed yet reads "Not available yet", and one whose source failed reads
"Couldn't load", each on its own, never as zeros.

### Fetching the report

The dashboard is behind Cloudflare Access. Authenticate with the service token stored in the sops secrets store (fetch
with `secret NAME`):

```bash
CF_ID=$(secret CF_ACCESS_CLIENT_ID_EXPIRES_2027_03_22)
CF_SECRET=$(secret CF_ACCESS_CLIENT_SECRET_EXPIRES_2027_03_22)
curl -s "https://analdash.getcmdr.com/api/report?range=7d" \
  -H "CF-Access-Client-Id: ${CF_ID}" \
  -H "CF-Access-Client-Secret: ${CF_SECRET}"
```

The service token expires 2027-03-22. See `cloudflare.md` for token management.

## Running locally

1. Copy `apps/analytics-dashboard/.env.example` to `.env` and fill in real values.
2. `pnpm dev:dashboard` from the repo root.
3. Dashboard at `http://localhost:4830`, report at `http://localhost:4830/api/report` (no auth needed locally).
