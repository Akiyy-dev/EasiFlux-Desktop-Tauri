import { describe, expect, it } from 'vitest'
import { parseInstrumentSymbols } from '../../src/utils/instruments'

describe('parseInstrumentSymbols', () => {
  it('extracts symbols from data.list envelope', () => {
    const payload = {
      code: 0,
      data: {
        list: [{ symbol: 'ETHUSDT' }, { symbol: 'BTCUSDT' }],
      },
    }
    expect(parseInstrumentSymbols(payload)).toEqual(['BTCUSDT', 'ETHUSDT'])
  })

  it('deduplicates and ignores invalid items', () => {
    const payload = [{ symbol: 'BTCUSDT' }, { foo: 'bar' }, { symbol: 'BTCUSDT' }]
    expect(parseInstrumentSymbols(payload)).toEqual(['BTCUSDT'])
  })
})
