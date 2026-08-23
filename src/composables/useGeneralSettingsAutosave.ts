import { ref } from 'vue'
import type { GeneralSettings } from '../types/models'

export type GeneralSettingsSaveStatus = 'idle' | 'saving' | 'saved' | 'error'

export function useGeneralSettingsAutosave(
  save: (draft: GeneralSettings) => Promise<GeneralSettings>,
  reconcile: () => Promise<void>,
  debounceMs = 300,
) {
  const draft = ref<GeneralSettings | null>(null)
  const lastSaved = ref<GeneralSettings | null>(null)
  const status = ref<GeneralSettingsSaveStatus>('idle')
  const error = ref<string | null>(null)
  let timer: ReturnType<typeof globalThis.setTimeout> | null = null
  let dirty = false
  let inFlight: Promise<void> | null = null

  function clearTimer(): void {
    if (timer !== null) globalThis.clearTimeout(timer)
    timer = null
  }

  function initialize(value: GeneralSettings): void {
    clearTimer()
    draft.value = { ...value }
    lastSaved.value = { ...value }
    dirty = false
    status.value = 'idle'
    error.value = null
  }

  function schedule(): void {
    clearTimer()
    timer = globalThis.setTimeout(() => { void flush() }, debounceMs)
  }

  function update(patch: Partial<GeneralSettings>): void {
    if (!draft.value) throw new Error('通用设置尚未初始化')
    draft.value = { ...draft.value, ...patch }
    dirty = true
    error.value = null
    if (status.value === 'error') status.value = 'idle'
    schedule()
  }

  async function drain(): Promise<void> {
    while (dirty && draft.value) {
      const snapshot = { ...draft.value }
      dirty = false
      status.value = 'saving'
      error.value = null
      try {
        const committed = await save(snapshot)
        lastSaved.value = { ...committed }
        status.value = dirty ? 'saving' : 'saved'
      } catch (cause) {
        dirty = true
        status.value = 'error'
        error.value = cause instanceof Error ? cause.message : String(cause)
        try {
          await reconcile()
        } catch {
          // Reconciliation must not replace the retained save error.
        }
        return
      }
    }
  }

  async function flush(): Promise<void> {
    clearTimer()
    if (inFlight) {
      await inFlight
      if (dirty && status.value !== 'error') await flush()
      return
    }
    if (!dirty || !draft.value || status.value === 'error') return
    inFlight = drain().finally(() => { inFlight = null })
    await inFlight
  }

  async function retry(): Promise<void> {
    if (!draft.value) return
    dirty = true
    status.value = 'idle'
    error.value = null
    await flush()
  }

  async function dispose(): Promise<void> {
    clearTimer()
    await flush()
    if (status.value === 'error') {
      throw new Error(error.value ?? '通用设置保存失败')
    }
  }

  return {
    draft,
    lastSaved,
    status,
    error,
    initialize,
    update,
    flush,
    retry,
    dispose,
  }
}
