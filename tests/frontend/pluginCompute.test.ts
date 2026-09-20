import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import * as pluginServiceModule from '../../src/services/pluginService'
import type { PluginComputeRequest } from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

type ComputeService = typeof pluginServiceModule & {
  parsePluginComputeInput: (raw: string) =>
    | { ok: true; values: number[] }
    | { ok: false; error: string }
  parsePluginComputeParameter: (
    raw: string,
    bounds: { min: number; max: number },
  ) => { ok: true; value: number } | { ok: false; error: string }
  executePluginCompute: (request: PluginComputeRequest) => Promise<unknown>
  cancelPluginCompute: (requestId: string) => Promise<unknown>
}

const service = pluginServiceModule as ComputeService

function request(): PluginComputeRequest {
  return {
    requestId: 'compute-1',
    pluginId: 'com.example.analytics',
    contributionId: 'analytics.average',
    expectedCatalogGeneration: '8',
    expectedRevision: '13',
    values: [1, 2, 3],
    parameter: 2.5,
  }
}

function result(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    schemaVersion: 1,
    requestId: 'compute-1',
    pluginId: 'com.example.analytics',
    contributionId: 'analytics.average',
    catalogGeneration: '8',
    revision: '13',
    value: 2,
    inputCount: 3,
    parameter: 2.5,
    ...overrides,
  }
}

beforeEach(() => {
  vi.mocked(tauriInvoke).mockReset()
})

describe('plugin compute input parsing', () => {
  it('accepts comma or whitespace separated finite decimal and scientific values', () => {
    expect(service.parsePluginComputeInput('1, -2.5\n3e2\t+.4')).toEqual({
      ok: true,
      values: [1, -2.5, 300, 0.4],
    })
  })

  it.each([
    '', '   ', '0x10', 'Infinity', '-Infinity', 'NaN', '1 nope 2', '1,,2', '1,',
  ])('rejects invalid input text %j', (raw) => {
    expect(service.parsePluginComputeInput(raw)).toMatchObject({ ok: false })
  })

  it('enforces raw text and item count bounds before execution', () => {
    expect(service.parsePluginComputeInput('1 '.repeat(4097))).toMatchObject({ ok: false })
    expect(service.parsePluginComputeInput(`1${' '.repeat(128 * 1024)}`))
      .toMatchObject({ ok: false })
  })

  it('accepts fractional parameters but enforces finite decimal syntax and manifest bounds', () => {
    expect(service.parsePluginComputeParameter('2.5', { min: 1, max: 3 })).toEqual({
      ok: true,
      value: 2.5,
    })
    for (const raw of ['', '0x2', 'NaN', 'Infinity', '0.5', '3.1']) {
      expect(service.parsePluginComputeParameter(raw, { min: 1, max: 3 }))
        .toMatchObject({ ok: false })
    }
  })
})

describe('plugin compute transport', () => {
  it('invokes only the fixed execute command and validates a correlated closed response', async () => {
    const sent = request()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result())

    await expect(service.executePluginCompute(sent)).resolves.toEqual(result())
    expect(tauriInvoke).toHaveBeenCalledWith('execute_plugin_compute', { request: sent })
  })

  it.each([
    { extra: true },
    { schemaVersion: 2 },
    { requestId: 'compute-other' },
    { pluginId: 'com.example.other' },
    { contributionId: 'analytics.other' },
    { catalogGeneration: '9' },
    { revision: '14' },
    { value: Infinity },
    { inputCount: 2 },
    { parameter: 2 },
  ])('rejects malformed or uncorrelated execute responses %#', async (override) => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result(override))
    await expect(service.executePluginCompute(request()))
      .rejects.toThrow('插件服务返回的数据无效，请重试。')
  })

  it('uses a closed correlated cancel response', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      schemaVersion: 1, requestId: 'compute-1', cancelled: true,
    })

    await expect(service.cancelPluginCompute('compute-1')).resolves.toEqual({
      schemaVersion: 1, requestId: 'compute-1', cancelled: true,
    })
    expect(tauriInvoke).toHaveBeenCalledWith('cancel_plugin_compute', {
      requestId: 'compute-1',
    })
  })

  it('maps compute errors only by the fixed code and never exposes backend text', () => {
    expect(pluginServiceModule.pluginErrorMessage({
      code: 'plugin_compute_trap', message: '<guest secret>',
    })).toBe('插件计算失败；模块未能完成本次运行。')
    expect(pluginServiceModule.pluginErrorMessage({
      code: 'plugin_compute_unknown', message: '<guest secret>',
    })).toBe('插件操作失败，请重试。')
  })
})
