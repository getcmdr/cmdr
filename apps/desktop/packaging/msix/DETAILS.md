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
`CN=Cmdr Dev`. A dev package never goes to the Store, and nothing signs it. To run one without a certificate, unpack it
and register the folder in Developer Mode (verified on Windows 10 22H2 build 19045, mami, 2026-11-18):

1. Turn on Developer Mode (Settings › For developers, or `AllowDevelopmentWithoutDevLicense=1` under
   `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock`).
2. Unzip `Cmdr_<v>_x64.msix` into a folder and run `Add-AppxPackage -Register <folder>\AppxManifest.xml`.
3. Start it with `explorer.exe shell:AppsFolder\<PackageFamilyName>!Cmdr` (`Get-AppxPackage Rymd.Cmdr.Dev` names the
   family). Remove it with `Get-AppxPackage Rymd.Cmdr.Dev | Remove-AppxPackage`, then turn Developer Mode off again.

Gotcha/Why: no unsigned-package marker in the dev publisher. Windows 10's Appx module has no `-AllowUnsigned`, and its
Developer Mode registration refuses a publisher carrying the marker
(`OID.2.25.311729368913984317654407730594956997722=1`) with 0x80073D2D, "not in the signed namespace". A plain `CN=`
publisher registers on both Windows 10 and 11.

## Decision/Why: the unstamped exe

Tauri's bundler patches `__TAURI_BUNDLE_TYPE_VAR_UNK` in the binary it packages into the bundle's type (`…_NSS` for
NSIS; tauri-utils 2.10.2 `platform::bundle_type`, source read 2026-11-18), and the updater's Windows arm
(`apps/desktop/src-tauri/src/updater/platform/windows.rs`) updates only an NSIS-stamped install. The NSIS bundle and the
MSIX come from one compile, so the release job copies the exe before `tauri bundle` runs, and the MSIX gets that copy:
inside the Store package the updater answers "not running from an installed build" and stays off, as D3 says the Store
build must. A stamped exe is refused at packaging and again in `verify-msix`.

On tauri-cli 2.12 the bundler patches the copy it installs and leaves `target/release/Cmdr.exe` unstamped (verified on
mami, 2026-11-18: the target exe's hash didn't change across `tauri bundle`, it still carried `…_UNK`, and the installed
`%LOCALAPPDATA%\Cmdr\Cmdr.exe` carried `…_NSS`). The copy-first step keeps the MSIX right if a later CLI patches in
place, and the check catches it if the copy step goes. Task 57 still owns the rest of the Store build's behavior (where
MSIX redirects the data and cache folders, "Delete all Cmdr data"). Seen on the dev package's run: a packaged process
logged into the existing `%LOCALAPPDATA%\com.veszelovszki.cmdr\logs\cmdr.log`, not a per-package folder.

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
`microsoft/setup-WinAppCli`, pinned by commit, with the release zip's SHA-256. On its first pack winapp fetches the
`Microsoft.Windows.SDK.BuildTools` NuGet package (10.0.28000.2957 on 2026-11-18) into its cache. It runs on Windows 10
too, though its docs list Windows 11.

Gotcha/Why: the manifest is staged outside the package folder. winapp packs every file in the folder it's given, so a
`Package.appxmanifest` inside shipped beside the real `AppxManifest.xml` (seen on mami, 2026-11-18); `verify-msix`
refuses that. winapp also leaves `pri.resfiles` and `priconfig.xml` from its resource index in the package, which are
harmless.

## Verified

On mami (Windows 10 Pro 22H2, build 19045, 2026-11-18; `docs/notes/windows-port/build-progress.md` has the run): a
release `tauri build --no-bundle` of `3224901455` plus this tooling, `msix -identity dev` packed and verified (55 MB),
`msix` with the release identity failed on its placeholder as intended, and the dev package registered in Developer
Mode, started from its `shell:AppsFolder` entry, listed `C:\Windows` and `D:\`, closed with Alt+F4, and unregistered
with no package folder left. Not verified: a Store upload (needs the Partner Center identity) and the Windows 11 context
menu.
