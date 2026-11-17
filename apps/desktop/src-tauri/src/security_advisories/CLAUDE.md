# Security advisories

The committed list of fixed vulnerabilities users should know about (`resources/security-advisories.json`), and the
questions asked of it: parse, "fixed between these two versions", and "may a notice link here". Cross-platform, since
the What's new popup reads it everywhere; the updater reads a STAGED bundle's copy (macOS) or the release's signed
copy (Windows, Linux) through `updater/advisories.rs`.

## Module map

- `mod.rs`: the `Severity` / `Advisory` / `SecurityAdvisory` types, `parse`, `fixed_between`, the link allowlist
  (`is_allowed_advisory_url`, `vetted_link`), and this build's embedded copy (`embedded`, `embedded_fixed_since`).
- `tests.rs`: the filter, the allowlist's refusals, and a canary over the committed file.
- IPC: `get_security_advisories` in `../commands/whats_new.rs`. The validator check: `desktop-security-advisories`.

## Must-knows

- **Lenient by design: a list that doesn't parse is "no advisory" plus a warn, ❌ never an error.** The update carrying
  it is the security fix itself, so nothing here may block, fail, or delay an install. The strictness lives in the
  `desktop-security-advisories` check instead, which fails on anything this parser would refuse or quietly drop.
- **Change the schema or the allowlist in three places at once**: `mod.rs`, the Go check
  (`scripts/check/checks/desktop-security-advisories.go`), and the release guide's advisory step. The canary test only
  vets the committed file against the Rust side.
- **The allowlist is exactly two `https` places** (GitHub advisories under either repo name, and the changelog), and the URL must already be canonical (no `..`, no default port, no
  userinfo). ❌ Don't widen it for convenience: a notice is something people act on.
- **Cumulative, never pruned.** An install jumping from 0.50 to 0.55 learns about a fix shipped in 0.52 only because
  0.55's bundle still carries that entry.

What gets an entry, the trust argument, and the window rules: `DETAILS.md`.
