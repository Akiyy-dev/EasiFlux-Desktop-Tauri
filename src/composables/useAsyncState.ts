import { computed, ref } from 'vue'
import type { AsyncState } from '../types/request'

function toMessage(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error instanceof Error) {
    return error.message
  }
  return '请求失败'
}

export function useAsyncState<T>(isEmpty?: (value: T) => boolean) {
  const state = ref<AsyncState<T>>({
    status: 'idle',
    data: null,
    error: null,
    updatedAt: null,
  })

  function setLoading(): void {
    state.value = {
      ...state.value,
      status: 'loading',
      error: null,
    }
  }

  function setData(data: T): void {
    const empty = isEmpty ? isEmpty(data) : false
    state.value = {
      status: empty ? 'empty' : 'success',
      data,
      error: null,
      updatedAt: Date.now(),
    }
  }

  function setError(error: unknown): string {
    const message = toMessage(error)
    state.value = {
      ...state.value,
      status: 'error',
      error: message,
      updatedAt: Date.now(),
    }
    return message
  }

  function reset(): void {
    state.value = {
      status: 'idle',
      data: null,
      error: null,
      updatedAt: null,
    }
  }

  async function run(task: () => Promise<T>): Promise<T> {
    setLoading()
    try {
      const result = await task()
      setData(result)
      return result
    } catch (error) {
      setError(error)
      throw error
    }
  }

  const status = computed(() => state.value.status)
  const loading = computed(() => state.value.status === 'loading')
  const success = computed(() => state.value.status === 'success')
  const empty = computed(() => state.value.status === 'empty')
  const errored = computed(() => state.value.status === 'error')
  const error = computed(() => state.value.error)

  return {
    state,
    status,
    loading,
    success,
    empty,
    errored,
    error,
    run,
    reset,
    setLoading,
    setData,
    setError,
  }
}
