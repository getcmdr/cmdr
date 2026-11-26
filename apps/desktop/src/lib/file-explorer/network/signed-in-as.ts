/**
 * Which account an SMB server or share is signed in as, and how the Servers list
 * and a share list say it ("as testuser", "as guest").
 *
 * ❗ Only from what's already known: a live mount's account (the mount table's
 * user), a share listing's own outcome, and ❌ never a Keychain read, since each
 * one can raise a system prompt (`CLAUDE.md`).
 */

export type SignedInAs = { kind: 'user'; username: string } | { kind: 'guest' }
