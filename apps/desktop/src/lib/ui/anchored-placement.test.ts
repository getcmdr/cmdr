import { describe, it, expect } from 'vitest'
import { placeAtPoint, placeBesideRow, placeOffAnchor, type PlacementRect } from './anchored-placement'

// A 1000×800 window with the house 8 px margin, so the arithmetic in each case reads plainly.
const VIEWPORT = { width: 1000, height: 800 }
const MARGIN = 8

function rect(left: number, top: number, width: number, height: number): PlacementRect {
  return { left, top, right: left + width, bottom: top + height }
}

describe('placeOffAnchor', () => {
  it('opens below the anchor when the surface fits there, capped to the room below', () => {
    const placed = placeOffAnchor({
      anchor: rect(100, 100, 80, 24),
      size: { width: 220, height: 200 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    expect(placed).toEqual({ left: 100, top: 128, maxHeight: 800 - 8 - 128, side: 'below' })
  })

  it('opens above the anchor when there is no room below but room above', () => {
    // The Multi-rename Presets button: a short anchor about 40 px above the window's bottom edge.
    const placed = placeOffAnchor({
      anchor: rect(20, 740, 90, 28),
      size: { width: 220, height: 120 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    // Its bottom edge sits `gap` above the anchor's top: 740 - 4 - 120.
    expect(placed).toEqual({ left: 20, top: 616, maxHeight: 740 - 4 - 8, side: 'above' })
  })

  it('takes the bigger side, capped to it, when the surface fits on neither', () => {
    const below = placeOffAnchor({
      anchor: rect(100, 300, 80, 20),
      size: { width: 220, height: 1200 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    expect(below).toEqual({ left: 100, top: 324, maxHeight: 800 - 8 - 324, side: 'below' })

    const above = placeOffAnchor({
      anchor: rect(100, 500, 80, 20),
      size: { width: 220, height: 1200 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    // Pinned to the top margin, ending `gap` above the anchor.
    expect(above).toEqual({ left: 100, top: 8, maxHeight: 500 - 4 - 8, side: 'above' })
  })

  it('treats a bottom limit above the window edge as the floor', () => {
    // The volume switcher stops above its pane's footer: 180 px fit the window but not the limit.
    const placed = placeOffAnchor({
      anchor: rect(100, 100, 80, 24),
      size: { width: 220, height: 180 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
      bottomLimit: 300,
    })
    expect(placed).toEqual({ left: 100, top: 128, maxHeight: 300 - 4 - 128, side: 'below' })
  })

  it('opens below a point anchor with no gap', () => {
    const placed = placeOffAnchor({
      anchor: rect(400, 300, 0, 0),
      size: { width: 220, height: 100 },
      viewport: VIEWPORT,
      gap: 0,
      margin: MARGIN,
    })
    expect(placed).toEqual({ left: 400, top: 300, maxHeight: 800 - 8 - 300, side: 'below' })
  })

  it('clamps horizontally inside the viewport on both sides', () => {
    const right = placeOffAnchor({
      anchor: rect(900, 100, 80, 24),
      size: { width: 220, height: 100 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    expect(right.left).toBe(1000 - 220 - 8)

    const left = placeOffAnchor({
      anchor: rect(-30, 100, 80, 24),
      size: { width: 220, height: 100 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    expect(left.left).toBe(8)

    // Wider than the viewport: pinned to the left margin.
    const wide = placeOffAnchor({
      anchor: rect(300, 100, 80, 24),
      size: { width: 1200, height: 100 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    expect(wide.left).toBe(8)
  })

  it('never starts above the top margin for an anchor scrolled above the viewport', () => {
    const placed = placeOffAnchor({
      anchor: rect(100, -60, 80, 24),
      size: { width: 220, height: 100 },
      viewport: VIEWPORT,
      gap: 4,
      margin: MARGIN,
    })
    expect(placed.top).toBe(8)
    expect(placed.side).toBe('below')
  })
})

describe('placeBesideRow', () => {
  const SUBMENU = { gap: 4, overlap: 5, margin: MARGIN, viewport: VIEWPORT }

  it('opens to the right of its parent row, overlapping it a little, when there is room', () => {
    const placed = placeBesideRow({ row: rect(100, 200, 220, 28), size: { width: 220, height: 150 }, ...SUBMENU })
    expect(placed).toEqual({ left: 320 - 5, top: 196, maxHeight: 800 - 16 })
  })

  it('flips to the left when there is no room on the right', () => {
    const placed = placeBesideRow({ row: rect(700, 200, 220, 28), size: { width: 220, height: 150 }, ...SUBMENU })
    // Its right edge overlaps the row's left edge by the same few px.
    expect(placed.left).toBe(700 - 220 + 5)
  })

  it('clamps into the viewport when neither side fits', () => {
    const placed = placeBesideRow({ row: rect(300, 200, 400, 28), size: { width: 600, height: 150 }, ...SUBMENU })
    // Right needs 695 + 600 > 992; left needs 300 + 5 - 600 < 8. The right side has more room, so clamp there.
    expect(placed.left).toBe(1000 - 600 - 8)
  })

  it('slides up so its bottom stays inside the viewport, and never above the top margin', () => {
    const low = placeBesideRow({ row: rect(100, 700, 220, 28), size: { width: 220, height: 300 }, ...SUBMENU })
    expect(low.top).toBe(800 - 8 - 300)

    const tall = placeBesideRow({ row: rect(100, 700, 220, 28), size: { width: 220, height: 2000 }, ...SUBMENU })
    expect(tall.top).toBe(8)
    expect(tall.maxHeight).toBe(800 - 16)
  })
})

describe('placeAtPoint', () => {
  const at = (x: number, y: number, width: number, height: number) =>
    placeAtPoint({ point: { x, y }, size: { width, height }, viewport: VIEWPORT, margin: MARGIN })

  it('opens down and right of the point when it fits, as a context menu does', () => {
    expect(at(100, 100, 220, 300)).toEqual({ left: 100, top: 100, maxHeight: 800 - 8 - 100 })
  })

  it('flips left of the point near the right edge, and up near the bottom', () => {
    const placed = at(900, 700, 220, 300)
    expect(placed.left).toBe(900 - 220)
    expect(placed.top).toBe(700 - 300)
  })

  it('clamps inside the window when neither side fits across', () => {
    expect(at(500, 100, 990, 100).left).toBe(8)
  })

  it('scrolls a menu taller than both sides, on the roomier one', () => {
    const below = at(100, 300, 220, 2000)
    expect(below.top).toBe(300)
    expect(below.maxHeight).toBe(800 - 8 - 300)
    const above = at(100, 600, 220, 1000)
    expect(above.top).toBe(8)
    expect(above.maxHeight).toBe(600 - 8)
  })
})
