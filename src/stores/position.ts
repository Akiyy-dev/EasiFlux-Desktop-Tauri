import { defineStore } from 'pinia'
import { ref } from 'vue'
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
      return
    }
    const key = positionKey(normalized)
    const idx = positions.value.findIndex((p) => positionKey(p) === key)
    if (idx >= 0) {
      positions.value[idx] = normalized
    } else {
      positions.value.push(normalized)
    }
  }

  function setPositions(next: Position[]): void {
    positions.value = normalizePositions(next)
  }

  async function refreshPositions(symbol?: string): Promise<void> {
    const raw = await request.run(() =>
      tauriInvoke<Position[]>('refresh_positions', {
        symbol: symbol ?? null,
      }),
    )
    positions.value = normalizePositions(raw)
  }

  return {
    positions,
    loading: request.loading,
    error: request.error,
    status: request.status,
    upsertPosition,
    setPositions,
    refreshPositions,
  }
})
