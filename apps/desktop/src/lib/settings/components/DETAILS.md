# Settings UI primitives details

Depth and rationale for the setting-row primitives. `CLAUDE.md` holds the must-knows that prevent silent breakage.

## Picking a control

Every row is a `SettingRow` (label + description + control + reset pip + restart-required badge, plus `searchQuery`).
Every description line sits in the label column under the label, beside the control (§ Row layout). The registry decides
whether the description shows under the label, behind an ⓘ after the label, or as a summary under the label plus the ⓘ
(`../DETAILS.md` § "Where the description shows"; the rule lives in `description-placement.ts`), so a section only
passes `description`. Two snippets escape the plain shape: `descriptionContent` replaces the description text with
markup (always under the label), and `labelTrailing` lands right after the label, ahead of the pip and the badges, for a
small adornment belonging to the label rather than the control (onboarding's step 4 hangs its own `<InfoTip>` there). A
disabled row says why in one of two ways: `disabledReason`, a short badge beside the label, or `disabledNote`, a full
sentence with an info glyph under the whole row (outside the label column, so the row's dimming doesn't reach it) that
also says how to enable the row ("Sort folders" uses it). The note is a visible line, not a tooltip, because a disabled
control can't take focus, so a tooltip on it is out of reach for keyboard and screen-reader users. A disabled row dims
its children rather than itself, which keeps the note at full AA contrast; point the control's `aria-describedby` at
`disabledNoteId(id)` (`settings-window.ts`, beside `settingAnchorId`) while it's disabled. Pick the inner control by
shape:

- `SettingSwitch`: the primary boolean, wrapping `lib/ui/Switch`.
- `SettingCheckbox`: a secondary boolean, for one hanging off a switch or in a denser layout.
- `SettingSelect`: enum dropdown, wrapping `lib/ui/Select`. It owns the `allowCustom` inline-number flow and its
  `__custom__` sentinel, which `ui/Select` never sees. The optional `onPicked(value)` fires on a COMMITTED pick (click,
  or Enter on a row) and not on the highlight preview `handleHighlightChange` applies while the user moves through the
  list, so one call means one deliberate choice: that's the seam the two language pickers report `language_changed` from
  (`src-tauri/src/analytics/DETAILS.md` § The language events).
- `SettingToggleGroup`: segmented control for a short enum list.
- `SettingRadioGroup`: vertical radio for a longer list, an option needing a `customContent` snippet, or an option
  carrying a control on its own line (`itemTrailing`, as Brief mode's "Limit to" does).
- `SettingSlider` vs `SettingNumberInput`: see the next section, and § Index-mapped stops for the discrete mode. A
  `duration` setting on the number input edits in `constraints.unit` while the store stays in ms (`durationValueToMs` /
  `msToDurationValue`).
- `SettingTextInput`: a registry-backed string field. It writes each edit to the store immediately and follows
  cross-window/reset changes, so a newer value never lives only in a section-local draft. `onValueChange` is for live
  previews and other derived UI; persistence stays inside the primitive.
- `SettingPasswordInput`: masked input with a reveal toggle; two modes, below.
- `SettingColorSwatchPicker`: circle trigger plus a 4×4 swatch popover for pane tints. `swatch-keyboard.ts` is its pure
  key-index resolver, unit-testable without a DOM.

Every `.svelte` here ships a `*.a11y.test.ts` (axe tier-3); the swatch picker and `swatch-keyboard` also have functional
`*.test.ts`.

One setting rendered in two UI locations follows the parent's mirror pattern (`../CLAUDE.md`). The card-group frame
guard has a worked reference in `../sections/DETAILS.md`, under the Notifications section
(`behavior.fileSystemWatching.*`).

## Row layout

`SettingRow` lays every row out the same way, macOS System Settings style: the label column takes what's left, and the
control sits flush against the card's right edge. There's no layout prop; all of it is CSS in `SettingRow.svelte`.

- **Natural size by default.** Switches, pop-up buttons (`SettingSelect`'s wrapper hugs its button), swatch pickers,
  number inputs, and radio lists keep their own width. A radio list moves right as a block; its options stay
  left-aligned inside it.
- **Text fields and sliders take one shared width (`15rem`).** Both fill their container, so they have no natural width
  of their own. The row matches them by the house `.text-field` frame class and the slider thumb's `role="slider"`. A
  radio list carrying a field on one option's line (the custom date format) is excluded: it sizes by its options.
- **One-line controls center** on the label column, so a short control beside a multi-line description sits in its
  middle.
- **A radio list top-aligns**, with the label line given one radio option's line height (`RadioGroup`'s item padding
  around its `--font-size-sm` label), so the label centers on the FIRST option rather than floating between two. The row
  finds it by Ark's `role="radiogroup"`, an ARIA contract rather than a `RadioGroup` class. Changing `RadioGroup`'s item
  padding or label size means changing that `min-height` too.
- **Decision: flush right, no midpoint grid.** A 50/50 grid lines up control left edges, but on a page mixing switches,
  pop-ups, and lists it leaves each control's right edge somewhere different. Flush right is the platform's own pattern
  and keeps one clean edge; toggle groups line up their left edges too (§ Toggle groups share a width).

## Toggle groups share a width

Every `SettingToggleGroup` in one card takes the width of the widest, so their left edges line up as well as their right
ones; the segments share the extra space (`ui/ToggleGroup`'s `stretch`). Pure CSS in `SettingRow.svelte`, no measuring:

- `SettingToggleGroup` wraps its group in a `data-shared-width` element: the marker `SettingRow` looks for.
- A `SectionCard` holding such a row becomes a `minmax(0, 1fr) auto` grid, and every child spans both tracks. Rows with
  a shared-width control turn into subgrids (row, then header), which puts their control in the `auto` track.
- The grid spec keeps any item spanning a flexible track out of the content-sized tracks' sizing, so the `auto` track is
  exactly as wide as the widest toggle group, and the other rows lay out as before.
- A row only joins when it's a DIRECT child of the card's box. A section that wraps rows in its own element opts those
  rows out: they keep their natural width.
- **Decision: per card, not per page.** One long group (file size format's "SI decimal (kB, MB, GB)") would otherwise
  stretch every short one on the page into wide, mostly empty pills.

## Managed rows

A setting the organization's MDM policy pins (`fixed` lock) renders locked wherever its row appears, with nothing passed
from the section. The policy and the store overlay are `../../managed-policy/DETAILS.md`.

- **`SettingRow`** reads `isSettingLocked(id)`: the row disables, its note becomes "Your organization manages this
  setting." (`settings.managed.rowNote`), and any `disabledNote` or `disabledReason` the section passed is dropped, so
  the row carries one reason. The reset pip hides too: a reset is refused anyway, and the stored choice comes back when
  the profile goes away.
- **Every primitive** calls `useSettingLock(id)` (`setting-lock.svelte.ts`): `disabled={disabled || lock.locked}`, and
  `lock.describedBy(own)` as the control's `aria-describedby`, the row's note while locked, else the section's own
  target. `SettingSlider` puts it on the thumb (`thumbProps`), the focusable part. `SettingColorSwatchPicker` has no
  disabled state; no lockable setting uses it.
- **A narrowed setting** (`disallowedValues`, like `ai.provider` under `DisableCloudAI`) stays usable: the row isn't
  disabled, and its own control rules out the values.
- **`SettingsSection`** says "Your organization manages some of these settings." under its title when any row it holds
  is managed, pinned or narrowed. Each `SettingRow` registers its id with the nearest section through context
  (`section-rows.svelte.ts`) while mounted, so the line follows the rows actually rendered and no section keeps a list.
  It's plain text in reading order because native `disabled` takes a control out of the Tab order: a keyboard or
  VoiceOver user meets the reason before the controls.
- Tests: `managed-lock.test.ts` (with `test/fixtures/managed-setting-section-fixture.svelte`).

## Slider vs number input

A registry `component: 'slider'` row is a COARSE choice: `SettingSlider` renders `lib/ui/Slider` with a readout label
and no paired number field, so the value can only be dragged. A row where the user wants to type an exact number uses
`component: 'number-input'` and `SettingNumberInput` instead. ❌ Don't glue a number field back onto the slider: two
controls fighting over one value read as a bug, and the pair costs every slider row more width than it needs.

Practical fallout worth knowing:

- The registry's `sliderStops` feed the slider's ticks AND its magnetic snap targets, so a stop is both visible and
  sticky. A `number-input` row ignores `sliderStops`; it clamps to `min`/`max` and steps by `step`.
- Double-clicking the thumb resets to the registry default. There's no equivalent gesture on the number input; the row's
  reset pip (`SettingRow`) covers it.
- `maxOverride` exists for a ceiling that isn't known until runtime (image-index parallelism, capped at this machine's
  CPU count). The registry keeps a static fallback so search and off-runtime rendering still work.
- A slider's readout joins the value and `unit` with NO space (`125%`), and `ariaValueText` carries the same string so
  screen readers hear the unit too. A row whose number isn't a plain count passes `formatValue` instead (the Ask Cmdr
  wake cadence renders `30s` / `5m` / `2h` through `#lib/units`), and that also turns the spoken value on.

## Index-mapped stops

`constraints.stopsAreDiscrete` makes the stop table the ONLY reachable set of values, and runs the track over the stops'
INDICES instead of over `min`..`max`. The Ask Cmdr wake cadence is the reference row: its stops run 5 seconds to 2
hours, and on a linear track the first three would share a single pixel. In index space every stop gets equal travel,
and `ui/Slider` needs no change for it — `positionOf` is linear over min/max, which is correct in index space, and ticks
and snap targets are consumed in the same space.

- **Two number spaces, four crossings.** The track carries an index, the store carries the stop's value, and
  `slider-stops.ts` (`nearestStopIndex` / `stopAt`) is the only conversion. The four call sites are the `$state` seed,
  the `onSpecificSettingChange` arm, `commit`, and the double-click reset (which starts from the registry default, a
  stored value).
- **The index is never stored.** Reordering the table or inserting a stop would then silently change what every user
  chose. `CLAUDE.md` carries the two guardrails.
- **Placing a stored value goes through `nearestStopIndex`, not `indexOf`.** A value that isn't in the table — a
  hand-edited settings file, or a stop a later build retired — answers `-1` from `indexOf`, which reads as the first
  stop while the store still holds the old number. `SettingSlider.svelte.test.ts` pins both directions.
- **`ariaValueText` is mandatory here, not optional.** Ark hands the raw track value to `getAriaValueText`, so without
  the mapping back a screen reader announces "5" for a five-minute cadence. `SettingSlider` turns the spoken value on
  whenever `unit`, `formatValue`, or discrete mode is in play, rather than on `unit` alone.
- **`maxOverride` is ignored** in this mode: the stop table decides both ends.

## Password-input modes

`SettingPasswordInput` runs in one of two modes based on whether `onchange` is passed:

- **Store-driven (default):** `id` is mandatory and the component reads/writes the settings store directly. Use for
  passwords persisted as plain settings.
- **Controlled:** when both `value` and `onchange` are provided, the component bypasses the store, treats `value` as
  external state, and calls `onchange(newValue)` on every input. Use when the value lives in the OS secret store (AI
  cloud provider API keys) or any other backing store that isn't `settings.json`. In this mode `id` is still required
  for label/aria fallbacks but isn't read or written.

The `$effect` that syncs `internalValue` from `externalValue` is gated on `onchange` being set, and the
`onSpecificSettingChange` subscription is skipped in controlled mode so secret-store updates don't get clobbered by
stale store reads.

## `SettingsSidebar` and `SettingsContent` live here, not a sibling dir

They're paired with the primitives because the settings window has exactly one consumer of each (the `routes/settings`
page). Splitting them into `lib/settings/window/` would add a directory boundary that crosses the same import edges
back. They share the `SettingsSection` wrapper styling and the same registry hooks, so they sit alongside the rest.

## The sidebar to a screen reader

- **One cursor, two focus homes.** ↑/↓ move the selected section from the section list AND from the search field, so
  both name the selected option through `aria-activedescendant`: the list as a `role="listbox"`, the field as a
  `role="combobox"` controlling it. Each id lands a frame after its element takes focus
  (`#lib/ui/deferred-active-descendant.svelte.ts`: VoiceOver reads only the container when the two arrive together).
- **Option ids index the unfiltered order** (`stableIndex`), so a search narrowing the list never renames a section and
  the active descendant doesn't jump to a different id for the same row.
- **Subsections sit in a `role="group"` named for their section.** A listbox may hold only options and groups, and
  `aria-level` isn't allowed on an option, so the group is how "Colors and formats" says it belongs to Appearance.
- **Every option carries `aria-posinset` / `aria-setsize`** over the visible list ↓ walks (subsections included,
  renumbered by a search). Left to itself, VoiceOver counted only the first row it landed on ("2 of 15") and read every
  later row by name alone (verified on macOS 27.0 + VoiceOver, listening run, 2026-10-15).
- **A search says how many sections it left** ("3 sections match", "No settings match") through a `LiveAnnouncer`, after
  the input's 200 ms debounce, so a word typed is one message. Clearing the search says nothing.
