import { describe, expect, it } from 'vitest'
import { startDataSync, stopDataSync, syncRunning } from '../../src/services/dataSyncService'

describe('dataSyncService bridge', () => {
  it('does not start frontend interval scheduler', () => {
    startDataSync()
    expect(syncRunning()).toBe(false)
    stopDataSync()
  })
})
