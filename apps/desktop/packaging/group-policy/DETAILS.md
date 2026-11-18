# Group Policy template: details

## Files and generation

- `rymd.admx` + `en-US/rymd.adml`: the vendor file. Namespace `Rymd.Policies`, prefix `rymd`, one category `Rymd` at the
  root of Administrative Templates, no policies.
- `cmdr.admx` + `en-US/cmdr.adml`: namespace `Rymd.Policies.Cmdr`, prefix `cmdr`,
  `<using prefix="rymd" namespace="Rymd.Policies">`, a `Cmdr` category with `parentCategory ref="rymd:Rymd"`, and every
  policy.
- **Decision/Why: a separate vendor file.** Each ADMX defines categories in its own namespace, so two products that each
  defined `Rymd` would show two `Rymd` folders. A shared vendor file that each product's template references is how
  multi-product vendors ship (Mozilla's `mozilla.admx` beside `firefox.admx`; Intune's import docs name that pair), so
  Rymdskottkärra AB's second product adds its own `<product>.admx` under the same `Rymd` folder.
- `scripts/group-policy/` (Go, run through `./scripts/group-policy.sh`) reads
  `apps/desktop/src-tauri/src/managed_policy/policies.json` (the definition's rules:
  `apps/desktop/src-tauri/src/managed_policy/DETAILS.md` § The policy definition) and writes the four files. Names,
  prefixes, and the `supportedOn` id derive from `groupPolicy.vendorCategory` and `productCategory`. Before writing, it
  holds each ADMX and ADML together: every `$(string.X)` and `$(presentation.X)` resolves, no string goes unused, and
  each presentation control names an ADMX element.
- `desktop-group-policy-fresh` builds the generator and runs it with `-check` (exit 3 means drift). Locally it
  regenerates the files; with `--ci` a difference fails. It runs in CI's light lane on every change.
- Each explain text ends with a generated "Registry:" line naming the key, value, and type, for admins who set values by
  script or check a PC with `reg query`.

## What each kind writes

All policies are `class="Both"`: Cmdr reads the key under `HKLM` and `HKCU`, so a policy can be set under Computer or
User Configuration, and the computer's value wins per name (`managed_policy/platform/registry_values.rs`).

- `bool`: `valueName` = the policy name under `SOFTWARE\Policies\Rymd\Cmdr`, `enabledValue` decimal 1, `disabledValue`
  decimal 0 (`REG_DWORD`). The app reads 0 as "not managed".
- `string`: a `<text>` element, `required="true"`, `REG_SZ` named after the policy. Disabling the policy deletes it.
- `stringList`: a `<list>` element at `SOFTWARE\Policies\Rymd\Cmdr\<Name>` with `valuePrefix=""`, so the entries are
  `REG_SZ` values named 1, 2, and so on. The policy itself writes no value (a plain value of the same name would outrank
  the list in the app). Disabling it clears the values and can leave the subkey behind, empty, so the app reads an empty
  list subkey as not set (`registry_values.rs`). To allow no cloud AI host, admins use `DisableCloudAI`; the explain
  text says so.
- `supportedOn`: one definition of our own (`SUPPORTED_Cmdr_Windows`), so the template needs no `Windows.admx`
  (`using prefix="windows"`), which Intune would make admins upload first.

## Schema conformance

Sources (read 2026-11-18): Microsoft's MS-GPREG § 7 "Appendix B: Full XML Schemas" (BaseTypes, PolicyDefinitions,
PolicyDefinitionFiles) and § 4.4 examples; "Understanding ADMX policies" (learn.microsoft.com, updated 2025-08-04);
"Win32 and Desktop Bridge app ADMX policy Ingestion" (updated 2025-11-20); Intune's "Import custom ADMX and ADML
administrative templates" (updated 2026-11-16).

- Namespace `http://schemas.microsoft.com/GroupPolicy/2006/07/PolicyDefinitions`, `revision` and `schemaVersion` `1.0`,
  `resources minRequiredRevision` equal to the revision, an ADML with `displayName`, `description`, `stringTable`, and a
  `presentationTable` (`textBox` with a `label`, `listBox`).
- Intune: up to 20 ADMX files of 1 MB each, one en-US ADML per ADMX, dependencies imported first (so `rymd.admx` before
  `cmdr.admx`), combo boxes unsupported (we use none). An ingested template may not write under `System`,
  `Software\Microsoft`, or `Software\Policies\Microsoft`; `Software\Policies\Rymd` is clear of those.
- Replacing an imported template in Intune means deleting the old one (and the profiles using it) first, or importing
  one with a new namespace; keep the namespaces stable and raise `revision` instead.

## Validation

- **Verified 2026-11-18**, against the schema rebuilt from MS-GPREG § 7: the web listing wraps lines mid-token, writes
  the target namespace as `http://www.microsoft.com/GroupPolicy/PolicyDefinitions`, and escapes `$` as `\$`. So the
  lines were rejoined and the namespace set to the 2006/07 one. Microsoft's own § 4.4 ADMX example and winget's
  `DesktopAppInstaller.admx` validate against the rebuilt schema, as a control.
  - `xmllint --schema` (libxml2, macOS): all four files valid. libxml2 rejects `\$` in a pattern, so its copy uses a
    plain `$`.
  - .NET's `XmlSchemaSet` in Windows PowerShell 5.1 on the Windows 10 22H2 builder (build 19045): all four valid, a copy
    with an invalid `class` value refused, and every policy read back with the class, value type, and registry path
    above. .NET reads an unescaped `$` as an anchor, so its copy keeps `\$`. Run from a temp folder over SSH; the files
    never went into `C:\Windows\PolicyDefinitions` and no policy was set.
- Not verified yet: loading the template in the Group Policy editor and Intune, and a GPO round trip (enable, disable,
  not configured) read back by a running Cmdr. That's on task 55's checklist in `docs/specs/windows-port-plan.md`.
- To repeat the schema check: rebuild the three XSDs from MS-GPREG § 7 as above, then
  `xmllint --noout --schema PolicyDefinitionFiles.xsd rymd.admx cmdr.admx en-US/*.adml`. The XSDs aren't committed
  (Microsoft's spec text, and no Go XSD validator to run them in CI).

## Where it ships

- Lives here, in the repo, generated and committed. Nothing publishes it yet, and `/trust` doesn't link it.
- With the first Windows release (task 53 of `docs/specs/windows-port-plan.md`): the release attaches it as one zip
  (`cmdr-group-policy-<version>.zip`, holding this folder's layout) and `/trust#windows-policy` links the latest one.
