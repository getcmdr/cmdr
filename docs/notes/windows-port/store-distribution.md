# Microsoft Store distribution: findings and route (2026-11-24)

What the Microsoft Store accepts for Cmdr on Windows in late 2026, what it asks of us, and the route we take. Written
for task 57 of `docs/specs/windows-port-plan.md` (#942) after David chose the Store as the first alpha's channel
(2026-11-24; the unsigned NSIS build stays working as the internal test build). The account itself is #1348. How the
package is built: `apps/desktop/packaging/msix/DETAILS.md`. How a release ships it: `docs/guides/releasing.md` § Windows
releases.

Every source below was read on 2026-11-24 unless it says otherwise; "updated" is the page's own date.

## The route

1. **Submit an MSIX, never the EXE/MSI path.** The Store re-signs an MSIX for free; an EXE or MSI submission must
   already carry an Authenticode signature from a CA in Microsoft's Trusted Root Program, which we don't have (D3: no
   certificate until Azure Artifact Signing, task 51).
2. **Package it ourselves with winapp, as today**: Tauri 2 has no MSIX bundler, and its Store guide only covers the
   signed-EXE path.
3. **A full-trust packaged desktop app** (`runFullTrust`, `Windows.FullTrustApplication`), no `broadFileSystemAccess`:
   that capability is for the sandboxed WinRT storage APIs, and a full-trust app already has the user's own file system
   rights.
4. **The Store updates a Store install; Cmdr's updater stands down** and says so in Settings and the check toast. The
   package can't replace its own files anyway (they live read-only under `WindowsApps`).
5. **A free listing, with the Commercial license sold as today** (Paddle, in a browser): allowed for a non-game PC app.
   Whether that's the Store setup is David's call (Needs David below).
6. **First alpha as a Private audience**, then Public. Public can't go back to Private.
7. **Submit by hand the first time** (Partner Center requires it); automate later updates with the Microsoft Store
   Developer CLI from CI, which needs four Partner Center credentials as secrets.

## Findings

### Package type and signing

- **MSIX: the Store signs it, free, worldwide, no SmartScreen warning. MSI/EXE: "Microsoft does not re-sign your
  installer. The installer and its PE files must be signed with a certificate chaining to a CA in the Microsoft Trusted
  Root Program — self-signed certificates are not accepted."** (Microsoft Learn, "Code signing options for Windows app
  developers", <https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options>.)
- The EXE/MSI path (Store policy 10.2.9) also needs a silent install, a standalone installer (no web stub), and a new
  versioned download URL per update, which we'd host (Store Policies 7.20, published 2026-09-15, effective 2026-10-22,
  <https://learn.microsoft.com/en-us/windows/apps/publish/store-policies>).
- **Package version**: the fourth part is the Store's and must be `0` at build time; the others are 0–65535, the first
  non-zero (Microsoft Learn, "App package requirements for MSIX app",
  <https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements>). Our
  `X.Y.Z.0` already follows it, so a prerelease tag can't ship to the Store.
- **Tauri**: "Currently Tauri only generates EXE and MSI installers … The installer linked in the Microsoft Installer
  must be offline, handle auto-updates and be code signed", and it asks for the `offlineInstaller` WebView2 mode (Tauri
  docs, "Microsoft Store", last updated 2026-06-15, <https://v2.tauri.app/distribute/microsoft-store/>). That's the
  10.2.9 path, so it needs the certificate we don't have. The same page notes the publisher name can't equal the product
  name, which matters for the publisher display name (Needs David).

### Capabilities and file system access

- `runFullTrust` is a restricted capability: Partner Center asks how the app uses it on the submission's Submission
  options page, and that can add certification time. Every packaged Win32 app declares it (Microsoft Learn, "App
  capability declarations", updated 2026-10-02,
  <https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations>).
- `broadFileSystemAccess` "works for the Windows.Storage APIs" and needs "specific reasons as to why [the pickers]
  aren't sufficient" (same page). Cmdr reads and writes through Win32 as a medium-integrity process, so it doesn't
  apply. ❌ Don't declare it: it would add a second restricted-capability review for nothing.
- **AppData writes are virtualized.** On Windows 10 1903 and later, new files and folders a packaged app creates under
  `AppData\Local` and `AppData\Roaming` go to a per-user, per-package private location, merged back into the app's own
  view; edits to files that already existed go to the real location; uninstall removes the private copies (Microsoft
  Learn, "Understanding how packaged desktop apps run on Windows", updated 2026-09-30,
  <https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes>). Consequences for Cmdr:
  - Good: Store installs uninstall cleanly (policy 10.2.7 asks for that).
  - To fix before the Store alpha (task 57 already names it): anything that hands a data-dir path to another process
    sees the real `%LOCALAPPDATA%\com.veszelovszki.cmdr`, which on a fresh Store install doesn't exist there. "Show in
    File Explorer" on the logs folder, a log path pasted into a bug report, and an outside tool reading Cmdr's data all
    miss. The redirected copy lives under `%LOCALAPPDATA%\Packages\<PackageFamilyName>\LocalCache\Local\…` (to verify on
    a real Store install).
  - An NSIS install's folder that already exists is edited in place by the Store build (seen on the dev package,
    2026-11-18, `apps/desktop/packaging/msix/DETAILS.md`). So someone moving from the test build to the Store keeps
    their settings, and the Store's uninstall leaves that folder.
  - Opting out (`desktop6:FileSystemWriteVirtualization` `disabled`) needs the `unvirtualizedResources` restricted
    capability, which Microsoft says is "intended to be used only by certain types of desktop PC games" (Microsoft
    Learn, element page, updated 2026-06-05; "Flexible virtualization" for Windows 11's per-folder form). ❌ Don't: it's
    the wrong lever for a file manager.
- Registry writes to HKCU are virtualized the same way; the package ships no keys
  (`apps/desktop/packaging/msix/DETAILS.md` § no registry in the package).

### WebView2

- The MSIX doesn't bundle or bootstrap WebView2. Microsoft says the Evergreen runtime ships with Windows 11 and that
  "some devices might not have the Runtime pre-installed, so it's a good practice to check" before creating a WebView2
  (Microsoft Learn, "Distribute your app and the WebView2 Runtime", updated 2026-10-02,
  <https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution>). The only declared-dependency route
  it names (`win32dependencies:ExternalDependency`) is for App Installer deployments, not the Store.
- Risk is low on our floor (D4: Windows 10 22H2 and 11), but a missing runtime today fails at window creation with
  Tauri's own error. Follow-up: a startup check with a "Get WebView2" link (P, not a Store gate).

### Updates

- A Store MSIX updates through the Store, automatically on desktop; package flights and gradual rollout also go through
  it (Microsoft Learn, "Package flights", <https://learn.microsoft.com/en-us/windows/apps/publish/package-flights>).
- Policy 10.2.5 ("installed and updated only through the Store") is worded for games and Xbox products, but for an MSIX
  it's moot: the installed files are read-only, and running our NSIS installer from inside the package would install a
  second, separate Cmdr. So a Store install must never download or stage an update.
- What the build does about it: the packaged process reports a `StorePackage` install, the update check answers
  `updatedByStore` with no request to `api.getcmdr.com`, and Settings and the manual check say updates come from the
  Microsoft Store (`apps/desktop/src-tauri/src/updater/DETAILS.md`). The exe-stamp check stays as a second guard.
- Managed policy: `DisableUpdates` and `MaxUpdateVersion` can't hold a Store install back; Cmdr answers "updates come
  from the Store" there too, and an admin uses Windows' own Store policies. Documented in the managed policy docs.
- Lost by it: a Store install never hits `update-check`, so the dashboard's active-install count misses Store installs
  (the `update_check` analytics event still reports `updated_by_store` when analytics is on; Partner Center has its own
  install numbers).

### Commerce and Cmdr's licensing

- "Non-game products made available on PC devices may either use a secure third-party purchase API or the Microsoft
  Store in-product purchase API for in-app purchases of digital items or services" (policy 10.8.1); subscriptions
  likewise (10.8.6). Using a third party means: identify the provider at the transaction, authenticate the user, and
  confirm it; meet PCI DSS (Paddle does); purchases may happen in a browser after install; and **note the third-party
  purchase API in Partner Center at submission** (10.8.2).
- 10.8.4: the product and its listing must say what in-product purchases exist and their price range, and be clear about
  any trial's scope. Cmdr's "Personal use only" window title and monthly reminder fit, as long as the listing says
  commercial use needs a paid license.
- Microsoft's own summary: non-game apps can "choose their own commerce platform and keep 100% of the revenue", or use
  Microsoft's at 15% for apps (Microsoft Learn, "Get started with Microsoft Store",
  <https://learn.microsoft.com/en-us/windows/apps/publish/get-started>).
- A Store price other than Free makes even a private audience pay. Store trials are 1, 7, 15, or 30 days, Store-managed
  ("Pricing and availability",
  <https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/price-and-availability>).
- **The Store Developer CLI updates free products only** ("App update operations … currently supported for free products
  only", "Microsoft Store Developer CLI", updated 2026-10-01,
  <https://learn.microsoft.com/en-us/windows/apps/publish/msstore-dev-cli/overview>). A paid Store listing means hand
  submissions or the older submission API.
- **The BSL** goes in as the listing's own license terms (Partner Center's "Additional license terms"; the submission
  API calls the field `additionalLicenseTerms`,
  <https://learn.microsoft.com/en-us/windows/apps/publish/store-submission-api>). Without it the Store's Standard
  Application License Terms apply, which don't say commercial use needs a license.

### Privacy, telemetry, and crash reports

- 10.5.1: an app that "accesses, collects or transmits Personal Information" must keep a privacy policy and enter its
  URL in Partner Center, saying what's collected, how it's used, stored, and secured, who it's shared with, and the
  user's controls. Partner Center's Properties page asks whether the app collects personal info and requires the URL if
  so ("Enter app properties",
  <https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/enter-app-properties>). Ours is
  `getcmdr.com/privacy-policy`; it must cover Windows (device id from `MachineGuid`, crash reports, update checks,
  analytics, the license check, AI requests to the user's own provider) before submission.
- 10.5.2: publishing a customer's personal info to "an outside service or third party" needs opt-in consent with a way
  to opt out later. Analytics is opt-out and on by default (`docs/business/product-facts.md`), and goes to our own
  server. Whether Microsoft reads that as "an outside service" is a review risk, not a settled no; a clear first-run
  disclosure plus the Settings switch is the defense. Flagged for David.
- 10.5.4: personal info in transit uses modern cryptography (every request is HTTPS).

### Age rating and listing

- 11.11: an IARC age rating, from Partner Center's questionnaire at first submission, kept current (Store Policies 7.20;
  "Age ratings for MSIX app", updated 2026-08-21,
  <https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/age-ratings>). Microsoft shares the
  publisher display name and email with IARC. Ask Cmdr shows model output, which the questionnaire's "unrestricted
  internet / user-generated content" style questions may touch; answer them as they stand when David fills it in.
- 10.1: the title must be unique and carry no marketing text, the value must be clear on first run, and the app must be
  "fully functional" (10.1.2), which a labelled alpha can still be.
- Search terms: at most seven, no pricing terms (10.1.3).
- Listing languages: Partner Center likely allows a listing only in languages the package declares, and ours declares
  `en-us` alone (not verified for MSIX; Microsoft's Marketplace FAQ says so for its own offers). Add the 13 locales'
  `Resource` lines before a localized listing.

### Distribution to a small alpha group

- **Private audience**: only Microsoft accounts in a known user group can see or install it, through a sign-in link. "If
  you submit a product with this option set to Public audience, you can't choose Private audience in a later
  submission." Reviews from a private audience never show on the public listing ("Visibility options", updated
  2026-08-28, <https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/visibility-options>).
- "Available but not discoverable" (direct link) is not a confidential channel: Microsoft says not to use it for
  controlled pre-release audiences.
- Package flights split packages, not listings, within one product, so a later beta channel can ride the public listing.

### Submission automation

- Partner Center requires the first publication by hand, age rating included. After that, the Store Developer CLI
  (`msstore`, preview) publishes updates from CI via
  `msstore reconfigure --tenantId --sellerId --clientId --clientSecret` (or a certificate), with Microsoft's
  `microsoft/microsoft-store-apppublisher` action; it needs an Entra ID tenant associated with the Partner Center
  account, not an MSA ("Microsoft Store Developer CLI commands", updated 2026-10-05,
  <https://learn.microsoft.com/en-us/windows/apps/publish/msstore-dev-cli/commands>).

## What we build now, and what waits for Partner Center

Built (no account needed): the package build as a CI artifact (`.github/workflows/windows-store-package.yml`), the
identity from `apps/desktop/packaging/msix/identity.json` so the real values drop in as one commit, the `StorePackage`
install kind and its UI, the Windows triple in `deny.toml`, and the Store section of `docs/guides/releasing.md` (marked
not live).

Waits for the account (#1348):

1. Reserve the name, copy the three identity values into `identity.json` (`docs/guides/releasing.md` § Windows
   releases).
2. First submission by hand: the package from the CI artifact or a release, the listing, the age rating, the privacy
   policy URL, the `runFullTrust` explanation, the third-party purchase note, the additional license terms, and the
   audience.
3. An Entra ID tenant linked to Partner Center and an app registration for the CLI, then four repo secrets (below) and a
   `store-submit` job.

Secrets the automation will need (none exist yet): `PARTNER_CENTER_TENANT_ID`, `PARTNER_CENTER_SELLER_ID`,
`PARTNER_CENTER_CLIENT_ID`, `PARTNER_CENTER_CLIENT_SECRET` (the names Microsoft's own example uses). The package
identity values are public and live in `identity.json`, not in secrets.

## Open questions to verify on real Windows

- Where a fresh Store install's data dir, logs, and drive index land, and whether "Show in File Explorer" on them opens
  an empty or missing folder.
- Whether a Store install's process sees the NSIS install's existing settings, and what an NSIS-to-Store switch should
  do with the NSIS copy (two Cmdrs side by side otherwise).
- That `GetCurrentPackageFullName` answers for the Store-signed package as it does for the dev one.
