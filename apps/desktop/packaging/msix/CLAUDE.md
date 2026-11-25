# Microsoft Store package (MSIX)

Cmdr's MSIX, the Microsoft Store's upload (D3 step 1 of `docs/specs/windows-port-plan.md`): `Package.appxmanifest` (a
template) and `identity.json` (Partner Center's identity, plus a local dev one). `scripts/windows-package/` fills them
in, stages the package, packs it with Microsoft's winapp CLI, and verifies it; the release's `windows-build` job runs
it, and `.github/workflows/windows-store-package.yml` builds it as a CI artifact (no release, no secrets).

## Must-knows

- **The release identity is a placeholder until David copies it from Partner Center** (`docs/guides/releasing.md` §
  Windows releases). `windows-package msix` fails on any `PLACEHOLDER_` value, so a Windows release can't ship an MSIX
  under a made-up identity. ❌ Never invent one.
- **Cmdr's updater never runs in a package.** The package's origin reads as `StorePackage` (Store-signed) or
  `SideloadedPackage` (the dev identity, any sideload) first (`src-tauri/src/updater/platform/windows.rs`). Second
  guard: the MSIX gets the exe from before `tauri bundle` stamped it NSIS, and the tool refuses a stamped exe.
- **❌ No registry keys in the package**, `Policies` least of all: a packaged app's registry view merges the package's
  keys with the real hive, so a shipped key would mix into what admins set. The tool refuses any hive file.
- **Unsigned on purpose**: the Store signs it, and no certificate is made anywhere. A `-identity dev` package runs
  locally unpacked, registered in Developer Mode (`Add-AppxPackage -Register`); Windows 10 has no `-AllowUnsigned`.
- An MSIX version is the release's three numbers plus `.0`, so a prerelease can't ship to the Store.

Each manifest line's why, the layout, and how it was verified: `DETAILS.md`.
