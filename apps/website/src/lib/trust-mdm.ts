/**
 * The `/trust` content that is a contract with the app binary rather than page copy: the code
 * requirement the Full Disk Access profile grants, and every managed preference (MDM) key the app
 * reads. Two Rust tests embed this file (`managed_policy/public_docs_test.rs` and
 * `pppc_profile_test.rs`), so CI's `rust` filter and the Rust check lanes list it as an input. Page
 * copy goes in `trust.ts`, so editing prose there doesn't run the Rust suite.
 *
 * Same truth rule as `trust.ts`: every claim must hold for the released app.
 *
 * Key names, kinds, and the Windows registry key come from `policies.json`, the one policy
 * definition the app's tests, this page, and the Group Policy template generator read. Rust tests
 * pin it to what the app reads (`managed_policy/public_docs_test.rs`); its rules are in
 * `apps/desktop/src-tauri/src/managed_policy/DETAILS.md` § The policy definition.
 */

import policyDefinition from '../../../desktop/src-tauri/src/managed_policy/policies.json'

/**
 * The app's designated requirement, exactly as `codesign -d -r-` prints it for a release build.
 * `/trust` shows it, and `public/mdm/cmdr-full-disk-access.mobileconfig` grants Full Disk Access to
 * it. A Rust test (`managed_policy/pppc_profile_test.rs`) checks both against the bundle id and the
 * Team ID in `tauri.macos.conf.json`, so a signing change can't silently break the profile.
 */
export const codeRequirement =
  'identifier "com.veszelovszki.cmdr" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = "83H6YAQMNP"'

export type PolicyKind = 'bool' | 'string' | 'stringList'

/**
 * The registry key Windows reads policies from, under both `HKLM` and `HKCU` (machine wins), from
 * the definition. `/trust#windows-policy` builds every mention from this. A public contract:
 * admins' GPOs name it.
 */
export const windowsPolicyKey: string = policyDefinition.windowsPolicyKey

export interface ManagedPreferenceKey {
  /** The key name in the `com.veszelovszki.cmdr` preference domain. */
  key: string
  type: 'Boolean' | 'String' | 'Array of strings'
  /** Inline HTML. */
  effect: string
}

const plistTypes: Record<PolicyKind, ManagedPreferenceKey['type']> = {
  bool: 'Boolean',
  string: 'String',
  stringList: 'Array of strings',
}

/**
 * What each key does, as `/trust` says it (inline HTML). A Rust test
 * (`managed_policy/public_docs_test.rs`) checks these names against `managed_policy/keys.rs`, and
 * `managedPreferenceKeys` below refuses to build when one is missing or extra. The canonical
 * catalog is `apps/desktop/src-tauri/src/managed_policy/DETAILS.md`.
 */
const policyEffects: Record<string, string> = {
  DisableUsageStats:
    'No usage stats. Cmdr sends no heartbeat and no feature events, and deletes the ones waiting on the Mac. "Send usage stats" stays off.',
  DisableCrashAndErrorReports:
    "No crash reports and no error reports, automatic or sent by hand. A crash report waiting from the last session is deleted without being offered. Users can still save an error report to disk. In-app feedback isn't covered: it's a message the user writes and sends on purpose.",
  DisableAutomaticUpdateChecks: 'No background update checks. A user can still check by hand.',
  DisableUpdates:
    'No update check, download, or install of any kind, so Cmdr never contacts the update servers. For teams that ship new versions themselves. Overrides the other two update keys.',
  MaxUpdateVersion:
    "\"Never update past this version.\" <code>0.52</code> allows up to the last 0.52.x, <code>0.52.3</code> up to and including 0.52.3, and <code>1</code> anything below 2.0.0. A whole number works too. A value Cmdr can't read turns updates off. <strong>What it can't do</strong>: Cmdr only learns about its newest release, so once a release past the ceiling ships, the Mac gets no more updates, patches included, until you raise it. It holds Cmdr at a version; it isn't an update channel.",
  DisableAI:
    "No AI at all: no request to any AI provider, no on-device model download, no local AI server, and no Ask Cmdr. AI search over the MCP server refuses too. Image search isn't covered: its on-device model indexes images and doesn't generate anything.",
  DisableCloudAI:
    "On-device AI only: nothing goes to a cloud AI provider. An Ollama or LM Studio server counts as cloud, even on the same Mac, because <code>localhost</code> can be a tunnel to another machine. Cmdr's own on-device model still works.",
  AllowedCloudAIHosts:
    "Cloud AI may reach only these hosts. Each entry is a host (<code>api.openai.com</code>), a host and port (<code>localhost:11434</code>), a pattern for any subdomain (<code>*.openai.azure.com</code>, which doesn't match <code>openai.azure.com</code> itself), or a pasted URL, of which only the host and port count. Case doesn't matter. Cmdr checks every request and every redirect. It skips entries it can't read, and an empty list allows no host, the same as <code>DisableCloudAI</code>.",
  DisableSecurityLogOptOut:
    'The security event log stays on, and users can\'t turn it off. It\'s a small local file of security-relevant changes and refusals (a policy taking effect, an update installed or refused, the MCP server switching on, a rejected access attempt). It never leaves the Mac, and "Delete all Cmdr data" still removes it.',
  RequireSMBEncryption:
    "Every connection Cmdr makes to an SMB share is encrypted, even when the server doesn't ask for it, and Cmdr doesn't connect to a server that can't encrypt. Guest connections can't be encrypted, so users sign in. \"Always require SMB encryption\" stays on, and users can't turn it off. It covers Cmdr's own connections: the macOS connection a mounted share keeps for Finder follows macOS's settings. A connection that's already open when the policy arrives keeps going until it reconnects.",
}

const policies = policyDefinition.policies as { name: string; kind: PolicyKind }[]

const unexplained = policies.map((p) => p.name).filter((name) => !(name in policyEffects))
const stray = Object.keys(policyEffects).filter((name) => !policies.some((p) => p.name === name))
if (unexplained.length > 0 || stray.length > 0) {
  throw new Error(
    `trust-mdm.ts: policyEffects must cover exactly the keys in policies.json (missing: ${unexplained.join(', ') || 'none'}; extra: ${stray.join(', ') || 'none'})`,
  )
}

/**
 * Every managed preference (MDM) key the app reads, in catalog order, with its plist type from the
 * definition. A Rust test checks both files in `public/mdm/` against the same names and kinds.
 */
export const managedPreferenceKeys: ManagedPreferenceKey[] = policies.map(({ name, kind }) => ({
  key: name,
  type: plistTypes[kind],
  effect: policyEffects[name],
}))

/** The name of a policy the page names in its own copy, refusing to build if the definition doesn't
 *  carry it with that kind, so a rename or a kind change can't leave the copy wrong. */
export function policyNamed(name: string, kind: PolicyKind): string {
  if (!policies.some((p) => p.name === name && p.kind === kind)) {
    throw new Error(`trust-mdm.ts: policies.json has no ${kind} policy named ${name}`)
  }
  return name
}
