import { beforeEach, describe, expect, it, vi } from 'vitest'
import { openUrl } from '@tauri-apps/plugin-opener'
import { openNewsLink, segmentNewsText } from '../../src/utils/newsLinks'

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))

describe('news links', () => {
  beforeEach(() => vi.mocked(openUrl).mockReset().mockResolvedValue(undefined))

  it('preserves text and strips sentence punctuation and unmatched closing brackets from hrefs', () => {
    const text = '参考（https://example.com/a_(b)?x=1#y）。以及 http://test.dev/path],结束'

    expect(segmentNewsText(text)).toEqual([
      { kind: 'text', value: '参考（' },
      { kind: 'link', value: 'https://example.com/a_(b)?x=1#y', href: 'https://example.com/a_(b)?x=1#y' },
      { kind: 'text', value: '）。以及 ' },
      { kind: 'link', value: 'http://test.dev/path', href: 'http://test.dev/path' },
      { kind: 'text', value: '],结束' },
    ])
    expect(segmentNewsText(text).map((part) => part.value).join('')).toBe(text)
  })

  it.each([
    'javascript:alert(1)',
    'data:text/html,bad',
    'file:///tmp/bad',
    'example.com/path',
    'httpsx://example.com',
    'http://[bad',
  ])('leaves non-http or malformed candidate as ordinary text: %s', (text) => {
    expect(segmentNewsText(text)).toEqual([{ kind: 'text', value: text }])
  })

  it('normalizes and opens only an independently revalidated http(s) URL', async () => {
    await openNewsLink('HTTPS://Example.COM/a b?q=1')

    expect(openUrl).toHaveBeenCalledWith('https://example.com/a%20b?q=1')
  })

  it('keeps unicode URL paths while separating adjacent sentence punctuation', () => {
    const text = '访问 https://example.com/😀?x=1,然后查看 https://example.com/路径]结束'

    expect(segmentNewsText(text)).toEqual([
      { kind: 'text', value: '访问 ' },
      { kind: 'link', value: 'https://example.com/😀?x=1', href: 'https://example.com/😀?x=1' },
      { kind: 'text', value: ',然后查看 ' },
      { kind: 'link', value: 'https://example.com/路径', href: 'https://example.com/路径' },
      { kind: 'text', value: ']结束' },
    ])
  })

  it('recognizes an http URL after Chinese prose but rejects an ASCII lookalike prefix', () => {
    expect(segmentNewsText('详情https://example.com。')).toEqual([
      { kind: 'text', value: '详情' },
      { kind: 'link', value: 'https://example.com', href: 'https://example.com' },
      { kind: 'text', value: '。' },
    ])
    expect(segmentNewsText('abchttps://example.com')).toEqual([
      { kind: 'text', value: 'abchttps://example.com' },
    ])
  })

  it.each(['javascript:alert(1)', 'data:text/plain,bad', 'file:///tmp/bad', 'not a url'])(
    'rejects dangerous or malformed URLs before invoking the opener: %s',
    async (raw) => {
      await expect(openNewsLink(raw)).rejects.toThrow('无效的新闻链接')
      expect(openUrl).not.toHaveBeenCalled()
    },
  )
})
