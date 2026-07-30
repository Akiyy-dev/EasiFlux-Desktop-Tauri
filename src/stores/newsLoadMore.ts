import type { NewsPage } from '../types/news'

interface NewsLoadMoreOptions {
  target: () => string | undefined
  list: (beforeDeliveryId: string) => Promise<NewsPage>
  apply: (result: NewsPage) => void
  setBusy: (busy: boolean) => void
  setError: (message: string | null) => void
  errorText: (error: unknown) => string
}

export function createNewsLoadMore(options: NewsLoadMoreOptions) {
  let epoch = 0
  let flight: Promise<void> | null = null

  function reset(): void {
    epoch += 1
    flight = null
    options.setBusy(false)
    options.setError(null)
  }

  function load(): Promise<void> {
    if (flight) return flight
    const target = options.target()
    if (!target) return Promise.resolve()
    const runEpoch = epoch
    options.setBusy(true)
    options.setError(null)
    const tracked = options.list(target).then((result) => {
      if (runEpoch === epoch) options.apply(result)
    }).catch((error: unknown) => {
      if (runEpoch === epoch) options.setError(options.errorText(error))
      throw error
    }).finally(() => {
      if (runEpoch !== epoch || flight !== tracked) return
      flight = null
      options.setBusy(false)
    })
    flight = tracked
    return tracked
  }

  return { load, reset }
}
