# Updater module

Cmdr's own updater on every OS: the check, the managed policy, minisign against both updater keys, and the advisory
list. Only the last step differs: macOS syncs files *into* the running `.app`; Windows (NSIS) and Linux (AppImage) stage
the verified installer and hand it to the Tauri updater plugin as Restart quits Cmdr.

## File map

- `mod.rs` (the five Tauri commands, `UpdateState`, typed outcomes; tests in `tests.rs`), `install_kind.rs` (which
  install this is and why some never update), `handoff.rs` (staging the installer, the exit hand-off), `manifest.rs`
  (re-exports `crates/cmdr-update-manifest/CLAUDE.md`, the reader the release gate also runs), `signature.rs` (both
  keys, the signed version), `advisories.rs`, `security_events.rs`.
- `platform/`: the per-OS last step. `macos/` holds `installer.rs` (extraction, the sync, `running_bundle()`),
  `bundle_location.rs`, and `local_network_refresh.rs`; `plugin.rs` is the Windows and Linux hand-off. The code
  signature check is `crates/cmdr-update-trust/CLAUDE.md`.

## Must-knows

- **Sync into the bundle, with per-file temp + `rename()`**, never in-place `fs::copy`: the code-signing cache keys on
  inode, so a same-inode copy dies with `SIGKILL (Code Signature Invalid)`. ❌ Not because replacing the `.app` loses
  FDA: it doesn't (`DETAILS.md` § Key decisions).
- **Only a real user's production install may check** (`skip_reason`). ❌ Never keep a second copy of
  `crate::prod_instance::NON_PROD_ENV_VARS` here. `DETAILS.md` § Who may check.
- **❌ Never escalate to admin**; a `.deb`/`.rpm` never checks rather than `pkexec`. `DETAILS.md` § A bundle that can't
  be written.
- **Minisign (main or backup key) on every artifact**, plus Cmdr's Developer ID on a macOS bundle
  (`vet_staged_bundle`; ❌ nothing may skip it). The backup pubkey lives only in `updater-backup.pub`; ❌ never copy it.
- **Something signed names the version, never the manifest alone**: macOS the extracted `Info.plist`, Windows and Linux
  the signature's `version:` (`bind_signed_version`). ❌ Never install around either: they stop a rollback.
- **The backend decides, under the organization's policy.** ❌ Never let the frontend name a URL or version.
  `DETAILS.md` § Managed policy.
- **Security notices come only from a signed list, ❌ never `latest.json` itself**, and a broken list ❌ never blocks the
  update.
- **Off macOS the plugin only runs the installer**, last in `RunEvent::Exit`, armed by `quit::relaunch` so "Keep
  working" disarms it. `DETAILS.md` § Windows and Linux.
- **Every remote body is bounded** in time and size (manifest 30 s and 1 MiB, advisory list 15 s and 256 KiB, payload
  30 s per read and 1 GiB). Bad optional data degrades with a warn and ❌ never blocks an update; bad required data is
  a typed `BadResponse`, ❌ never "up to date". The per-field audit: `DETAILS.md` § Remote data audit.

Sync order, signatures, the hand-off, who may check, the policy, and Local Network after an update: `DETAILS.md`. Read
it before any non-trivial work here: editing, planning, reorganizing, or advising.
