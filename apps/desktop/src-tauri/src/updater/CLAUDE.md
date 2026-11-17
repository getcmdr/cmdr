# Updater module

Cmdr's own updater, on every OS: the check, the managed policy, minisign against both updater keys, and the advisory
list are ours everywhere. Only the last step differs: macOS syncs files *into* the running `.app` bundle; Windows (NSIS)
and Linux (AppImage) stage the verified installer and hand it to the Tauri updater plugin as Restart quits Cmdr.

## File map

- `mod.rs`: the five Tauri commands, shared `UpdateState`, and the typed outcomes and errors. Tests in `tests.rs`.
- `install_kind.rs`: which install this is (`InstallKind`), its manifest keys, and why some never update.
- `handoff.rs`: staging the verified installer (Windows, Linux) and handing it off at exit.
- `platform/`: the seam (`docs/guides/platform-seams.md`): the install kind, the write check, the bundle install, the
  hand-off. `macos/` holds `installer.rs` (extraction, the sync, `running_bundle()` for `dock/`, `cli/`,
  `install_location`), `bundle_location.rs` (`BundleWriteBlocker`, typed in `write_blocker.rs`), and
  `local_network_refresh.rs` (#1019); `plugin.rs` is the Windows and Linux plugin hand-off. The code-signature check
  is `crates/cmdr-update-trust/CLAUDE.md`.
- `manifest.rs`: parses `latest.json`, compares versions, picks the platform entry.
- `signature.rs`: minisign verification against the main and backup updater keys, and the signed version.
- `advisories.rs`: the `SecurityNotice`, from the staged bundle (macOS) or the release's signed list (elsewhere).
- `security_events.rs`: the security event log's updater events (`../security_log/CLAUDE.md`).

## Must-knows

- **Sync into the bundle, never replace the `.app` directory**, because the per-file atomic rename below needs a bundle
  to sync into. ❌ NOT because replacing loses the FDA grant: it doesn't (measured; `DETAILS.md`).
- **Per-file writes use temp + `rename()`, never in-place `fs::copy`.** The code-signing cache keys on inode, so a
  same-inode copy dies with `SIGKILL (Code Signature Invalid)`.
- **Only a real user's production install may check** (`skip_reason`: an `InstallKind`, no
  `crate::prod_instance::NON_PROD_ENV_VARS`). ❌ Never keep a second copy of that list here. `DETAILS.md` § Who may check.
- **❌ Never escalate to admin** (macOS App Management refuses it anyway; a `.deb`/`.rpm` never checks rather than
  `pkexec`). `DETAILS.md` § A bundle that can't be written.
- **Minisign (main or backup key) on every artifact.** macOS adds Cmdr's Developer ID on the bundle
  (`vet_staged_bundle`, `UntrustedCodeSignature`; ❌ nothing may skip it); Windows has no Authenticode check yet. The
  backup pubkey lives only in `updater-backup.pub`; ❌ never copy it. `DETAILS.md` § Code signature, § Two updater keys.
- **Something signed names the version, never the manifest alone.** macOS: the extracted `Info.plist`
  (`installer::refuse_unless_newer`). Windows and Linux: the signature's `version:` (`bind_signed_version`, signed with
  `--app-version`). ❌ Never install around either: they stop an older signed release rolling an install back.
- **The organization's policy gates every step, and the backend decides.** `download_update` takes no URL and fetches
  only what the last check offered; `install_update` re-asks `update_to` on a fresh read. ❌ Never let the frontend
  name a URL or version. `DETAILS.md` § Managed policy.
- **Security notices come only from a signed list, ❌ never `latest.json` itself.** A broken or missing list ❌ never
  blocks the update. `DETAILS.md` § Security notices.
- **Off macOS the plugin only runs the installer**: no `updater:` capability, no check of its own. The hand-off runs
  last in `RunEvent::Exit` (Windows never returns from it), armed by `quit::relaunch` so "Keep working" disarms it.
  `DETAILS.md` § Windows and Linux.
- **An update leaves Local Network on the old binary's UUID** (macOS); the next launch re-announces it.
  `DETAILS.md` § Local Network after an update.
- **Manifest fetch is bounded** (10 s connect, 30 s overall); download and install are untimed. All ride
  `server_request::send` (`Egress::UpdateCheck`, `UpdateDownload`).

Sync order, signatures, the hand-off, who may check, the policy, and Local Network: `DETAILS.md`. Read it before any
non-trivial work here.
