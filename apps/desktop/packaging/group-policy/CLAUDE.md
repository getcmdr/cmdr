# Group Policy template

Cmdr's administrative template for Group Policy and Intune, in the layout admins copy into `PolicyDefinitions`:
`rymd.admx` (the shared `Rymd` vendor category), `cmdr.admx` (Cmdr's category and policies under it), and their en-US
ADMLs in `en-US/`.

## Must-knows

- **Generated, ❌ never hand-edited.** `scripts/group-policy/` builds all four files from
  `apps/desktop/src-tauri/src/managed_policy/policies.json`; change that file, then run `./scripts/group-policy.sh`.
  `desktop-group-policy-fresh` fails CI on any difference.
- **The registry path and policy names are public contracts.** Admins' GPOs name them, so they never move
  (`managed_policy/DETAILS.md` § The Windows registry keys). Display names and explain texts can change freely.
- **Raise `groupPolicy.revision`** in `policies.json` when a policy is added or changed, so the central store and Intune
  see a newer template.
- **Nothing publishes it yet.** It ships with the first Windows release (task 53 of `docs/specs/windows-port-plan.md`).

Schema, namespaces, what each kind writes, how it was validated, and where it ships: `DETAILS.md`.
