<script lang="ts">
    import { asset } from '$app/paths'
    import type { FileEntry, SyncStatus } from '../types'
    import {
        getCachedIcon,
        getCachedCustomFolderIcon,
        getAccentTintedCustomFolderEmoji,
        iconCacheVersion,
        isAccentTintedCustomFolder,
    } from '#lib/icon-cache.js'
    import { stackedIconBase } from '#lib/stacked-icons.js'
    import { glyphIcon } from '#lib/glyph-icons.js'
    import { getIsCmdrGold } from '#lib/settings/reactive-settings.svelte.js'
    import { dependOn } from '#lib/utils/reactivity.js'
    import Icon from '#lib/ui/Icon.svelte'
    import type { IconName } from '#lib/ui/icons/icon-map.js'
    import {
        getSyncIconPath,
        getSyncStatusLabelKey,
        type ImageIndexBadge,
        type RowStatusBadge,
    } from '../views/file-list-utils'
    import ConnectionDot from '../navigation/ConnectionDot.svelte'
    import { tooltip } from '#lib/tooltip/tooltip.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { convention } from '#lib/platform/conventions.js'

    interface Props {
        file: FileEntry
        /** The entry's cloud sync status; drawn as a named badge when it has one. */
        syncStatus?: SyncStatus
        /** Resolved image-index overlay for this row (glyph + tooltip key), or null for none. */
        imageIndexBadge?: ImageIndexBadge | null
        /**
         * Renders the whole glyph stack at half strength, the icon half of the
         * hidden-entry de-emphasis. The caller owns the decision (both list views
         * ask `full-list-utils.ts::isHiddenRowDimmed`), so this component never
         * reads `file.isHidden` itself.
         */
        dimmed?: boolean
        /**
         * The row's status dot (a Servers row's server, a saved share), chosen outside this
         * component (`file-list-utils.ts::rowBadgeFor`). Decorative: the row's accessible name
         * says its words; the tooltip shows them to a pointer.
         */
        statusBadge?: RowStatusBadge | null
    }

    const { file, syncStatus, imageIndexBadge = null, dimmed = false, statusBadge = null }: Props = $props()

    // A Servers row's server: one of the app's own glyphs, never an OS icon (`glyph-icons.ts`).
    const glyph = $derived(glyphIcon(file.iconId))

    // The symlink and sync badges carry meaning the row's text doesn't, so each names itself.
    // The row's accessible name is built from its content, which folds these names into it.
    const syncIcon = $derived(getSyncIconPath(syncStatus))
    const syncLabelKey = $derived(getSyncStatusLabelKey(syncStatus))

    // Subscribe to cache version - this makes getIconUrl reactive
    const _cacheVersion = $derived($iconCacheVersion)

    // A stacked id (a server's share) draws its base's OS icon twice, so everything below
    // (cache lookup, gold tint, cold-cache fallback) works on the base id.
    const stackedBase = $derived(stackedIconBase(file.iconId))
    const baseIconId = $derived(stackedBase ?? file.iconId)

    // A folder the user assigned a Finder custom icon to still carries the generic
    // `dir` iconId (the backend defers that `getxattr` off the bulk-listing hot
    // path), so its icon lands in the cache keyed by PATH instead. Ask for it
    // first: `iconId` alone would draw the generic folder over an icon
    // `prefetchCustomFolderIcons` already fetched and cached.
    // Mirrors that prefetch's own filter — real directories only, symlinks keep
    // their link-badged glyph.
    const customFolderIconUrl = $derived.by((): string | undefined => {
        dependOn(_cacheVersion) // Track cache version for reactivity
        if (!file.isDirectory || file.isSymlink || stackedBase) return undefined
        return getCachedCustomFolderIcon(file.path)
    })

    // Reads `_cacheVersion` itself rather than leaning on `customFolderIconUrl`:
    // that stays `undefined` for ~99% of rows, so a cache update wouldn't
    // propagate through it and the generic icon would never swap in.
    const iconUrl = $derived.by((): string | undefined => {
        dependOn(_cacheVersion) // Track cache version for reactivity
        return customFolderIconUrl ?? getCachedIcon(baseIconId)
    })

    // Which icon ids get the Cmdr-gold recolor filter. All are folders macOS
    // renders as a plain (accent-tinted) folder shape, so the grayscale-first
    // filter re-tints them to gold cleanly regardless of the system accent:
    //   - `dir` / `symlink-dir`: the generic folder (~99% of rows).
    //   - `special:*`: the standard folders macOS badges with a white glyph
    //     (Downloads, Desktop, Documents, Movies, Music, Pictures, Public,
    //     Trash, home). Without this they keep the raw OS bitmap, whose folder
    //     tint is the system accent, so they'd leak through non-gold.
    //     Only where the OS draws them accent-tinted (`standardFolderIcons`): Windows
    //     draws them as full-color artwork, which the filter grays into a plain gold
    //     folder (#1479), so there they keep their own pixels.
    // Deliberately EXCLUDED: `pkg:*` (full-color `.app`/bundle icons the filter
    // would destroy). Custom-icon folders are decided per folder below.
    const tintsStandardFolders = convention('standardFolderIcons') === 'accentTinted'
    const isFolderIcon = $derived(
        baseIconId === 'dir' ||
            baseIconId === 'symlink-dir' ||
            (tintsStandardFolders && baseIconId.startsWith('special:')),
    )
    // A custom-icon folder keeps the `dir` iconId, so `isFolderIcon` is true for it
    // too. Its icon gets the filter only when it's a macOS Customize Folder symbol or
    // emoji that macOS tinted with the system accent (issue #110); pasted artwork and
    // a color the person chose with a tag stay as drawn.
    const customIconFollowsAccent = $derived.by((): boolean => {
        dependOn(_cacheVersion) // Track cache version for reactivity
        return customFolderIconUrl !== undefined && isAccentTintedCustomFolder(file.path)
    })
    const recolorToGold = $derived(
        isFolderIcon && (!customFolderIconUrl || customIconFollowsAccent) && getIsCmdrGold(),
    )
    // The gold filter is grayscale-first, so on an emoji folder it turned the emoji
    // gold-sepia too (#814). Such a folder draws as two layers instead: the plain
    // folder (its own `dir` iconId) under the filter, and the emoji on top, outside it.
    // A symbol is macOS-tinted in the accent's hue, so it recolors cleanly as one bitmap.
    const goldFolderEmoji = $derived(
        recolorToGold && customIconFollowsAccent ? getAccentTintedCustomFolderEmoji(file.path) : undefined,
    )
    const displayedIconUrl = $derived.by((): string | undefined => {
        dependOn(_cacheVersion) // Track cache version for reactivity
        return goldFolderEmoji ? getCachedIcon(baseIconId) : iconUrl
    })

    // Git portal icons resolve in the frontend via Lucide instead of going
    // through the OS icon provider. The four IDs are reserved by the
    // `FileEntry` schema (`git:`-prefixed) and rendered as Lucide
    // components here.
    const isGitIcon = $derived(file.iconId.startsWith('git:'))

    // Maps the reserved `git:`-prefixed icon ids to the shared registry glyph names.
    const gitIconName = $derived.by((): IconName => {
        switch (file.iconId) {
            case 'git:branch':
                return 'git-branch'
            case 'git:tag':
                return 'tag'
            case 'git:commit':
                return 'git-commit-horizontal'
            default:
                return 'git-fork'
        }
    })

    // A restic repo's `Browse snapshots` row: a way into the backup's history,
    // so it reads as that rather than as one more folder. The id is the
    // `cmdr-restic` hub's (`OVERLAY_ROW_ICON_ID`).
    const isResticIcon = $derived(file.iconId === 'restic:snapshots')
</script>

<span class="icon-wrapper" class:is-dimmed={dimmed}>
    {#if isGitIcon}
        <span class="git-icon">
            <Icon name={gitIconName} size={16} />
        </span>
    {:else if isResticIcon}
        <span class="restic-icon">
            <Icon name="history" size={16} />
        </span>
    {:else if glyph}
        <span class="glyph-icon" class:is-machine={glyph.machine}>
            <Icon name={glyph.name} size={16} />
        </span>
    {:else if stackedBase}
        <!-- The base OS icon twice, shrunk, the back copy up and to the right: "folders".
             Decorative like every row icon (the name says what it is). A cold cache draws the
             bundled default the same way until the live icon lands. -->
        {@const layerUrl =
            displayedIconUrl ?? asset(isFolderIcon ? 'icons/default-folder.png' : 'icons/default-file.png')}
        <img class="icon stacked-back" class:gold-folder={recolorToGold} src={layerUrl} alt="" width="16" height="16" />
        <img
            class="icon stacked-front"
            class:gold-folder={recolorToGold}
            src={layerUrl}
            alt=""
            width="16"
            height="16"
        />
    {:else if displayedIconUrl}
        <img class="icon" class:gold-folder={recolorToGold} src={displayedIconUrl} alt="" width="16" height="16" />
    {:else}
        <!-- Cache miss (cold first launch, or briefly after a theme/accent change clears the cache):
             show the bundled macOS default folder/file icon (from `static/icons/default-*.png`,
             extracted from the system GenericFolderIcon/GenericDocumentIcon) so the placeholder is
             always the real OS-shaped icon, never an emoji. It swaps seamlessly to the live
             (accent-tinted) OS icon once `get_icons` populates the cache. -->
        <img
            class="icon"
            class:gold-folder={recolorToGold}
            src={asset(isFolderIcon ? 'icons/default-folder.png' : 'icons/default-file.png')}
            alt=""
            width="16"
            height="16"
        />
    {/if}
    {#if goldFolderEmoji}
        <!-- Decorative: the folder's name says what it is. -->
        <span class="folder-emoji" aria-hidden="true">{goldFolderEmoji}</span>
    {/if}
    {#if file.isSymlink}
        <span
            class="symlink-badge"
            class:has-sync={!!syncIcon}
            role="img"
            aria-label={tString('fileExplorer.fileIcon.symlinkBadge')}><Icon name="link" size={10} /></span
        >
    {/if}
    {#if syncIcon && syncLabelKey}
        <img class="sync-badge" src={syncIcon} alt={tString(syncLabelKey)} width="10" height="10" />
    {/if}
    {#if statusBadge}
        <!-- Decorative for a screen reader (the row's name says the words), a tooltip for a pointer. -->
        <span class="status-badge" aria-hidden="true" use:tooltip={statusBadge.label}
            ><ConnectionDot state={statusBadge.state} explain={false} /></span
        >
    {/if}
    {#if imageIndexBadge}
        <!-- Folder badges carry an already-resolved `tooltip` (interpolated counts); file
             badges carry a static `tooltipKey` resolved here. Exactly one is set. -->
        <span
            class="image-index-badge"
            use:tooltip={imageIndexBadge.tooltip ??
                (imageIndexBadge.tooltipKey ? tString(imageIndexBadge.tooltipKey) : '')}
        >
            <Icon name={imageIndexBadge.icon} size={10} />
        </span>
    {/if}
</span>

<style>
    .icon-wrapper {
        position: relative;
        width: var(--spacing-icon-size);
        height: var(--spacing-icon-size);
        flex-shrink: 0;
    }

    /* The icon half of the hidden-entry de-emphasis. `opacity` rather than a
       color token because these are raster OS icons and badge glyphs, not text:
       nothing here carries a `color` a token could quiet, and no text-contrast
       bar applies to a decorative image whose row already spells the name out.
       It carries the weight the name's own dim can't: `--color-text-quiet` is
       capped by the APCA floor (~5 Lc of slack in dark mode), so the text alone
       reads too close to an ordinary row, especially in light mode.
       `.icon-wrapper.is-dimmed` is listed in `opacity_check.go`'s decorative
       allowlist for exactly that reason; the whole stack fades together so a
       badge never floats at full strength over a faded icon. */
    .icon-wrapper.is-dimmed {
        opacity: 0.5;
    }

    .icon {
        width: var(--spacing-icon-size);
        height: var(--spacing-icon-size);
        object-fit: contain;
    }

    .gold-folder {
        filter: grayscale(1) sepia(1) hue-rotate(3deg) saturate(2.5) brightness(0.95);
    }

    /* A stacked icon (a server's share): two copies at 80%, the back one in the top-right
       corner, the front one in the bottom-left, so the back peeks out up and to the right.
       Percentages, so it follows `--spacing-icon-size` and `--font-scale`. */
    .stacked-back,
    .stacked-front {
        position: absolute;
        width: 80%;
        height: 80%;
    }

    /* The back copy a shade darker, so the two read as two folders, ❌ not one merged shape.
       Under gold it keeps the gold filter, darkened the same. */
    .stacked-back {
        top: 0;
        right: 0;
        filter: brightness(0.8);
    }

    .stacked-back.gold-folder {
        filter: grayscale(1) sepia(1) hue-rotate(3deg) saturate(2.5) brightness(0.76);
    }

    .stacked-front {
        bottom: 0;
        left: 0;
    }

    /* The emoji layer of a gold emoji folder, placed and sized like macOS draws it on the
       folder face: at the 32 px render Cmdr fetches, both put a 🙂 at 16×15 px from (8, 9)
       (verified on macOS 27.0.1 by diffing an `NSWorkspace` render against this CSS in
       Chrome, 2026-10-29). Scales with the icon, so it follows `--font-scale`. The top
       padding nudges the flex-centered glyph down half its own amount. */
    .folder-emoji {
        position: absolute;
        inset: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding-top: calc(var(--spacing-icon-size) / 16);
        font-size: calc(var(--spacing-icon-size) * 0.41);
        line-height: var(--font-line-height-flat);
        pointer-events: none;
    }

    .git-icon {
        display: inline-flex;
        width: var(--spacing-icon-size);
        height: var(--spacing-icon-size);
        align-items: center;
        justify-content: center;
        color: var(--color-git-portal-text);
    }

    /* A Servers row's server glyph. Inherits the row's text color; an SMB server's machine
       wears the server-icon accent, as macOS tints a server's icon. */
    .glyph-icon {
        display: inline-flex;
        width: var(--spacing-icon-size);
        height: var(--spacing-icon-size);
        align-items: center;
        justify-content: center;
    }

    .glyph-icon.is-machine {
        color: var(--color-server-icon);
    }

    /* The status dot, at the icon's bottom-right like the symlink badge, shrunk from the
       switcher's 10 px dot so it doesn't cover the glyph. */
    .status-badge {
        position: absolute;
        bottom: -1px;
        right: -1px;
        display: inline-flex;
        transform: scale(0.7);
        transform-origin: bottom right;
    }

    /* Inherits the row's text color, so it follows selection and dimming like the name. */
    .restic-icon {
        display: inline-flex;
        width: var(--spacing-icon-size);
        height: var(--spacing-icon-size);
        align-items: center;
        justify-content: center;
    }

    .symlink-badge {
        position: absolute;
        bottom: -2px;
        right: -2px;
        display: inline-flex;
        /* Higher-contrast accent (darker than the accent in light mode, lighter in dark mode) so the
           badge stays legible over gold/accent-tinted icons in both schemes. */
        color: var(--color-accent-pop);
    }

    .symlink-badge.has-sync {
        bottom: auto;
        right: auto;
        top: -2px;
        left: -2px;
    }

    .sync-badge {
        position: absolute;
        bottom: -2px;
        right: -2px;
        width: 10px;
        height: 10px;
    }

    /* Image-index overlay: top-right (free corner; bottom-right + top-left are sync/symlink).
       Deliberately quiet — a gray glyph, no background fill — so it never competes with the
       file icon or the sync/symlink badges. */
    .image-index-badge {
        position: absolute;
        top: -2px;
        right: -2px;
        display: inline-flex;
        white-space: nowrap;
        color: var(--color-text-tertiary);
    }
</style>
