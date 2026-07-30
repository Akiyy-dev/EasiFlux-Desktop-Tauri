import { onMounted } from 'vue'
import { reportError } from '../services/errorService'
import { useNewsStore } from '../stores/news'
import type { NewsMessagesCommittedEvent, NewsStatusSnapshot } from '../types/news'
import { useTauriEvent, whenTauriListenersReady } from './useTauriEvent'

export function useNewsRuntimeHost(): void {
  const store = useNewsStore()
  useTauriEvent<NewsMessagesCommittedEvent>('news://messages-committed', (payload) => {
    void store.handleMessagesCommitted(payload)
      .catch((error: unknown) => reportError(error, '处理新闻提交事件失败'))
  })
  useTauriEvent<NewsStatusSnapshot>('news://status-changed', (payload) => {
    void store.handleStatusChanged(payload)
      .catch((error: unknown) => reportError(error, '处理新闻状态事件失败'))
  })
  onMounted(() => {
    void (async () => {
      try {
        await whenTauriListenersReady()
      } catch (error) {
        reportError(error, '初始化新闻监听失败')
      }
      try {
        await store.initialize()
      } catch (error) {
        reportError(error, '初始化新闻中心失败')
      }
    })()
  })
}
