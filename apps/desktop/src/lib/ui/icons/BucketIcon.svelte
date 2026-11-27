<script lang="ts">
    import type { SVGAttributes } from 'svelte/elements'

    /**
     * A bucket, for an S3 bucket's row in its account's bucket list. `andrsrxn:bucket` from
     * "andrsrxn/icons" by Andrés Raxón (MIT, https://github.com/andrsrxn/icons), credited in
     * `scripts/check/checks/third-party-vendored.json`. As published, apart from this comment
     * and its `<g>` wrapper's `fill="none"` moving onto the `<svg>`. Spreads
     * `width`/`height`/aria attributes from `Icon` onto the `<svg>`.
     */
    const rest: SVGAttributes<SVGSVGElement> = $props()
</script>

<svg viewBox="0 0 24 24" fill="none" {...rest}>
    <path
        fill="currentColor"
        d="M17.51 3.67c1.72.42 2.58.62 2.71 1.23c.14.6-.55 1.16-1.93 2.27l-.65.52c-.27.22-.4.33-.57.39s-.33.05-.68.05H7.61c-.34 0-.5 0-.67-.05a3 3 0 0 1-.56-.37l-.72-.57c-1.4-1.1-2.1-1.64-1.97-2.25s.99-.83 2.7-1.26l4.88-1.24c.24-.06.36-.09.48-.09c.13 0 .24.03.48.09z"
        opacity=".2"
    />
    <path
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="1.5"
        d="m19.85 5.49l-2.02 11.14c-.31 1.72-.47 2.57-.88 3.23a4 4 0 0 1-1.89 1.58c-.72.29-1.6.29-3.34.29c-1.75 0-2.63 0-3.35-.3a4 4 0 0 1-1.89-1.58c-.41-.66-.56-1.53-.87-3.25L3.66 5.49"
    />
    <path
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="1.5"
        d="M11.25 12.54c2.52 1.95 5.4 3.24 8.32 2.91c2.69-.3 2.81-2.9-.61-4.44M3.66 5.49c0 .83.88 1.58 2.3 2.13a17 17 0 0 0 5.8.91c4.46 0 8.09-1.36 8.09-3.04c0-1.69-3.63-3.05-8.1-3.05s-8.1 1.36-8.1 3.05"
    />
    <path
        fill="currentColor"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="1.5"
        d="M12.01 12.65a.76.76 0 1 1-1.51 0a.76.76 0 0 1 1.51 0"
    />
</svg>
