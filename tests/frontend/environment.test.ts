import { describe, expect, it } from 'vitest'
import { resolveApiEnvironmentLabel } from '../../src/utils/environment'

describe('resolveApiEnvironmentLabel', () => {
  it('detects production api host', () => {
    expect(resolveApiEnvironmentLabel('https://api.easicoin.io')).toBe('正式')
    expect(resolveApiEnvironmentLabel(null)).toBe('未知')
  })

  it('detects non-production api host', () => {
    expect(resolveApiEnvironmentLabel('https://test-api.example.com')).toBe('测试')
  })
})
