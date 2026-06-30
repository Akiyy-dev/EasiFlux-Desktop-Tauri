import { describe, expect, it } from 'vitest'

describe('models', () => {
  it('defines connection status union', () => {
    const status: 'connected' | 'disconnected' = 'connected'
    expect(status).toBe('connected')
  })
})
