<script lang="ts">
    /**
     * The volume chip at the head of the path bar: what the pane's volume is, the badges that
     * say how it's doing, and the control that takes it away. Clicking it opens the switcher,
     * which is `VolumeChooserMenu.svelte` (the house `Menu`, portaled).
     *
     * ❗ It hosts TWO menus off the same chip — the switcher and the favorites menu
     * (`FavoritesMenu.svelte`, ⌃D) — and holds the ONE `openMenu` that says which, with a
     * share list's account menu after the path as its third member (`noteAccountMenu`). Each
     * reports through `onOpenChange`, so whichever opens (a command, a click, a swap from
     * inside another) closes the others from one place.
     *
     * Presentational: it reads the volume list from the shared `volume-store.svelte.ts` and
     * fetches nothing of its own except the containing volume behind the checkmark.
     */
    import { onMount, onDestroy } from 'svelte'
    import { dependOn } from '#lib/utils/reactivity.js'
    import { getVolumes } from '#lib/stores/volume-store.svelte.js'
    import { isVolumeBusy, isVolumeEjecting } from '#lib/stores/volume-busy-store.svelte.js'
    import { isRestricted } from '#lib/stores/restricted-paths-store.svelte.js'
    import { getUseAppIconsForDocuments } from '#lib/settings/reactive-settings.svelte.js'
    import { getCachedIcon, iconCacheVersion, prefetchIcons } from '#lib/icon-cache.js'
    import { tooltip } from '#lib/tooltip/tooltip.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import Icon from '#lib/ui/Icon.svelte'
    import type { PaneId } from '#lib/commands/types.js'
    import type { VolumeChangePayload, VolumeChooserMenuAPI, FavoritesMenuAPI } from '../pane/types'
    import ConnectionDot from './ConnectionDot.svelte'
    import DetachButton from './DetachButton.svelte'
    import DriveIndexBadge from './DriveIndexBadge.svelte'
    import FavoritesMenu from './FavoritesMenu.svelte'
    import ImageIndexDriveBadge from './ImageIndexDriveBadge.svelte'
    import UsbSpeedDot from './UsbSpeedDot.svelte'
    import VolumeChooserMenu from './VolumeChooserMenu.svelte'
    import { connectDirectlyToRow } from './connect-directly-row'
    import { detachControlFor } from './detach-control'
    import { runDetach } from './detach-volume'
    import { createDriveBadges } from './drive-badges.svelte'
    import { isDriveRow } from './drive-index-manager.svelte'
    import { answersNow } from './connection-state'
    import { filesystemLabel } from './filesystem-label'
    import { getIconForVolume } from './volume-grouping'
    import { createBreadcrumbPopupController } from './volume-breadcrumb-handlers.svelte'
    import { paneVolumeOf } from './pane-volume'

    interface Props {
        /** Which pane the chip belongs to, so the favorites menu knows whose switcher ⌥F1 / ⌥F2 means. */
        paneId: PaneId
        volumeId: string
        currentPath: string
        /**
         * What the backend says holds the pane's path, from the pane's ONE
         * `pane-volume-state.svelte.ts` (the status bar's space reads the same), so
         * the chip never runs its own lookup that could disagree with it.
         */
        containingVolumeId: string | null
        onVolumeChange?: (change: VolumeChangePayload) => void
        /** Tab in this pane's open switcher hands it to the other pane. */
        onSwitchPane?: () => void
    }

    const { paneId, volumeId, currentPath, containingVolumeId, onVolumeChange, onSwitchPane }: Props = $props()

    /** The header's menus: the switcher, the favorites menu, and a share list's account menu. */
    type HeaderMenu = 'volumes' | 'favorites' | 'account'

    const volumes = $derived(getVolumes())

    let chipEl: HTMLButtonElement | undefined = $state()
    // The whole chip: the name plus every control beside it. A pointer-down in here belongs to
    // the switcher, so pressing the eject button doesn't close the list it opened.
    let clusterEl: HTMLDivElement | undefined = $state()
    // The two menus that hang off this chip. The pane's commands are the chip's own, which
    // is why this forwards rather than wraps.
    let chooser: VolumeChooserMenuAPI | undefined = $state()
    let favoritesMenu: FavoritesMenuAPI | undefined = $state()

    /**
     * Which menu the chip has open, and the reason the two can never both be. Each menu
     * reports through `onOpenChange`, so ONE place answers it however the menu came up: a
     * command, a click on the chip, or a swap from inside the other one.
     */
    let openMenu = $state<HeaderMenu | null>(null)
    /** Whether a menu hanging off the chip itself is up (the account menu has its own button). */
    const chipMenuOpen = $derived(openMenu === 'volumes' || openMenu === 'favorites')
    /** Closes a share list's account menu (`ShareListAccountChip.svelte`), which reports in through `noteAccountMenu`. */
    let closeAccountMenu: (() => void) | null = null

    function handleMenuOpenChange(which: HeaderMenu, open: boolean): void {
        if (open) {
            openMenu = which
            // Close the others AFTER claiming the slot: their own close notifications then
            // see a slot that isn't theirs and leave it alone.
            if (which !== 'favorites') favoritesMenu?.close()
            if (which !== 'volumes') chooser?.close()
            if (which !== 'account') closeAccountMenu?.()
        } else if (openMenu === which) {
            openMenu = null
        }
    }

    /**
     * A share list's account menu, the third member of the ONE `openMenu`. It lives after the
     * path (the pane renders it), so it reports in here rather than hanging off the chip.
     */
    export function noteAccountMenu(change: { open: boolean; close: () => void }): void {
        closeAccountMenu = change.close
        handleMenuOpenChange('account', change.open)
    }


    // Breadcrumb inline popup state (for the yellow os_mount indicator).
    const breadcrumbPopup = createBreadcrumbPopupController()
    let breadcrumbPopupRef: HTMLButtonElement | undefined = $state()

    // The volume the pane is on: `pane-volume.ts` says which of its two answers wins.
    // Special case: 'network' is a virtual volume, not from the backend.
    const currentVolume = $derived(
        volumeId === 'network'
            ? { id: 'network', name: tString('fileExplorer.navigation.networkVolume'), path: 'smb://', category: 'network' as const, isEjectable: false }
            : volumeId === 'search-results'
              ? {
                    // R3 B6: the volume selector reads "Search results", a
                    // generic noun matching every other volume's slot. The
                    // search-specific label (the AI title / pattern) moved to
                    // the path slot in `FilePane.svelte::breadcrumbDisplayPath`.
                    id: 'search-results',
                    name: tString('fileExplorer.navigation.searchResultsVolume'),
                    path: 'search-results://',
                    category: 'network' as const,
                    isEjectable: false,
                }
              : paneVolumeOf(volumes, volumeId, currentPath, containingVolumeId),
    )

    /**
     * The snapshot's friendly label is rendered as the trailing path text
     * (in `FilePane.svelte`'s `breadcrumbDisplayPath`). The volume selector
     * here reads the static "Search results" label so the volume-selector
     * slot describes the KIND of volume (matching every other volume:
     * "Network", "Macintosh HD", an MTP device name) and the path slot
     * carries the QUERY-specific label. Don't invert these (label in the
     * volume slot, empty path).
     */
    const currentVolumeName = $derived(currentVolume?.name ?? tString('fileExplorer.navigation.volumeFallback'))
    /** Filesystem name for the closed-picker tooltip; null for non-real-FS volumes (shown as no tooltip). */
    const currentVolumeFsLabel = $derived(currentVolume ? filesystemLabel(currentVolume) : null)
    const currentVolumeIcon = $derived(getIconForVolume(currentVolume))

    // Generic macOS folder icon used as fallback when a volume has no icon (for example,
    // FDA-gated favorites whose icons aren't fetched yet to avoid TCC popups). The `dir`
    // icon is sampled from `~`, which isn't TCC-protected, so prefetching is always safe.
    // Reading `$iconCacheVersion` re-evaluates the derived value once the icon lands.
    const dirIconFallback = $derived.by(() => {
        dependOn($iconCacheVersion)
        return getCachedIcon('dir')
    })

    // The two index dots, in both placements: this chip's (the active drive) and the
    // switcher's rows. One instance, so the fetches and event subscriptions happen once.
    const badges = createDriveBadges({
        getVolumes: () => volumes,
        getActiveVolume: () => currentVolume,
    })

    /** Exported for keyboard shortcut access from parent. */
    export function toggleVolumeChooser() {
        chooser?.toggle()
    }
    export function openVolumeChooser() {
        chooser?.open()
    }
    export function toggleFavoritesMenu() {
        favoritesMenu?.toggle('command')
    }
    /** Whether ANY header menu is up: what suppresses the panes' keys behind it. */
    export function isHeaderMenuOpen(): boolean {
        return openMenu !== null
    }
    export function closeHeaderMenu() {
        chooser?.close()
        favoritesMenu?.close()
        closeAccountMenu?.()
    }

    /** The favorites menu takes the header: ⌃D typed inside the open switcher. */
    function showFavorites() {
        favoritesMenu?.open('command')
    }

    function handleBreadcrumbPopupClickOutside(event: MouseEvent) {
        if (breadcrumbPopupRef && !breadcrumbPopupRef.contains(event.target as Node)) {
            breadcrumbPopup.close()
        }
    }

    function handleBreadcrumbPopupKeyDown(event: KeyboardEvent) {
        if (event.key === 'Escape' && breadcrumbPopup.isOpen) {
            breadcrumbPopup.close()
        }
    }

    onMount(() => {
        // Make sure the generic dir icon is cached for the fallback below.
        if (!getCachedIcon('dir')) {
            void prefetchIcons(['dir'], getUseAppIconsForDocuments())
        }

        document.addEventListener('click', handleBreadcrumbPopupClickOutside)
        document.addEventListener('keydown', handleBreadcrumbPopupKeyDown)
    })

    onDestroy(() => {
        badges.destroy()
        document.removeEventListener('click', handleBreadcrumbPopupClickOutside)
        document.removeEventListener('keydown', handleBreadcrumbPopupKeyDown)
    })
</script>

<div class="volume-breadcrumb" bind:this={clusterEl}>
    <!-- A real button, so VoiceOver finds it and says it opens a menu. A click doesn't focus a
         button in WKWebView, so pointer use leaves focus where it was. -->
    <button
        type="button"
        class="volume-name borderless-control"
        class:is-open={chipMenuOpen}
        bind:this={chipEl}
        aria-haspopup="menu"
        aria-expanded={chipMenuOpen}
        aria-label={tString('fileExplorer.navigation.volumeChooserButtonLabel', { name: currentVolumeName })}
        use:tooltip={currentVolumeFsLabel ?? ''}
        onclick={toggleVolumeChooser}
    >
        {#if currentVolume && isRestricted(currentVolume.path) && dirIconFallback}
            <!-- TCC-denied paths: `NSWorkspace.iconForFile` returns a confusing "no
                 access" placeholder. Use the generic Aqua folder icon instead. -->
            <img class="icon" src={dirIconFallback} alt="" />
        {:else if currentVolumeIcon}
            <img class="icon" src={currentVolumeIcon} alt="" />
        {:else if volumeId === 'network'}
            <span class="icon-emoji"><Icon name="globe" size={16} aria-hidden="true" /></span>
        {:else if dirIconFallback}
            <img class="icon" src={dirIconFallback} alt="" />
        {/if}
        {currentVolumeName}
        {#if currentVolume?.mountIsReadOnly}
            <span class="read-only-indicator" use:tooltip={tString('fileExplorer.navigation.readOnlyTooltip')}><Icon name="lock" size={14} aria-hidden="true" /></span>
        {/if}
        <span class="chevron"></span>
    </button>
    {#if currentVolume?.usbSpeed}
        <UsbSpeedDot speed={currentVolume.usbSpeed} breadcrumb />
    {/if}
    <!-- The chip's dot only ever shows a LIVE session (the pane's connect views own the
         other states), so it earns its place only where the OS mount is the other way in
         (SMB). On any other server it could only say "connected" about what you're browsing. -->
    {#if currentVolume?.connectionState === 'direct'}
        {#if currentVolume.capabilities?.hasOsMountFallback}
            <ConnectionDot state="direct" hasOsMountFallback breadcrumb />
        {/if}
    {:else if currentVolume?.connectionState === 'os_mount'}
        <button
            type="button"
            class="breadcrumb-options-trigger borderless-control"
            class:is-open={breadcrumbPopup.isOpen}
            bind:this={breadcrumbPopupRef}
            aria-expanded={breadcrumbPopup.isOpen}
            aria-label={tString('fileExplorer.navigation.volumeOptionsTooltip')}
            use:tooltip={tString('fileExplorer.navigation.volumeOptionsTooltip')}
            onclick={(e: MouseEvent) => {
                e.stopPropagation()
                closeHeaderMenu()
                breadcrumbPopup.toggle()
            }}
        >
            <!-- The trigger carries the sentence, so the dot inside it stays quiet. -->
            <ConnectionDot state="os_mount" explain={false} />
            <span class="chevron"></span>
        </button>
        {#if breadcrumbPopup.isOpen}
            <div class="breadcrumb-popup">
                <button
                    type="button"
                    class="breadcrumb-popup-item"
                    onclick={(e: MouseEvent) => {
                        e.stopPropagation()
                        breadcrumbPopup.close()
                        void connectDirectlyToRow(currentVolume.id, volumes)
                    }}
                >
                    {tString('fileExplorer.navigation.connectDirectly')}
                </button>
            </div>
        {/if}
    {/if}
    {#if currentVolume && isDriveRow(currentVolume)}
        {@const activeIndexStatus = badges.statusFor(currentVolume.id)}
        {#if activeIndexStatus}
            <DriveIndexBadge
                volumeId={currentVolume.id}
                status={activeIndexStatus}
                driveName={currentVolume.name}
                answers={answersNow(currentVolume)}
                breadcrumb
                onAction={badges.runAction}
            />
        {/if}
        {@const activeImageState = badges.imageStateFor(currentVolume.id)}
        {#if activeImageState}
            <ImageIndexDriveBadge volumeId={currentVolume.id} volumeState={activeImageState} breadcrumb />
        {/if}
    {/if}
    {#if currentVolume}
        <!-- ❗ The same answer the switcher rows render, so the chip can't offer an Eject
             for a server the backend can only refuse. `detach-control.ts` has the story. -->
        {@const detach = detachControlFor(currentVolume, {
            busy: isVolumeBusy(currentVolume.id),
            ejecting: isVolumeEjecting(currentVolume.id),
        })}
        {#if detach}
            <DetachButton
                {...detach.button}
                breadcrumb
                onclick={() => {
                    breadcrumbPopup.close()
                    void runDetach(currentVolume, detach.action)
                }}
            />
        {/if}
    {/if}

    <VolumeChooserMenu
        bind:this={chooser}
        {paneId}
        {volumeId}
        {currentPath}
        containingVolumeId={currentVolume?.id ?? containingVolumeId}
        {badges}
        {onVolumeChange}
        getAnchor={() => chipEl}
        getChipCluster={() => clusterEl}
        onSwitchPane={() => onSwitchPane?.()}
        onShowFavorites={showFavorites}
        onOpenChange={(open: boolean) => { handleMenuOpenChange('volumes', open) }}
    />
    <FavoritesMenu
        bind:this={favoritesMenu}
        {paneId}
        {volumeId}
        {currentPath}
        {onVolumeChange}
        getAnchor={() => chipEl}
        getChipCluster={() => clusterEl}
        onShowVolumes={() => { chooser?.open() }}
        onSwitchPane={() => onSwitchPane?.()}
        onOpenChange={(open: boolean) => { handleMenuOpenChange('favorites', open) }}
    />
</div>

<span class="path-separator">▸</span>

<style>
    .volume-breadcrumb {
        position: relative;
        display: inline-flex;
        align-items: center;
    }

    /* Geometry mirrors a file row so this strip lines up with the list below it.
       Reading left to right from the pane's edge: `.header`'s `--spacing-sm` inset,
       this element's `--spacing-xs` padding, then the icon — 12px, exactly where a
       row's icon starts (`.listbox-region`'s `--spacing-xs` gutter plus
       `.file-entry`'s `--spacing-sm` padding). The `--spacing-sm` gap after the icon
       column. The extra gap lives on the icon's margin, not on this `gap`, so the
       trailing chevron and read-only badge keep their tighter spacing. Change any of
       those four values and this drifts. */
    /* A `<button>` that must look like the `<span>` it was: no UA font, margin, or border. */
    .volume-name {
        font: inherit;
        margin: 0;
        border: none;
        cursor: default;
        font-weight: 500;
        color: var(--color-text-primary);
        padding: var(--spacing-xxs) var(--spacing-xs);
        border-radius: var(--radius-sm);
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        transition: background-color var(--transition-fast);
    }

    .volume-name:hover {
        background-color: var(--color-bg-tertiary);
    }

    .volume-name.is-open {
        background-color: var(--color-bg-tertiary);
    }

    /* Same box as a file icon in the pane (`FileIcon` uses this token too), so the
       volume icon and the icons in the list below read as one column. The trailing
       margin tops the row's `--spacing-xs` gap up to the `--spacing-sm` that
       `.file-entry` puts between its icon and name, landing the label on the Name
       column without loosening the chevron and badges after it. */
    .icon,
    .icon-emoji {
        flex-shrink: 0;
        width: var(--spacing-icon-size);
        height: var(--spacing-icon-size);
        margin-inline-end: var(--spacing-xs);
        object-fit: contain;
    }

    .icon-emoji {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        color: var(--color-text-secondary);
    }

    .chevron {
        /* CSS triangle: consistent size across fonts. Uses currentcolor
           so the parent element controls the color via hover/active states. */
        display: inline-block;
        width: 0;
        height: 0;
        border-left: 4px solid transparent;
        border-right: 4px solid transparent;
        border-top: 5px solid currentcolor;
        vertical-align: middle;
        color: var(--color-text-tertiary);
    }

    .volume-name:hover .chevron,
    .volume-name.is-open .chevron,
    .breadcrumb-options-trigger:hover .chevron,
    .breadcrumb-options-trigger.is-open .chevron {
        color: var(--color-text-primary);
    }

    .path-separator {
        color: var(--color-text-tertiary);
        margin: 0 var(--spacing-xs);
        font-size: var(--font-size-xs);
    }

    .read-only-indicator {
        display: inline-flex;
        align-items: center;
        opacity: 0.7;
    }

    /* ── Breadcrumb inline popup ───────────────────────────────── */

    .breadcrumb-options-trigger {
        font: inherit;
        border: none;
        color: var(--color-text-tertiary);
        cursor: default;
        padding: var(--spacing-xxs) var(--spacing-xs);
        border-radius: var(--radius-sm);
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        margin-left: var(--spacing-xxs);
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast);
    }

    .breadcrumb-options-trigger:hover,
    .breadcrumb-options-trigger.is-open {
        color: var(--color-text-primary);
        background-color: var(--color-bg-tertiary);
    }

    .breadcrumb-popup {
        position: absolute;
        top: 100%;
        left: 0;
        margin-top: var(--spacing-xs);
        min-width: 220px;
        /* Frosted-glass material: shared tokens with the tooltip / menu surface so the whole
           app reads as one glass. See `app.css` § Frosted-glass material. The fill and
           blur tokens flip to opaque / none when reduce-transparency is active. */
        background: var(--color-bg-glass);
        -webkit-backdrop-filter: var(--glass-backdrop);
        backdrop-filter: var(--glass-backdrop);
        border: 0.5px solid var(--color-border-glass);
        border-radius: var(--radius-menu);
        box-shadow: var(--shadow-glass), var(--shadow-glass-rim);
        z-index: var(--z-dropdown);
        padding: var(--spacing-xs) 0;
    }

    .breadcrumb-popup-item {
        display: block;
        width: 100%;
        margin: 0;
        border: none;
        font: inherit;
        color: inherit;
        text-align: start;
        padding: var(--spacing-sm) var(--spacing-md);
        cursor: default;
        white-space: nowrap;
    }

    .breadcrumb-popup-item:hover {
        background-color: var(--color-accent-subtle);
    }

</style>
