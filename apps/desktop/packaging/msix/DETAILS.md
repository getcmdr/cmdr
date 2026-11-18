# Microsoft Store package: details

## What's in the package

`scripts/windows-package msix` stages, then packs:

- `Cmdr.exe`: the exe `pnpm tauri build --no-bundle` left, before `tauri bundle` stamps it (below).
- `resources/…`: what `tauri.conf.json`'s `bundle.resources` puts beside the exe in an NSIS install (`resources/ai/*`),
  at the same relative paths, so the app resolves them the same way in both.
- `Assets/StoreLogo.png`, `Square150x150Logo.png`, `Square44x44Logo.png`: from `apps/desktop/src-tauri/icons/`, which
  `tauri icon` makes from `brand/logos/cmdr.svg` (`docs/guides/updating-icon.md`).
- `AppxManifest.xml`: `Package.appxmanifest` with the identity and version filled in. winapp renames it and adds the
  `resources.pri` index.

`verify-msix` (also run at the end of `msix`) reopens the package and checks the identity, the `X.Y.Z.0` version, `x64`,
the one application running `Cmdr.exe`, that exe unstamped, the logos and resources present, and no registry hive.

## The manifest, line by line

- **`Identity`**: `Name` and `Publisher` are what Partner Center assigns under Product management › Product identity
  (`Package/Identity/Name`, `Package/Identity/Publisher`); the Store refuses an upload whose identity differs. Version
  `X.Y.Z.0`: the Store reserves the fourth part, and every update must be higher.
- **`PublisherDisplayName`**: also from Product identity (`Package/Properties/PublisherDisplayName`).
- **`TargetDeviceFamily` `MinVersion="10.0.19045.0"`**: Windows 10 22H2, D4's floor. `MaxVersionTested` is the Windows
  SDK the builds use (10.0.26100).
- **`EntryPoint="Windows.FullTrustApplication"`** and **`runFullTrust`**: a desktop app packaged as-is (a "packaged
  desktop app"), with the same file system and process rights as the NSIS install. The Store reviews `runFullTrust` as a
  restricted capability; every packaged Win32 app declares it.
- **No `Extensions`** yet: the Explorer verb, the Windows 11 top-level context menu, and file associations are task 49's
  and later.
- **WebView2**: not bundled. The Evergreen runtime ships with Windows 11 and every supported Windows 10 (D4), and the
  NSIS installer's bootstrapper isn't available inside an MSIX.
- **`Description`** "The AI-native file manager" (the tagline, `docs/guides/branding.md`) and `DisplayName` "Cmdr" are
  user-facing copy, for David's review with the Store listing (task 57).

## The two identities

`identity.json` holds `release` (Partner Center's, placeholders until David fills them in) and `dev`: `Rymd.Cmdr.Dev` by
`CN=Cmdr Dev, OID.2.25.311729368913984317654407730594956997722=1`. That OID marks an unsigned package, which
`Add-AppxPackage -AllowUnsigned` installs without a certificate (Microsoft Learn, "An overview of Package Identity",
read 2026-11-18). A dev package never goes to the Store.

## Decision/Why: the unstamped exe

Tauri's bundler patches `__TAURI_BUNDLE_TYPE_VAR_UNK` in the binary it packages into the bundle's type (`…_NSS` for
NSIS; tauri-utils 2.10.2 `platform::bundle_type`, source read 2026-11-18), and the updater's Windows arm
(`apps/desktop/src-tauri/src/updater/platform/windows.rs`) updates only an NSIS-stamped install. The NSIS bundle and the
MSIX come from one compile, so the release job copies the exe before `tauri bundle` runs, and the MSIX gets that copy:
inside the Store package the updater answers "not running from an installed build" and stays off, as D3 says the Store
build must. A stamped exe is refused at packaging and again in `verify-msix`. Task 57 still owns checking the rest of
the Store build's behavior (where MSIX redirects the data and cache folders, "Delete all Cmdr data").

## Decision/Why: no registry in the package

A packaged app sees the real registry merged with its package's own keys (Microsoft Learn, "MSIX registry
virtualization", read 2026-11-18). Cmdr reads managed policy from `SOFTWARE\Policies\Rymd\Cmdr`
(`managed_policy/DETAILS.md` § The Windows registry keys), so any key the package shipped there would read as an admin's
setting. The package ships no `Registry.dat`, `User.dat`, or `UserClasses.dat`, and its manifest names no `Policies`
path; both are checked.

## Why winapp

The plan chose Microsoft's winapp CLI (`microsoft/winappCli`, MIT; v0.8.0, 2026-11-07, about 1,400 stars, active), which
wraps the Windows SDK's MakeAppx and resolves the manifest. `winapp pack <stage> --manifest … --no-sign`, with
`WINAPP_CLI_TELEMETRY_OPTOUT=1` (it collects usage data by default). The release job installs it through
`microsoft/setup-WinAppCli`, pinned by commit, with the release zip's SHA-256.

## Verified

Filled in from the builder run below; see `docs/notes/windows-port/build-progress.md` for the full record.
