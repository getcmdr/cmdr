# Remark42 (comments)

Self-hosted commenting engine. Runs as a Docker container alongside the website on the Hetzner VPS. The version is
pinned in `apps/website/docker-compose.yml`; don't restate it here, it goes stale.

- **Host URL**: https://comments.getcmdr.com
- **Container name**: `remark42`
- **Docker Compose**: `apps/website/docker-compose.yml`

**Gotcha**: a second Remark42 container, `remark42-blog`, runs on the same box for David's personal blog
(`comments.veszelovszki.com`, `SITE=vdavid-blog`, its own OAuth clients). It's deployed from the `infra` repo at
`hetzner/services/remark42-blog/` and has nothing to do with Cmdr. Match the container name exactly before restarting or
reading env.

## Sites served

| Site ID   | Website          | Remark42 host                          |
| --------- | ---------------- | -------------------------------------- |
| `getcmdr` | getcmdr.com blog | `comments.getcmdr.com` (this instance) |

To add a site, append its ID to the `SITE=` env var (comma-separated) and restart the container.

## Infrastructure

- **DNS**: A record `comments.getcmdr.com` → `37.27.245.171` (Cloudflare, NOT proxied)
- **Caddy route**: `comments.getcmdr.com { reverse_proxy remark42:8080 }` (in `hetzner-server` repo)
- **Data**: Docker volume `remark42-data` mounted at `/srv/var` inside the container

## Required secrets

Stored on the server at `apps/website/.env`:

- **`REMARK42_SECRET`**: Signing secret. Generate with `openssl rand -hex 32`
- **`AUTH_GITHUB_CID`**: GitHub OAuth app client ID
- **`AUTH_GITHUB_CSEC`**: GitHub OAuth app client secret
- **`AUTH_GOOGLE_CID`**: Google OAuth app client ID
- **`AUTH_GOOGLE_CSEC`**: Google OAuth app client secret

## OAuth callback URLs

- **GitHub**: `https://comments.getcmdr.com/auth/github/callback`
- **Google**: `https://comments.getcmdr.com/auth/google/callback`

These must match exactly in the OAuth app settings on GitHub / Google Cloud Console.

## Admin and data removal

- **Admins**: `docker-compose.yml` sets `ADMIN_SHARED_ID` to David's GitHub and Google ids and
  `ADMIN_SHARED_EMAIL=legal@getcmdr.com` (the address the privacy policy gives for deletion requests). Remark42 ids are
  unsalted hashes (GitHub: `github_` + sha1 of the login), so they're the same as his ids on
  `comments.veszelovszki.com`. Unset, `admin_email` falls back to `admin@comments.getcmdr.com`, which doesn't exist.
- **How a removal request works**: a signed-in commenter clicks "Request my data removal" in their profile. Remark42
  mints a link (`/web/deleteme.html?site_id=getcmdr&token=…`, token valid three months, not usable for login), opens a
  prefilled `mailto:` to `admin_email` with the user id and link, and signs them out. The commenter still has to send
  that email. David then signs in to the comments on a getcmdr.com blog post (the session must belong to site `getcmdr`)
  and opens the link: it hard-deletes the user's comments, stored details, and avatar, and shows the result as JSON.
  Opened without an admin session, the page says "You are not logged in" and does nothing.
- **Check it**: `curl -s 'https://comments.getcmdr.com/api/v1/config?site=getcmdr'` shows `admins` and `admin_email`.

## Client IP handling

Vote dedup and rate limiting key off the client IP, which reaches Remark42 in `X-Forwarded-For`, so both ends are
configured to keep that header trustworthy:

- **Caddy** sets `header_up X-Forwarded-For {remote_host}` on the `comments.getcmdr.com` route (`infra` repo,
  `hetzner/services/caddy/Caddyfile`). Caddy's default **appends** to a client-supplied header rather than replacing it,
  which leaves the spoofed value in place.
- **Remark42** sets `TRUSTED_PROXY=172.18.0.0/16` (the `proxy-net` subnet), so it only honors the header from Caddy.
  Unset, it trusts any client and logs a startup warning.

**Gotcha**: trust the network CIDR, not Caddy's container IP. Docker bridge IPs are pool-assigned and shift when a
container is recreated, and a stale pin doesn't fail loudly: Remark42 falls back to the socket peer, which is Caddy, so
every commenter collapses into a single IP and shares one rate limit and vote identity. `comments.getcmdr.com` is
deliberately not Cloudflare-proxied, so `{remote_host}` at Caddy is the real client.

## OAuth clients

- **Google**: client named `Cmdr Remark42` in Google Cloud project `cmdr-490722` (Cmdr's project; the consent screen is
  "Cmdr comments", In production). Manage it under Google Auth Platform → Clients. Its secret lives only in the server
  `.env`; the console can't show it again, so replace it by rotating. Google keys users on their account's `sub`, the
  same for every client, so switching clients keeps commenters' identity. Project map:
  `~/projects-git/vdavid/infra/gcp/CLAUDE.md`.
- **GitHub**: OAuth app `Ov23lihhp2lm7WROt4VX`.

**Guardrail**: Google auto-deletes an OAuth client after six months with no token request and no settings change, which
would silently break the Google sign-in button on the blog (GitHub login would keep working, so the outage is easy to
miss). Traffic here is low enough that the client can idle past the limit on its own. Either sign in with Google on the
blog occasionally, or rename the client / rotate its secret in the console; both count as activity and reset the clock.
Google emails a warning first, and a deleted client is restorable for ~30 days under Clients → "Restore deleted OAuth
clients". (Verified in the Google Cloud console, 2026-08-27.)

Google supports two live secrets per client, so rotate without downtime: add a secret, update `AUTH_GOOGLE_CSEC` in the
server `.env`, `docker compose up -d remark42`, confirm a real Google sign-in works, then delete the old secret.

## Common operations

```bash
# Start the container
docker compose up -d remark42

# Health check (expect "pong")
curl -s https://comments.getcmdr.com/ping

# View logs
docker logs remark42

# Add a new site: append to the SITE= env var (comma-separated), then restart
docker compose up -d remark42
```

## Related files

- `apps/website/src/components/Remark42Comments.astro`: Astro component that embeds the comment widget
- `../guides/deploying-remark42.md`: Step-by-step deployment guide
