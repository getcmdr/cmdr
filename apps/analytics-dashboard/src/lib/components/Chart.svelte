<script lang="ts">
    import { onMount } from 'svelte'
    import uPlot from 'uplot'
    import 'uplot/dist/uPlot.min.css'

    interface Props {
        /** Array of [timestamps, ...valueSeries] in uPlot format. Timestamps are unix seconds. */
        data: uPlot.AlignedData
        /** Series labels (excluding the timestamp series). */
        labels?: string[]
        /** Custom stroke colors per series. Falls back to gold/grey. */
        colors?: string[]
        /** Chart height in pixels. */
        height?: number
        /** X-axis min (unix seconds); with `xMax`, fixes the window. null = the data's span. */
        xMin?: number | null
        /** X-axis max (unix seconds). */
        xMax?: number | null
    }

    // The chart never reacts to the mouse wheel: scroll-to-zoom hijacked page scrolling (David's call
    // 2026-11-19). Hover tooltips stay.
    const { data, labels = [], colors = [], height = 200, xMin = null, xMax = null }: Props = $props()

    let container: HTMLDivElement
    let chart: uPlot | null = null

    const defaultColors = ['#ffc206', '#a1a1aa', '#8b5cf6', '#10b981']

    function colorAt(i: number): string {
        // `.at()` rather than `[i]`: the index can run past either array, and only `.at()` says so in the type.
        return colors.at(i) ?? defaultColors.at(i) ?? '#a1a1aa'
    }

    function buildOpts(width: number): uPlot.Options {
        const series: uPlot.Series[] = [
            {}, // timestamp series (x-axis)
            ...labels.map((label, i) => ({
                label,
                stroke: colorAt(i),
                width: 2,
                fill: colorAt(i) + '14', // ~8% opacity
            })),
        ]

        // If no labels provided, add a default series
        if (labels.length === 0 && data.length > 1) {
            for (let i = 1; i < data.length; i++) {
                const c = colorAt(i - 1)
                series.push({
                    stroke: c,
                    width: 2,
                    fill: c + '14',
                })
            }
        }

        return {
            width,
            height,
            series,
            axes: [
                {
                    stroke: '#a1a1aa',
                    grid: { stroke: 'rgba(46, 45, 42, 0.6)', width: 1 },
                    ticks: { stroke: 'rgba(46, 45, 42, 0.6)', width: 1 },
                    font: '11px -apple-system, system-ui, sans-serif',
                },
                {
                    stroke: '#a1a1aa',
                    grid: { stroke: 'rgba(46, 45, 42, 0.6)', width: 1 },
                    ticks: { stroke: 'rgba(46, 45, 42, 0.6)', width: 1 },
                    font: '11px -apple-system, system-ui, sans-serif',
                    size: 50,
                },
            ],
            cursor: {
                drag: { x: false, y: false },
            },
            legend: {
                show: labels.length > 1,
            },
            padding: [8, 8, 0, 0],
        }
    }

    function applyXScale() {
        if (!chart) return
        if (xMin != null && xMax != null) {
            chart.setScale('x', { min: xMin, max: xMax })
        } else if (data[0].length > 0) {
            chart.setScale('x', { min: data[0][0], max: data[0][data[0].length - 1] })
        }
    }

    function createChart() {
        if (data[0].length === 0) return
        chart?.destroy()
        const opts = buildOpts(container.clientWidth)
        chart = new uPlot(opts, data, container)
        applyXScale()
    }

    onMount(() => {
        const resizeObserver = new ResizeObserver((entries) => {
            for (const entry of entries) {
                if (chart) {
                    chart.setSize({ width: entry.contentRect.width, height })
                }
            }
        })
        resizeObserver.observe(container)

        return () => {
            resizeObserver.disconnect()
            chart?.destroy()
            chart = null
        }
    })

    // Recreate chart when data changes. The bare read registers `data` as this effect's
    // reactive dependency; `no-meaningless-void-operator` doesn't know Svelte gives `void` a
    // second job here.
    $effect(() => {
        // eslint-disable-next-line @typescript-eslint/no-meaningless-void-operator
        void data
        createChart()
    })

    // Re-apply the window when the caller moves it. Same reactive-dependency reasoning as above.
    $effect(() => {
        // eslint-disable-next-line @typescript-eslint/no-meaningless-void-operator
        void xMin
        // eslint-disable-next-line @typescript-eslint/no-meaningless-void-operator
        void xMax
        applyXScale()
    })
</script>

<div bind:this={container} class="w-full"></div>
