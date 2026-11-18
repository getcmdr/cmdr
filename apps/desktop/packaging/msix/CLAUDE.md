# Microsoft Store package (MSIX)

Cmdr's MSIX, the Microsoft Store's upload (D3 step 1 of `docs/specs/windows-port-plan.md`): `Package.appxmanifest` (a
template) and `identity.json` (Partner Center's identity, plus a local dev one). `scripts/windows-package/` fills them
in, stages the package, packs it with Microsoft's winapp CLI, and verifies it; the release's `windows-build` job runs
it.

## Must-knows

- **The release identity is a placeholder until David copies it from Partner Center** (`docs/guides/releasing.md` §
  Windows releases). `windows-package msix` fails on any `PLACEHOLDER_` value, so a Windows release can't ship an MSIX
  under a made-up identity. ❌ Never invent one.
- **The MSIX gets the exe from before `tauri bundle` ran.** The bundler stamps its bundle type into the exe it packages,
  and an NSIS-stamped exe would run the NSIS updater inside the Store's package. Unstamped, the updater reads it as "not
  an installed build" and stays off, and the Store updates it. The tool refuses a stamped exe.
- **❌ No registry keys in the package**, `Policies` least of all: a packaged app's registry view merges the package's
  keys with the real hive, so a shipped key would mix into what admins set. The tool refuses any hive file.
- **Unsigned on purpose**: the Store signs it. `-identity dev` installs locally with `Add-AppxPackage -AllowUnsigned`
  (its publisher carries the unsigned-package marker); no certificate anywhere.
- An MSIX version is the release's three numbers plus `.0`, so a prerelease can't ship to the Store.

Each manifest line's why, the layout, and how it was verified: `DETAILS.md`.
