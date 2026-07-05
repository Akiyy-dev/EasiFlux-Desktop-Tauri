import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { Position } from '../types/models'

function positionKey(position: Position): string {
  return `${position.symbol}:${position.positionIdx ?? 0}`
}

export const usePositionStore = defineStore('position', () => {
  const positions = ref<Position[]>([])

  function upsertPosition(position: Position): void {
    const key = positionKey(position)
    const idx = positions.value.findIndex((p) => positionKey(p) === key)
    if (idx >= 0) {
      positions.value[idx] = position
    } else {
      positions.value.push(position)
    }
  }

  async function refreshPositions(symbol?: string): Promise<void> {
    positions.value = await tauriInvoke<Position[]>('refresh_positions', {
      symbol: symbol ?? null,
    })
  }

  return { positions, upsertPosition, refreshPositions }
})
