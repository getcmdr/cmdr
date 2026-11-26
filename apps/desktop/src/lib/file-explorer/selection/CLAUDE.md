# Selection display components

Renders selection state and file metadata in the status bar and list headers. Does NOT manage the selection set (that
lives in `FilePane.svelte` as a `Set<number>`).

## Key files

- **`selection-info-utils.ts`**: pure utilities (size/date formatting, display deciders), no DOM deps, fully tested.
- **`context-menu-target.ts`**: the words and digits the NATIVE context menu's header line shows.
- **`SelectionInfo.svelte`**: status bar below each pane. Four display modes derived from props.
- **`FileIcon.svelte`**: 16x16 icon with overlay badges (fallback below).
- **`SortableHeader.svelte`**: clickable column header with sort-direction triangle and shortcut tooltip.
- **`TagDots.svelte`** + **`tag-dots-utils.ts`**: Finder-tag dot cluster (Name cell, right edge); logic in the util.

## Must-knows

- **`sizeTierClasses` CSS rules live in the consuming view**, ❌ not in `selection-info-utils.ts`: the util is pure
  TypeScript, and `size-bytes` … `size-tb` belong to the list view's stylesheet.
- **Size-tier color tracks the underlying byte magnitude in every mode, not the displayed unit.** A 349-byte file shown
  as `"0.00 MB"` (forced-MB) still tiers as `size-bytes` (green), via `dynamicTierIndex(bytes, format)`. Don't tier off
  the displayed unit.
- **Age-tier mapping and the `appearance.dateColors` palette live with the setting**, in
  `#lib/settings/age-tier-utils.ts`, ❌ not here; the renderer is `#lib/ui/DateLabel.svelte`. See
  `#lib/settings/CLAUDE.md` § "Date display".
- **`isBrokenSymlink` checks `iconId === 'symlink-broken'`, NOT filesystem flags.** The backend already resolves broken-
  symlink status when computing the icon ID; re-checking via stat would be redundant and possibly stale.
- **❌ Never infer "no access" from entry metadata** (`permissions === 0`, missing `size`): every non-local backend
  leaves `permissions` at its `0` default, so the guess fires on EVERY SMB / archive / MTP folder. `DETAILS.md`.
- **The native context menu's header gets ALL its text from HERE, pre-rendered** (`contextMenuCountText`,
  `contextMenuSizeText`): Rust picks the shape, but every number in it needs a locale's separator, plural, and size
  unit, which `menu_t` hasn't got. ❌ `undefined` over a stand-in size (a folder, an unknown size, a selection holding a
  folder). `DETAILS.md`.
- **`SelectionInfo` derives its display mode from props** (`viewMode`, `selectedCount`, `stats`), never an explicit
  `mode` prop. The four modes: `empty`, `selection-summary`, `no-selection` (Full, no selection), `file-info` (Brief, no
  selection).
- **Middle truncation in `file-info` mode uses the `useShortenMiddle` action** (`#lib/utils/`) with `preferBreakAt: '.'`
  and `startRatio: 0.7`, NOT CSS `text-overflow: ellipsis`: CSS truncates from the right and loses the file extension.
- **Counts, size decimals, and triad separators follow the active locale via `#lib/intl`**; ❌ never a hardcoded locale
  or separator. Size tiers come from `TieredSize`, never the (translated) unit text.
- **`SortableHeader`'s shortcut shows only when `isFocused` is true** (the `sort.by*` commands act on the focused pane).
  Its spoken name carries the sort state and any `fallbackNote` (the glyph is `aria-hidden`), always starting with the
  visible label (WCAG 2.5.3). Pinned by `SortableHeader.svelte.test.ts`.
- **`SelectionInfo`'s date is `displayedDateColumn(effectiveSortBy)`** (`../displayed-date.ts`), the date Full would
  show, ❌ never a derivation of its own. `DETAILS.md` § display modes.
- **`FileIcon` resolves a folder by PATH before `iconId`** (`getCachedCustomFolderIcon`): a custom-icon folder keeps the
  generic `dir` id, so gold recolor skips it unless `isAccentTintedCustomFolder`, and draws an emoji folder as the gold
  `dir` plus a `.folder-emoji` layer outside the filter (`src-tauri/src/icons/DETAILS.md`). `special:*` recolors only
  where `convention('standardFolderIcons')` is `accentTinted`: Windows' are full-color (#1479). A cache miss draws the
  bundled macOS default, ❌ not an emoji, then swaps to the live icon on `$iconCacheVersion`. Stacked ids: `DETAILS.md`.

## Status-bar hints (`SelectionInfo`)

The MTP free-space hint, the stale-size hourglass (keyed like Full's size column, `getDirSizeDisplayState`), and the
symlink info glyph by a folder's size: `DETAILS.md`.

Full details (the full `selection-info-utils` export catalog, per-mode conditions, `recursiveSizePending` overlay flow,
size-unit decision rationale, and component props): `DETAILS.md`.
