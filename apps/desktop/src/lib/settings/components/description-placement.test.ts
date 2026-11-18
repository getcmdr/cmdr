import { describe, it, expect } from 'vitest'
import { placeDescription } from './description-placement'

const DETAIL = 'The long explanation.'
const SUMMARY = 'The short version.'

describe('placeDescription', () => {
  it('keeps an inline description under the row, searching or not', () => {
    for (const searchQuery of ['', 'dark']) {
      expect(placeDescription({ placement: 'inline', description: DETAIL, summary: '', searchQuery })).toEqual({
        tip: '',
        underLabel: [],
        inline: DETAIL,
      })
    }
  })

  it('puts a tip’s text behind the ⓘ, with its summary under the label', () => {
    expect(placeDescription({ placement: 'tip', description: DETAIL, summary: SUMMARY, searchQuery: '' })).toEqual({
      tip: DETAIL,
      underLabel: [SUMMARY],
      inline: '',
    })
  })

  it('shows a tip’s text under the label while search has a query, and drops the ⓘ', () => {
    expect(placeDescription({ placement: 'tip', description: DETAIL, summary: SUMMARY, searchQuery: 'x' })).toEqual({
      tip: '',
      underLabel: [SUMMARY, DETAIL],
      inline: '',
    })
  })

  it('reads a blank query as no search', () => {
    expect(placeDescription({ placement: 'tip', description: DETAIL, summary: '', searchQuery: '  ' }).tip).toBe(DETAIL)
  })

  it('shows nothing for a setting with no description', () => {
    expect(placeDescription({ placement: 'inline', description: '', summary: '', searchQuery: '' }).inline).toBe('')
    expect(placeDescription({ placement: 'tip', description: '', summary: '', searchQuery: '' }).tip).toBe('')
  })
})
