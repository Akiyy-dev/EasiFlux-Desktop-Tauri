import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { Position } from '../types/models'

export const usePositionStore = defineStore('position', () => {
  const positions = ref<Position[]>([])

  function upsertPosition(position: Position): void {
    const idx = positions.value.findIndex((p) => p.symbol === position.symbol)
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
