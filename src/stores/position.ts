import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { Position } from '../types/models'
import { normalizePosition, normalizePositions } from '../utils/position'

function positionKey(position: Position): string {
  return `${position.symbol}:${position.positionIdx ?? 0}`
}

export const usePositionStore = defineStore('position', () => {
  const positions = ref<Position[]>([])

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
    positions.value = next
  }

  async function refreshPositions(symbol?: string): Promise<void> {
    const raw = await tauriInvoke<Position[]>('refresh_positions', {
      symbol: symbol ?? null,
    })
    positions.value = normalizePositions(raw)
  }

  return { positions, upsertPosition, setPositions, refreshPositions }
})
