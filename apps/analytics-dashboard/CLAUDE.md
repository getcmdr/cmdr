# Analytics dashboard

Private SvelteKit dashboard of Cmdr business metrics on Cloudflare Pages (`analdash.getcmdr.com`), behind Cloudflare
Access plus an in-app JWT gate (`src/hooks.server.ts`).

## Module map

- Routes: `/` (Acquisition), `/product`, `/licenses`, `/links`; `/api/report` mirrors both data pages as text.
- `src/lib/components/sections/` (one per section), `src/lib/server/sources/` (one per external API),
  `src/lib/server/fetch-all.ts` (loaders and per-page composers).
- Stack, routes, sources, auth, env, QA, deployment: `DETAILS.md`. Checks: `pnpm check dashboard`, never raw
  `pnpm lint`. Local dev: `pnpm dev:dashboard` (port 4830, reads `.env`).

## Must-knows

- **Auth is the app's job too.** The `pages.dev` alias bypasses Access, so `hooks.server.ts` verifies the Access JWT on
  every request and fails closed. ❌ Never gate on the spoofable `cf-access-authenticated-user-email` header.
  `DETAILS.md` § Auth.
- **Write `{#if source.ok}` with the error in `{:else}`**, never the inverse: ESLint loses the narrowing.
- **❌ No runtime `#lib/server/*` import in browser code** (type-only is fine). `svelte-check` misses it; `pnpm build`
  catches it.
- **Every source goes behind the 20s `withTimeout` in `fetch-all.ts`**: Workers `fetch` has no timeout, and one hung
  upstream stalls the page until a 524.
- **The admin token stays server-side** (`+page.server.ts` only).
- **❌ Nothing on `/licenses` may say a Paddle subscription is live.** `DETAILS.md` § The licenses list.
- **Null renders as `–`, never 0.** A missing worker field reads "Not available yet".
- **Every stat skips today and pools rates as ratios of sums** (`#lib/acquisition-stats.ts`, `#lib/product-stats.ts`).
- **On `/`, ❌ never rebuild a URL from `page.url`**: country chips update it shallowly; read `window.location`.
- **One color means one property everywhere**; a version's color follows its recency among the versions in view
  (`versionColors`). `DETAILS.md` § Color coding.
- **❌ Never auto-scale `#lib/count-color-scale.ts`.** `DETAILS.md` § "The country map and table".
