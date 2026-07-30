import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import {
  getNewsStatus,
  listNewsMessages,
  markNewsSeen,
  recheckNewsCredentials,
  retryNewsSync,
} from '../../src/services/newsService'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

describe('news command service', () => {
  beforeEach(() => vi.mocked(tauriInvoke).mockReset().mockResolvedValue(undefined))

  it('uses the exact command names and camelCase arguments without credentials or source data', async () => {
    await getNewsStatus()
    await listNewsMessages()
    await listNewsMessages('9007199254740993', 12)
    await markNewsSeen('9007199254740995')
    await recheckNewsCredentials()
    await retryNewsSync()

    expect(vi.mocked(tauriInvoke).mock.calls).toEqual([
      ['get_news_status'],
      ['list_news_messages', { limit: 50 }],
      ['list_news_messages', { beforeDeliveryId: '9007199254740993', limit: 12 }],
      ['mark_news_seen', { throughDeliveryId: '9007199254740995' }],
      ['recheck_news_credentials'],
      ['retry_news_sync'],
    ])
  })
})
