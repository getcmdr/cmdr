<script lang="ts">
    /**
     * The command menus (`MenuBar`, `ContextMenu`) in this window's look: the house look, or
     * Windows 11's under `CMDR_MOCK_CONVENTIONS=windows pnpm dev`, for comparing side by side with
     * Notepad and File Explorer. Three sources: fixed rows (picks only logged), the app's own bar
     * as the backend answers it (`get_menu_bar`, live, picks RUN in the main window), and the
     * viewer's bar from toggles. Plus a fixed and a live right-click menu.
     */
    import { onDestroy, onMount } from 'svelte'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import MenuBar from '#lib/ui/MenuBar.svelte'
    import ContextMenu from '#lib/ui/ContextMenu.svelte'
    import { createContextMenu } from '#lib/ui/command-menu/context-menu.svelte.js'
    import Checkbox from '#lib/ui/Checkbox.svelte'
    import type { BarMenuView, Row } from '#lib/ipc/bindings.js'
    import {
        followMenuBar,
        getViewerMenuBar,
        openContextMenuInPage,
        pickContextMenuItem,
        pickMenuBarItem,
    } from '#lib/tauri-commands/index.js'
    import DemoAnchor from '../DemoAnchor.svelte'

    const item = (id: string, text: string, shortcut: string | null = null, enabled = true): Row => ({
        kind: 'item',
        id,
        label: { text },
        shortcut,
        ariaKeyshortcuts: shortcut,
        enabled,
    })

    const check = (id: string, text: string, checked: boolean, checkKind: 'checkbox' | 'radio'): Row => ({
        kind: 'check',
        id,
        label: { text },
        shortcut: null,
        ariaKeyshortcuts: null,
        enabled: true,
        checked,
        checkKind,
    })

    /** Every row kind and state, in Notepad's own menu names, so the shapes compare one to one. */
    const FIXTURE: BarMenuView[] = [
        {
            id: 'menu_file',
            title: { text: 'File' },
            rows: [
                item('new', 'New tab', 'Ctrl+N'),
                item('open', 'Open', 'Ctrl+O'),
                item('save', 'Save', 'Ctrl+S', false),
                { kind: 'separator' },
                item('exit', 'Exit'),
            ],
        },
        {
            id: 'menu_edit',
            title: { text: 'Edit' },
            rows: [item('undo', 'Undo', 'Ctrl+Z', false), item('cut', 'Cut', 'Ctrl+X'), item('copy', 'Copy', 'Ctrl+C')],
        },
        {
            id: 'menu_view',
            title: { text: 'View' },
            rows: [
                check('status_bar', 'Status bar', true, 'checkbox'),
                check('word_wrap', 'Word wrap', false, 'checkbox'),
                {
                    kind: 'submenu',
                    id: 'zoom',
                    label: { text: 'Zoom' },
                    enabled: true,
                    rows: [item('zoom_in', 'Zoom in', 'Ctrl+Plus'), item('zoom_out', 'Zoom out', 'Ctrl+Minus')],
                },
                { kind: 'separator' },
                check('view_full', 'Full view', true, 'radio'),
                check('view_brief', 'Brief view', false, 'radio'),
            ],
        },
    ]

    const CONTEXT_FIXTURE: Row[] = [
        { kind: 'header', label: { text: 'report.pdf' } },
        item('open', 'Open', 'Enter'),
        item('copy', 'Copy', 'F5'),
        item('rename', 'Rename', 'Shift+F6', false),
        { kind: 'separator' },
        {
            kind: 'submenu',
            id: 'sort',
            label: { text: 'Sort by' },
            enabled: true,
            rows: [check('sort_name', 'Name', true, 'radio'), check('sort_size', 'Size', false, 'radio')],
        },
        item('delete', 'Delete', 'F8'),
    ]

    let lastPick = $state<string | null>(null)
    let liveMenus = $state<BarMenuView[]>([])
    let viewerMenus = $state<BarMenuView[]>([])
    const viewerInputs = $state({ wordWrap: false, canGoBack: true, canGoForward: false, searchFocused: false })
    const VIEWER_TOGGLES = [
        { key: 'wordWrap', label: 'Word wrap' },
        { key: 'canGoBack', label: 'Can go back' },
        { key: 'canGoForward', label: 'Can go forward' },
        { key: 'searchFocused', label: 'Search box focused' },
    ] as const
    let liveToken = 0

    const fixtureMenu = createContextMenu({
        onPick: (id) => {
            lastPick = `Context menu: ${id}`
        },
    })
    const liveMenu = createContextMenu({
        onPick: (id) => {
            void pickContextMenuItem(liveToken, id).then((outcome) => {
                lastPick = `Function key bar menu: ${id} (${outcome})`
            })
        },
    })

    let stopLive: (() => void) | undefined
    onMount(() => {
        void followMenuBar((menus) => {
            liveMenus = menus
        }).then((stop) => {
            stopLive = stop
        })
    })
    onDestroy(() => stopLive?.())

    $effect(() => {
        const inputs = { ...viewerInputs }
        void getViewerMenuBar(inputs).then((menus) => {
            viewerMenus = menus
        })
    })

    async function openLiveContextMenu(event: MouseEvent): Promise<void> {
        const answer = await openContextMenuInPage({ menu: 'functionKeyBar' })
        liveToken = answer.token
        liveMenu.open({ rows: answer.rows, at: { x: event.clientX, y: event.clientY }, fromKeyboard: event.detail === 0 })
    }
</script>

<SectionCard id="components-command-menus" label="Menu bar and context menus">
    <div class="cell">
        <p class="caption">
            Fixed rows (picks are only logged). Click a title, or press Alt or F10 and walk with the arrows; Alt plus
            an underlined letter opens a menu.
        </p>
        <div class="bar-frame">
            <MenuBar
                menus={FIXTURE}
                ariaLabel="Fixture menu bar"
                onPick={(id: string) => {
                    lastPick = `Fixture bar: ${id}`
                }}
            />
        </div>
        {#if lastPick}<p class="caption">Last pick: {lastPick}</p>{/if}
    </div>

    <div class="cell">
        <p class="caption">
            The app's own bar, as the backend answers it and keeps it live. A pick runs its command in the main window.
        </p>
        <div class="bar-frame">
            <MenuBar
                menus={liveMenus}
                ariaLabel="Live menu bar"
                takesWindowKeys={false}
                onPick={(id: string) => void pickMenuBarItem(id)}
            />
        </div>
    </div>

    <div class="cell">
        <p class="caption">
            The viewer's bar, from its own state (picks are only logged). The last two bars take no Alt or F10, so the
            first one answers those; click a title to walk these.
        </p>
        <div class="toggles">
            {#each VIEWER_TOGGLES as toggle (toggle.key)}
                <Checkbox
                    checked={viewerInputs[toggle.key]}
                    onCheckedChange={(checked: boolean) => {
                        viewerInputs[toggle.key] = checked
                    }}>{toggle.label}</Checkbox
                >
            {/each}
        </div>
        <div class="bar-frame">
            <MenuBar
                menus={viewerMenus}
                ariaLabel="Viewer menu bar"
                keyScope="Viewer window"
                takesWindowKeys={false}
                onPick={(id: string) => {
                    lastPick = `Viewer bar: ${id}`
                }}
            />
        </div>
    </div>

    <div class="cell">
        <p class="caption">
            Right-click menus. The live one is the function key bar's, from the backend; its pick runs.
        </p>
        <div class="anchors">
            <DemoAnchor
                onclick={(event: MouseEvent) => {
                    fixtureMenu.open({
                        rows: CONTEXT_FIXTURE,
                        at: { x: event.clientX, y: event.clientY },
                        fromKeyboard: event.detail === 0,
                    })
                }}>Open a context menu</DemoAnchor
            >
            <DemoAnchor
                onclick={(event: MouseEvent) => {
                    void openLiveContextMenu(event)
                }}>Open the function key bar's menu</DemoAnchor
            >
        </div>
        <ContextMenu menu={fixtureMenu} />
        <ContextMenu menu={liveMenu} />
    </div>
</SectionCard>

<style>
    .caption {
        margin: 0 0 var(--spacing-sm);
        font-size: var(--font-size-xs);
        color: var(--color-text-tertiary);
    }

    /* A strip of window behind the bar, so its transparent background reads as a window's. */
    .bar-frame {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-sm);
        background: var(--color-bg-primary);
    }

    .toggles,
    .anchors {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-md);
        margin-bottom: var(--spacing-sm);
    }
</style>
