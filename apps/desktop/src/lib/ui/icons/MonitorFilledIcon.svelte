<script lang="ts">
    import type { SVGAttributes } from 'svelte/elements'

    /**
     * A monitor with a filled screen, the way macOS draws an SMB server (a bezel around a
     * darker screen). The outline is `withicons:monitor` from "with icons" by Evergrow (MIT,
     * https://github.com/withevergrow/withicons), credited in
     * `scripts/check/checks/third-party-vendored.json`. Changed: the screen path is also
     * filled, in `currentColor` at reduced opacity, so one color draws both the edge and a
     * dimmer screen in either mode. Spreads `width`/`height`/aria attributes from `Icon` onto
     * the `<svg>`.
     */
    const rest: SVGAttributes<SVGSVGElement> = $props()
</script>

<svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.75"
    stroke-linecap="round"
    stroke-linejoin="round"
    {...rest}
>
    <path
        d="M4 3.5h16a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2"
        fill="currentColor"
        fill-opacity="0.45"
    />
    <path d="M12 16.5V21m-4 0h8" />
</svg>
