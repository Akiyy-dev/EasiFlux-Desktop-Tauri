import { openUrl } from '@tauri-apps/plugin-opener'
import type { NewsTextSegment } from '../types/news'

const URL_CANDIDATE = /(?<![A-Za-z0-9_])https?:\/\/[^\s<>"'，。！？；：、）】》」』]+/giu
const SENTENCE_END = /[.,!?;:，。！？；：、…]$/u
const BRACKETS: ReadonlyArray<readonly [string, string]> = [
  ['(', ')'], ['[', ']'], ['{', '}'], ['（', '）'], ['【', '】'], ['《', '》'], ['「', '」'], ['『', '』'],
]

function trimTrailing(candidate: string): string {
  let cutoff = candidate.length
  const adjacentSentencePunctuation = candidate.match(/[,!?;:](?=\p{Script=Han})/u)
  if (adjacentSentencePunctuation?.index !== undefined) cutoff = adjacentSentencePunctuation.index
  for (const [open, close] of BRACKETS) {
    let depth = 0
    for (let index = 0; index < candidate.length; index += 1) {
      const character = candidate[index]
      if (character === open) depth += 1
      if (character !== close) continue
      if (depth === 0) {
        cutoff = Math.min(cutoff, index)
        break
      }
      depth -= 1
    }
  }
  let result = candidate.slice(0, cutoff)
  while (SENTENCE_END.test(result)) {
    result = result.slice(0, -1)
  }
  return result
}

function isHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

export function segmentNewsText(text: string): NewsTextSegment[] {
  const segments: NewsTextSegment[] = []
  let cursor = 0
  for (const match of text.matchAll(URL_CANDIDATE)) {
    const start = match.index
    const raw = match[0]
    const link = trimTrailing(raw)
    if (!link || !isHttpUrl(link)) continue
    if (start > cursor) segments.push({ kind: 'text', value: text.slice(cursor, start) })
    segments.push({ kind: 'link', value: link, href: link })
    cursor = start + link.length
  }
  if (cursor < text.length) segments.push({ kind: 'text', value: text.slice(cursor) })
  return segments.length > 0 ? segments : [{ kind: 'text', value: text }]
}

export async function openNewsLink(raw: string): Promise<void> {
  let parsed: URL
  try {
    parsed = new URL(raw)
  } catch {
    throw new Error('无效的新闻链接')
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error('无效的新闻链接')
  }
  await openUrl(parsed.toString())
}
