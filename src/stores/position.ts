import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { Position } from '../types/models'
import { normalizePosition, normalizePositions, positionKey } from '../utils/position'
import { useAsyncState } from '../composables/useAsyncState'

export const usePositionStore = defineStore('position', () => {
  const positions = ref<Position[]>([])
  const request = useAsyncState<Position[]>((value) => value.length === 0)

  function upsertPosition(position: Position): void {
    const normalized = normalizePosition(position)
    if (!normalized.symbol || parseFloat(normalized.size) === 0) {
      positions.value = positions.value.filter((p) => positionKey(p) !== positionKey(normalized))
      request.setData([...positions.value])
      return
    }
    const key = positionKey(normalized)
    const idx = positions.value.findIndex((p) => positionKey(p) === key)
    if (idx >= 0) {
      positions.value[idx] = normalized
    } else {
      positions.value.push(normalized)
    }
    request.setData([...positions.value])
  }

  function setPositions(next: Position[]): void {
    const normalized = normalizePositions(next)
    positions.value = normalized
    request.setData(normalized)
  }

  async function refreshPositions(symbol?: string): Promise<void> {
    await request.run(
      async () => normalizePositions(await tauriInvoke<Position[]>('refresh_positions', {
        symbol: symbol ?? null,
      })),
      (normalized) => { positions.value = normalized },
    )
  }

  function clearPositions(): void {
    positions.value = []
    request.reset()
  }

  return {
    positions,
    loading: request.loading,
    error: request.error,
    status: request.status,
    updatedAt: computed(() => request.state.value.updatedAt),
    requestState: request.state,
    upsertPosition,
    setPositions,
    refreshPositions,
    clearPositions,
  }
})
