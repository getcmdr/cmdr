# Proposal spine (`agent/store/proposals/`)

Sweeps, groups, ops, and the acceptance record, in `main.db` (migration v4, in `../migrations.rs`). Three levels:
a **sweep** (`proposal_sets`) is one agent wake's output, a **group** (`proposals`) is the reviewable unit and exactly
one executor call, an **op** (`proposal_ops`) is one path that may be a whole directory. Depth: `DETAILS.md`.

## Module map

- `write.rs` — `create_sweep` / `create_group` (creation is where a proposal FREEZES) and `repropose_group`.
- `read.rs` — group headers, `COUNT(*)` counts, and the one paged op reader.
- `claim.rs` — preflight, the claim transaction and its release, rejection, and the streaming binding.
- `complete.rs` — what execution writes back: per-op outcomes, and the group's own end.
- `recovery.rs` — the `interrupted` startup sweep. `overlap.rs` — newest wins per file; a run retires others' ops.
- `draft.rs` — a server group's plain shapes (`ServerDraft`); `server.rs` — its draft writes (v13) and its claim.
- `thread_line.rs` — a group's one thread line (v10), rewritten in place.

## Must-knows

- **The claim is the one place a bug applies ops to real files twice.** One `BEGIN IMMEDIATE`: read the stored
  acceptance, compare the live op set as a hash plus count, conditional `UPDATE ... WHERE status = 'pending'`, refuse on
  a mismatch **or** on `rows_affected == 0` as two distinct typed variants (the recovery differs). ❌ Don't reorder it.
  It leaves op statuses alone, so a losing concurrent claim reports stale status, not a mismatch the winner caused
  (`tests/claim.rs::two_concurrent_claims_leave_exactly_one_winner_and_a_typed_refusal` holds it).
- **The acceptance record is SERVER-OWNED.** The client presents an `OpSelection` of op IDS, never values
  (`DETAILS.md` § What preflight accepts). Comparing `proposal_ops` against itself is a tautology once the agent can
  amend a pending group, so the record held apart from the rows is the whole mechanism.
- **`live_binding` STREAMS** one row at a time, so a 60 000-op group claims cheaply. `page_ops`
  is the only op-row-materializing read, and a test asserts the claim path never calls it.
- **`pending` is the only mutable status.** `repropose_group` guards on it, TEARS UP the acceptance record, and bumps
  `revision`, which an approval must name, so a click on the old version can't approve the new one. The rest are the
  user's, bar `retired` (nobody answered: `DETAILS.md` § Two groups naming one file).
- **Run `recover_interrupted_groups` exactly once per launch, from `agent::start`.** ❌ Not from
  `open_write_connection`, which runs the ladder on every connection open: a sweep there would reclassify a group that
  is genuinely executing.
- **`GroupIntent` makes the wrong shapes unrepresentable.** Each variant carries the target its executor binds AND its
  op shape, so a trash group with a destination can't be built. Reversibility falls out of the same enum (what a VOLUME blocks:
  `../../suggested_ops/facts.rs`). Add a verb by adding a variant, never a check.
- **`completed` is written on SETTLE, whatever the operation did**: it answers "still in flight?". Marking only on
  success leaves a cancelled group to read `interrupted` next launch. Per-op statuses carry the rest.
- **`WritableDestination` stops a group being built to write where a ROUTE serves**: inside an archive, or inside a
  repo's `.git` trees. The engine's refusal would fire AFTER the user approved; here it costs the
  agent a retry.
- **A deselected op keeps its ROW** (`OpStatus::Excluded`). The decision record says what was offered, not only what ran.
- **A server group has zero ops and its own claim.** Its draft is written in the group's own transaction and fails
  closed on read; the revision is its binding. Neither claim takes the other's kind (`NotAFileGroup` /
  `NotAServerGroup`). `DETAILS.md` § A server group.

Service layer above this (selectors, analytics): `../../suggested_ops/CLAUDE.md`.
