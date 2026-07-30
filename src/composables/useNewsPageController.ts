import { storeToRefs } from 'pinia'
import { nextTick, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import { useNewsStore } from '../stores/news'

const LATEST_THRESHOLD = 24

export function useNewsPageController(active: Readonly<Ref<boolean>>) {
  const store = useNewsStore()
  const state = storeToRefs(store)
  const scrollContainer = ref<HTMLElement | null>(null)
  let entered = false
  let generation = 0
  let revealOperation = 0
  let markedCandidate: string | undefined

  function current(runGeneration: number): boolean {
    return entered && active.value && runGeneration === generation && state.isPageActive.value
  }

  function renderedTopId(expected?: string): string | undefined {
    const hasRenderedRow = scrollContainer.value?.querySelector('.news-entry:first-child')
    const topId = state.messages.value[0]?.deliveryId
    if (!hasRenderedRow || !topId || (expected && expected !== topId)) return undefined
    return topId
  }

  async function markRenderedTop(expected?: string, runGeneration = generation): Promise<void> {
    await nextTick()
    const scroll = scrollContainer.value
    if (!scroll || !current(runGeneration) || !state.isAtLatest.value
      || scroll.scrollTop > LATEST_THRESHOLD) return
    const topId = renderedTopId(expected)
    if (!topId || topId === markedCandidate) return
    markedCandidate = topId
    try {
      await store.markLatestSeen(topId)
    } catch {
      if (markedCandidate === topId) markedCandidate = undefined
    }
  }

  function scrollToTop(scroll: HTMLElement): void {
    if (typeof scroll.scrollTo === 'function') scroll.scrollTo({ top: 0 })
    else scroll.scrollTop = 0
  }

  async function showLatest(): Promise<void> {
    const operation = ++revealOperation
    const runGeneration = generation
    try {
      const candidate = await store.showLatest()
      if (!candidate || operation !== revealOperation || !current(runGeneration)) return
      await nextTick()
      const scroll = scrollContainer.value
      if (!scroll || renderedTopId(candidate) !== candidate || !current(runGeneration)) return
      scrollToTop(scroll)
      if (!state.isAtLatest.value) store.setAtLatest(true)
      await markRenderedTop(candidate, runGeneration)
    } catch {
      // Store exposes safe error state; page actions never leak rejected promises.
    }
  }

  function handleScroll(): void {
    const scroll = scrollContainer.value
    if (!scroll) return
    const atLatest = scroll.scrollTop <= LATEST_THRESHOLD
    if (atLatest !== state.isAtLatest.value) store.setAtLatest(atLatest)
    if (!atLatest) return
    if (state.pendingNewCount.value > 0) {
      void showLatest()
      return
    }
    void markRenderedTop()
  }

  function enter(): void {
    if (entered) return
    entered = true
    generation += 1
    markedCandidate = undefined
    const runGeneration = generation
    void store.enterPage()
      .then(() => markRenderedTop(undefined, runGeneration))
      .catch(() => undefined)
  }

  function leave(): void {
    if (!entered) return
    entered = false
    generation += 1
    revealOperation += 1
    markedCandidate = undefined
    store.leavePage()
  }

  watch(active, (isActive) => {
    if (isActive) enter()
    else leave()
  }, { immediate: true })
  onBeforeUnmount(leave)

  watch(state.messages, async (next, previous) => {
    const scroll = scrollContainer.value
    const nextTop = next[0]?.deliveryId
    const previousTop = previous[0]?.deliveryId
    if (!scroll || !entered || !nextTop) return
    if (previousTop && BigInt(nextTop) <= BigInt(previousTop)) return
    const oldHeight = scroll.scrollHeight
    const oldTop = scroll.scrollTop
    const wasAtLatest = state.isAtLatest.value && oldTop <= LATEST_THRESHOLD
    const runGeneration = generation
    await nextTick()
    if (!current(runGeneration)) return
    const delta = scroll.scrollHeight - oldHeight
    if (wasAtLatest) {
      scrollToTop(scroll)
      if (!state.isAtLatest.value) store.setAtLatest(true)
      await markRenderedTop(nextTop, runGeneration)
    } else if (delta !== 0) {
      scroll.scrollTop = oldTop + delta
    }
  }, { flush: 'pre' })

  function loadMore(): void {
    if (state.loadingMore.value) return
    void store.loadMore().catch(() => undefined)
  }

  function recheckCredentials(): void {
    void store.recheckCredentials().catch(() => undefined)
  }

  function retrySync(): void {
    void store.retrySync().catch(() => undefined)
  }

  return {
    ...state,
    scrollContainer,
    handleScroll,
    showLatest,
    loadMore,
    recheckCredentials,
    retrySync,
  }
}
