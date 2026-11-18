<script lang="ts">
    const {
        value,
        size = 'md',
        ariaLabel,
        ariaValuetext,
        animated = true,
    }: {
        value: number
        size?: 'sm' | 'md'
        ariaLabel?: string
        /** The amount shown beside the bar ("1.2 GB of 3 GB"), so a screen reader says it
         *  too, and not only the percentage. */
        ariaValuetext?: string
        /** Whether the bar is MOVING. The shimmer says "something is happening",
         *  so a bar that's halted (a paused operation) passes `false` and sits
         *  still. Reduced motion drops the sweep regardless. */
        animated?: boolean
    } = $props()

    const percent = $derived(Math.min(100, Math.round(value * 100)))
    const widthPercent = $derived(Math.min(100, value * 100))
</script>

<div
    class="track {size}"
    role="progressbar"
    aria-valuenow={percent}
    aria-valuemin={0}
    aria-valuemax={100}
    aria-label={ariaLabel}
    aria-valuetext={ariaValuetext}
>
    <div class="fill" class:animated style="width: {widthPercent}%"></div>
</div>

<style>
    .track {
        flex: 1;
        min-width: 0;
        background: var(--color-progress-track);
        overflow: hidden;
    }

    /* The Safari 15.4 floor's guard (`scripts/vite-webkit-css-floor.ts`): every engine with contrast
       themes reads it. */
    @supports (forced-color-adjust: none) {
        .track {
            forced-color-adjust: var(--color-forced-adjust);
        }
    }

    .track.sm {
        height: var(--spacing-progress-height-sm);
        border-radius: var(--radius-progress-sm);
    }

    .track.md {
        height: var(--spacing-progress-height-md);
        border-radius: var(--radius-progress-md);
    }

    .fill {
        position: relative;
        height: 100%;
        background: var(--color-progress-fill);
        border-radius: inherit;
        transition: width 0.15s ease-out;
        overflow: hidden;
    }

    /* The sweep is decoration on top of the width, so it goes away entirely
       under reduced motion — and on any bar the caller says isn't moving. */
    @media (prefers-reduced-motion: no-preference) {
        .fill.animated::after {
            content: '';
            position: absolute;
            inset: 0;
            background: linear-gradient(
                45deg,
                transparent 20%,
                color-mix(in oklch, var(--color-accent), white 30%) 50%,
                transparent 80%
            );
            background-size: 200% 100%;
            animation: shimmer 2.5s infinite linear;
        }
    }

    @keyframes shimmer {
        from {
            background-position: 200% 0;
        }
        to {
            background-position: -200% 0;
        }
    }
</style>
