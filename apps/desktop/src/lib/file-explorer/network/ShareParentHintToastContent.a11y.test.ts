/**
 * Tier 3 a11y tests for `ShareParentHintToastContent.svelte`: the one-time explainer
 * for `..` out of an SMB share's top folder, a sentence and three buttons.
 */

import { describe, it, vi } from 'vitest'
import { mount, tick } from 'svelte'
import ShareParentHintToastContent from './ShareParentHintToastContent.svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

vi.mock('#lib/ui/toast/index.js', () => ({ dismissToast: vi.fn() }))
vi.mock('./share-parent-row.svelte', () => ({
  setShareParentRow: vi.fn(),
  turnOffShareParentRowForAll: vi.fn(),
}))

describe('ShareParentHintToastContent a11y', () => {
  it('has no a11y violations', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(ShareParentHintToastContent, {
      target,
      props: { toastId: 'share-parent-row-hint', volumeId: 'smb-testnas-media', serverLabel: 'Testnas' },
    })
    await tick()
    await expectNoA11yViolations(target)
  })
})
