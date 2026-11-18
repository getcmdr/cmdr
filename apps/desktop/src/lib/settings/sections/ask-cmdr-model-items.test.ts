import { describe, it, expect } from 'vitest'
import { askCmdrModelItems, pickFromStoredModel, storedModelFromPick, SAME_AS_CMDR_AI } from './ask-cmdr-model-items'

const sameAsLabel = 'Same as Cmdr’s AI (claude-opus-5-5)'

describe('askCmdrModelItems', () => {
  it('puts "Same as Cmdr’s AI" first, then the provider’s models in order', () => {
    const items = askCmdrModelItems({ current: '', models: ['b-model', 'a-model'], sameAsLabel })
    expect(items.map((i) => i.value)).toEqual([SAME_AS_CMDR_AI, 'b-model', 'a-model'])
    expect(items[0].label).toBe(sameAsLabel)
  })

  it('keeps a stored model the list doesn’t have, so a hand-typed one stays visible and selected', () => {
    const items = askCmdrModelItems({ current: 'my-own-model', models: ['a-model'], sameAsLabel })
    expect(items.map((i) => i.value)).toEqual([SAME_AS_CMDR_AI, 'my-own-model', 'a-model'])
  })

  it('shows the stored model even before the list loads', () => {
    const items = askCmdrModelItems({ current: 'my-own-model', models: [], sameAsLabel })
    expect(items.map((i) => i.value)).toEqual([SAME_AS_CMDR_AI, 'my-own-model'])
  })

  it('lists a stored model that IS in the list once, in the provider’s place', () => {
    const items = askCmdrModelItems({ current: 'a-model', models: ['b-model', 'a-model', 'a-model', ''], sameAsLabel })
    expect(items.map((i) => i.value)).toEqual([SAME_AS_CMDR_AI, 'b-model', 'a-model'])
  })
})

describe('the stored-value mapping', () => {
  it('maps the empty stored value to the "same as" row and back', () => {
    expect(pickFromStoredModel('')).toBe(SAME_AS_CMDR_AI)
    expect(storedModelFromPick(SAME_AS_CMDR_AI)).toBe('')
  })

  it('passes a real model id through both ways', () => {
    expect(pickFromStoredModel('gpt-4o')).toBe('gpt-4o')
    expect(storedModelFromPick('gpt-4o')).toBe('gpt-4o')
  })
})
