# Analytics dashboard

Private SvelteKit dashboard consolidating Cmdr business metrics, organized by acquisition stage. Deployed to Cloudflare
Pages at `analdash.getcmdr.com`, behind Cloudflare Access plus an in-app JWT gate (`src/hooks.server.ts`).

## Pages

Four routes share `routes/+layout.svelte` (header: nav, dates, range picker). The picker writes `?range=` (7d / 30d /
90d), hidden on `/licenses` and `/links`; on `/`, ‹ › step back via `?end=` (`DETAILS.md` § Selection state).

- `/` (Acquisition, `routes/+page.svelte`): funnel trend charts (`?country=` scoped), country table, more sections.
- `/product` (`routes/product/`): headline tiles, usage, rollout, D7/D28 retention, money, onboarding, features,
  settings, inbox. `/api/report` mirrors it through the same helpers (`DETAILS.md` § Product page).
- `/licenses` (`routes/licenses/`): every license we've issued, from `/admin/licenses`, with an editable note each.
- `/links` (`routes/links/`): CRUD for the `?r=` short codes, proxied to the api-server admin endpoints.

Sections live in `src/lib/components/sections/`, sources in `src/lib/server/sources/` (one per external API), loaders
and per-page composers in `src/lib/server/fetch-all.ts`.

## Stack

SvelteKit + `@sveltejs/adapter-cloudflare`, Tailwind v4 (CSS-first in `src/app.css`, dark mode only), uPlot for charts.
All API keys stay server-side, behind `+server.ts` / `+page.server.ts`.

## Checks

`pnpm check dashboard` runs all nine checks; never raw `pnpm lint`.

## Must-knows

- **Auth is the app's job, not just Cloudflare's.** Access binds to a hostname, and the `pages.dev` alias reaches the
  same deployment around it, so `src/hooks.server.ts` verifies the Access JWT on every server-handled request; every
  failure path must fail closed. ❌ Never gate on the spoofable `cf-access-authenticated-user-email` header. People and
  service tokens both pass, so `locals.identity` is a `user | service` union. Why and how: `DETAILS.md` § Auth.
- **Write `{#if source.ok}` with the error state in `{:else}`, never the inverse.** Inverted, ESLint loses the narrowing
  and every `source.data` read looks like missing prop types; flip the branch, never add an `any`.
- **Don't import `#lib/server/*` as a runtime value into browser-bundled code** (components, route `.svelte` files);
  type-only imports are fine. It trips `vite-plugin-sveltekit-guard` at BUILD time and **`svelte-check` does NOT catch
  it**, so run `pnpm build` after touching imports across the boundary. `DETAILS.md` lists the client-shared values.
- **Every data source must go behind the 20s `withTimeout` cap in `fetch-all.ts`.** Workers `fetch` has no timeout, so
  one hung upstream stalls the whole `Promise.all` until Cloudflare's 524 at 100s. Sources run in parallel, return
  `SourceResult<T>`, and cache via `cache.ts`. Details: `DETAILS.md`.
- **The admin token (`LICENSE_SERVER_ADMIN_TOKEN`) stays server-side**, resolved in `+page.server.ts` only; the browser
  gets rows and a load-error string.
- **❌ Nothing on `/licenses` may say a Paddle subscription is live.** `active` on a `paddle` row means we fulfilled
  that purchase; only `/validate` knows whether it still runs. Labels live in `#lib/licenses.ts`; the vocabulary is
  owned by `apps/api-server/src/licensing/DETAILS.md` § The licenses listing.
- **A `null` cell renders as a dash (`–`), never a 0**: "couldn't get this" and "this was zero" differ.
- **Every stat on both pages skips today and pools rates as ratios of sums** (`#lib/acquisition-stats.ts`,
  `#lib/product-stats.ts`); a missing worker field reads "Not available yet". Country chips update the URL shallowly, so
  ❌ never rebuild a URL from `page.url` on `/`; read `window.location`.
- **(i) popovers: a gist line, bullets, then `Source:`** (`InfoPopover.svelte`).
- **One color means one property across the whole dashboard**, and a version's color depends only on its string
  (`versionColor`), never its rank. Palette: `DETAILS.md` § Color coding.
- **❌ Never auto-scale `#lib/count-color-scale.ts`**: purple always means `COUNT_SCALE_PURPLE_AT`, raised only by hand.
  `DETAILS.md` § "The country map and table".

Local dev: `pnpm dev:dashboard` serves on port 4830, reading `.env` (copy `.env.example`; escape a literal `$` as `\$`).

Everything else (structure, data, sources, auth, env, QA, deployment): `DETAILS.md`.
