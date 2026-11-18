# Secrets

Generic key-value secret storage with pluggable backends. The trait stores opaque `&[u8]` / `Vec<u8>`; callers own
their serialization. Consumers: `network/keychain.rs` (SMB creds, `username\0password` under `smb://server/share` keys),
`ai/api_keys.rs` (cloud AI API keys, raw UTF-8 under `ai.apiKey.<provider>` keys), and `licensing/key_storage.rs` (the
license key and short code, JSON under `license.key`). `data_wipe/` lists (`keys`) and deletes them all.

## Module map

- **`mod.rs`**: `SecretStore` trait, `store()` global accessor, the dev/test overrides.
- **`platform/`** (seam): each OS's system store. `macos/keychain.rs` (Keychain via `security-framework`),
  `linux/keyring.rs` (Secret Service via `keyring-core` + `zbus-secret-service-keyring-store`), `windows/
  credential_manager.rs` (Credential Manager via `keyring-core` + `windows-native-keyring-store`), `service_name.rs`.
- **`encrypted_file.rs`**: Cocoon-encrypted (ChaCha20-Poly1305, `/etc/machine-id` key) fallback for prod Linux without
  Secret Service. A file this machine can't open is set aside on the next save (`DETAILS.md` § Linux migration notes).
- **`plain_file.rs`**: Plain JSON file (dev mode, all platforms).
- **`system_keychain_smb.rs`** (macOS only): reads SMB passwords *other* apps (Finder/macOS) saved in the login
  keychain, so an os-mount volume can borrow it and go green without retyping. See the guardrail below.

## Backend selection

`store()` picks once at first access: `CMDR_SECRET_STORE=file` env (set by `tauri-wrapper.ts` in dev) or
`CMDR_E2E_MODE=1` → `PlainFileStore`, honored only in debug and `playwright-e2e` builds (a release build logs a warning
and ignores both, so an env var can't downgrade a user's Keychain to plaintext); else `platform::system_store()`: macOS
→ Keychain; Linux → Keyring when an `is_available()` write-read-delete probe passes, else `EncryptedFileStore`; Windows
→ Credential Manager, no probe and no fallback. ❌ No OS falls back to `PlainFileStore`: it's only ever chosen on
purpose.

## Must-knows

- **Never call `system_keychain_smb::read_password` at startup**: it reads `kSecClassInternetPassword` data and triggers
  the macOS SecurityAgent consent dialog (one per share = popup storm, the FDA-popup lesson). It is NOT a `SecretStore`
  backend. Consume it only from user-initiated commands (`upgrade_to_smb_volume_using_saved_password` /
  `system_has_saved_smb_password`). `account_for_any` only probes attributes (no dialog); `read_password` reads data (a
  dialog; "Always Allow" adds Cmdr to the item ACL for silence after).
- **Linux's `keyring-core` needs a process-wide default store**: installed lazily via a `Once` in `ensure_default_store()`,
  called from every public `KeyringStore` method. Use `set_secret`/`get_secret` (binary) over `set_password`/
  `get_password` (forces UTF-8).
- **The macOS and Windows service name is instance-suffixed** (`platform/service_name.rs`): prod (env unset/empty)
  keeps `"Cmdr"`; any non-empty `CMDR_INSTANCE_ID` maps to `"Cmdr-<instance>"`. Belt-and-suspenders so a stray non-prod
  launch on a system store can't mix into prod credentials. Linux keeps plain `"Cmdr"`.
- **Windows targets are `<service>:<key>`, ❗ capped at 2,560 bytes a secret** (Credential Manager's blob limit; past it
  `set` answers `Other`). `DETAILS.md` § Windows.
- **File stores respect `CMDR_DATA_DIR`** so dev and prod credentials are isolated. Dev mode doesn't encrypt (developer's
  own machine, dev data dir).
- **A Rust test that reaches `store()` opens with `crate::test_support::isolate_secrets()`.** Under `cfg(test)` the
  backend is `TestStore`, which panics rather than resolve a directory when `CMDR_DATA_DIR` is unset: unisolated, it
  would write fixtures into the developer's real `secrets.json` and make the next run's "nothing is stored yet"
  assertions fail. `docs/testing.md` § "Secret-store isolation (Rust)".
- **`is_file_backed()`** drives a one-time frontend info toast about credential storage. It's true only when the store
  FELL BACK to a file for lack of a keyring; a file store chosen on purpose (tests, dev, E2E) returns false, so dev shows
  no toast.
- **`keys` is a required trait method, ❌ never a default**: "Delete all Cmdr data" finds what to delete through it, so a
  backend that answered empty would leave every password behind. The Keychain listing reads attributes only, which
  raises no consent dialog.

Full details (Linux path-change history, migration notes, the `keyring` v4 ecosystem-split rationale):
`DETAILS.md`.
