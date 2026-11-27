import { describe, expect, it } from 'vitest'
import { loadDialogGallery } from './load-dialog-gallery'

// Vitest runs with `import.meta.env.DEV` set, so this pins the dev side of the gate: the main
// layout gets the real harness. The release side (`null`, and no harness in the output) is a
// build-time fold, which `dialog-gallery/DETAILS.md` § "What actually reaches a production bundle"
// verifies against the built bundle.
describe('loadDialogGallery', () => {
  it('hands a dev build the harness component', async () => {
    const pending = loadDialogGallery()
    expect(pending).not.toBeNull()
    const module = await pending
    expect(module?.default).toBeTypeOf('function')
  })
})
