import { onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { flushAllChartWorkspaces } from '../services/chartWorkspaceFlushRegistry'
import { reportError } from '../services/errorService'

type CloseState = 'idle' | 'flushing' | 'destroying' | 'destroyed'

const CLOSE_FLUSH_TIMEOUT_MS = 5_000

async function flushWithTimeout(): Promise<void> {
  let timeout: ReturnType<typeof globalThis.setTimeout> | null = null
  try {
    await Promise.race([
      flushAllChartWorkspaces('close'),
      new Promise<never>((_resolve, reject) => {
        timeout = globalThis.setTimeout(() => {
          reject(new Error('Timed out while saving chart workspaces before close'))
        }, CLOSE_FLUSH_TIMEOUT_MS)
      }),
    ])
  } finally {
    if (timeout !== null) globalThis.clearTimeout(timeout)
  }
}

export function useChartWorkspaceCloseGuard(): void {
  const appWindow = getCurrentWindow()
  let state: CloseState = 'idle'
  let unlisten: (() => void) | null = null
  let unmounted = false

  async function handleClose(event: { preventDefault(): void }): Promise<void> {
    event.preventDefault()
    if (state !== 'idle') return

    state = 'flushing'
    try {
      await flushWithTimeout()
    } catch (error) {
      reportError(error, '关闭前保存图表失败')
    }

    state = 'destroying'
    try {
      await appWindow.destroy()
      state = 'destroyed'
    } catch (error) {
      reportError(error, '关闭图表窗口失败')
      state = 'idle'
    }
  }

  onMounted(async () => {
    try {
      const dispose = await appWindow.onCloseRequested((event) => handleClose(event))
      if (unmounted) dispose()
      else unlisten = dispose
    } catch (error) {
      reportError(error, '安装图表窗口关闭保护失败')
    }
  })

  onUnmounted(() => {
    unmounted = true
    unlisten?.()
    unlisten = null
  })
}
