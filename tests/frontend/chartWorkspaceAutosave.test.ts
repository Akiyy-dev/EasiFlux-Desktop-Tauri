import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  ChartWorkspaceAutosave,
  type CapturedWorkspaceState,
} from '../../src/services/chartWorkspaceAutosave'
import type {
  ChartPreferencesV1,
  ChartWorkspaceKey,
  ChartWorkspaceSaveResult,
  ChartWorkspaceSnapshot,
  SaveChartWorkspaceRequest,
} from '../../src/types/chartWorkspace'
import {
  chartPreferencesContentFingerprint,
  chartViewContentFingerprint,
} from '../../src/utils/chartWorkspace'
import {
  deferred,
  key,
  samplePreferences,
  sampleSnapshot,
  segment,
  successfulSave,
  viewState,
  viewStateFor,
} from './helpers/chartWorkspaceFixtures'

function saveResult(
  workspaceKey: ChartWorkspaceKey,
  viewRevision: number,
  preferencesRevision: number,
  patch: Partial<ChartWorkspaceSaveResult> = {},
): ChartWorkspaceSaveResult {
  return {
    ...successfulSave(viewRevision, preferencesRevision),
    key: workspaceKey,
    ...patch,
  }
}

function snapshotWithPreferences(
  workspaceKey: ChartWorkspaceKey,
  preferences: ChartPreferencesV1,
): ChartWorkspaceSnapshot {
  return {
    ...sampleSnapshot([], workspaceKey),
    preferences,
  }
}

function makeFixture(options: {
  saveResults?: Array<ChartWorkspaceSaveResult | Promise<ChartWorkspaceSaveResult>>
  firstSaveRejects?: boolean
} = {}) {
  const state = {
    activeKey: key(),
    capturedView: viewState(),
    capturedPreferences: samplePreferences(),
  }
  const results = [...(options.saveResults ?? [])]
  let call = 0
  const save = vi.fn(async (request: SaveChartWorkspaceRequest) => {
    call += 1
    if (options.firstSaveRejects && call === 1) throw new Error('disk full')
    return await (results.shift() ?? {
      ...successfulSave(
        request.viewState?.revision ?? 0,
        request.preferences?.revision ?? 0,
      ),
      key: request.key,
    })
  })
  const report = vi.fn()
  const capture = vi.fn((
    _key: ChartWorkspaceKey,
    viewRevision: number,
    preferencesRevision: number,
  ): CapturedWorkspaceState => ({
    viewState: { ...state.capturedView, revision: viewRevision },
    preferences: { ...state.capturedPreferences, revision: preferencesRevision },
    viewFingerprint: chartViewContentFingerprint(state.capturedView),
    preferencesFingerprint: chartPreferencesContentFingerprint(state.capturedPreferences),
  }))
  const autosave = new ChartWorkspaceAutosave({
    save,
    report,
    setInterval: globalThis.setInterval,
    clearInterval: globalThis.clearInterval,
  })
  const source = autosave.registerCaptureSource({
    getActiveKey: () => state.activeKey,
    capture: { capture },
  })
  source.setActive(true)
  autosave.adoptSnapshot(sampleSnapshot())
  return { state, autosave, save, report, capture, source }
}

describe('ChartWorkspaceAutosave', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('does not save clean state on a five-second tick', async () => {
    const fixture = makeFixture()
    fixture.autosave.start()

    await vi.advanceTimersByTimeAsync(15_000)

    expect(fixture.save).not.toHaveBeenCalled()
  })

  it('detects a missed callback from the captured fingerprint', async () => {
    const fixture = makeFixture()
    fixture.autosave.start()
    fixture.state.capturedView = viewState(1, [segment('new')])

    await vi.advanceTimersByTimeAsync(5_000)

    expect(fixture.save).toHaveBeenCalledOnce()
    expect(fixture.save.mock.calls[0]![0].viewState?.revision).toBe(1)
  })

  it('keeps the newer payload when an older revision completes', async () => {
    const first = deferred<ChartWorkspaceSaveResult>()
    const fixture = makeFixture({ saveResults: [first.promise, successfulSave(2, 0)] })
    fixture.source.markViewDirty()
    const flushing = fixture.autosave.flush('timer')
    await vi.waitFor(() => expect(fixture.save).toHaveBeenCalledOnce())
    fixture.state.capturedView = viewState(2, [segment('newer')])
    fixture.source.markViewDirty()

    first.resolve(successfulSave(1, 0))
    await flushing

    expect(fixture.save).toHaveBeenCalledTimes(2)
    expect(fixture.save.mock.calls[1]![0].viewState?.revision).toBe(2)
    expect(fixture.save.mock.calls[1]![0].viewState?.overlays).toEqual([segment('newer')])
  })

  it('retains a serialized old-key payload after navigation', async () => {
    const fixture = makeFixture({ firstSaveRejects: true })
    fixture.state.capturedView = viewState(1, [segment('pending')])
    fixture.source.markViewDirty()
    await fixture.autosave.flush('context')
    fixture.state.activeKey = key('ETHUSDT', '15')
    fixture.state.capturedView = viewStateFor(fixture.state.activeKey, 0, [])
    fixture.autosave.start()

    await vi.advanceTimersByTimeAsync(5_000)

    expect(fixture.save.mock.calls[1]![0].key).toEqual(key('BTCUSDT', '1'))
    expect(fixture.save.mock.calls[1]![0].viewState?.overlays).toEqual([segment('pending')])
  })

  it('single_flight_never_overlaps_save_calls', async () => {
    const first = deferred<ChartWorkspaceSaveResult>()
    let inFlight = 0
    let maximumInFlight = 0
    const save = vi.fn(async (request: SaveChartWorkspaceRequest) => {
      inFlight += 1
      maximumInFlight = Math.max(maximumInFlight, inFlight)
      try {
        if (save.mock.calls.length === 1) return await first.promise
        return saveResult(
          request.key,
          request.viewState?.revision ?? 0,
          request.preferences?.revision ?? 0,
        )
      } finally {
        inFlight -= 1
      }
    })
    const fixture = makeFixture()
    const autosave = new ChartWorkspaceAutosave({
      save,
      report: vi.fn(),
      setInterval: globalThis.setInterval,
      clearInterval: globalThis.clearInterval,
    })
    const source = autosave.registerCaptureSource({
      getActiveKey: () => fixture.state.activeKey,
      capture: { capture: fixture.capture },
    })
    source.setActive(true)
    autosave.adoptSnapshot(sampleSnapshot())
    source.markViewDirty()

    const flushes = [autosave.flush('timer'), autosave.flush('timer'), autosave.flush('timer')]
    await vi.waitFor(() => expect(save).toHaveBeenCalledOnce())
    expect(maximumInFlight).toBe(1)
    first.resolve(successfulSave(1, 0))
    await Promise.all(flushes)

    expect(maximumInFlight).toBe(1)
    expect(save).toHaveBeenCalledOnce()
  })

  it('queued_flush_drains_immediately_after_settlement', async () => {
    const first = deferred<ChartWorkspaceSaveResult>()
    const fixture = makeFixture({ saveResults: [first.promise, successfulSave(2, 0)] })
    fixture.source.markViewDirty()
    const flushing = fixture.autosave.flush('timer')
    await vi.waitFor(() => expect(fixture.save).toHaveBeenCalledOnce())
    fixture.state.capturedView = viewState(2, [segment('revision-2')])
    fixture.source.markViewDirty()

    first.resolve(successfulSave(1, 0))
    await flushing

    expect(fixture.save).toHaveBeenCalledTimes(2)
    expect(fixture.save.mock.calls[1]![0].viewState?.revision).toBe(2)
  })

  it('partial_success_retries_only_the_failed_preferences_part', async () => {
    const fixture = makeFixture({
      saveResults: [
        saveResult(key(), 1, 0, {
          preferencesSaved: false,
          preferencesError: 'preferences unavailable',
        }),
        successfulSave(1, 1),
      ],
    })
    fixture.state.capturedView = viewState(1, [segment('view')])
    fixture.state.capturedPreferences = {
      ...samplePreferences(1),
      mainIndicators: ['RSI'],
    }
    fixture.source.markViewDirty()
    fixture.source.markPreferencesDirty()
    await fixture.autosave.flush('timer')
    const failedPreferences = fixture.save.mock.calls[0]![0].preferences

    await fixture.autosave.flush('timer')

    expect(fixture.save).toHaveBeenCalledTimes(2)
    expect(fixture.save.mock.calls[1]![0].viewState).toBeNull()
    expect(fixture.save.mock.calls[1]![0].preferences).toEqual(failedPreferences)
    expect(fixture.save.mock.calls[1]![0].preferences?.revision).toBe(1)
  })

  it('immediate_kline_only_flush_omits_frontend_state', async () => {
    const fixture = makeFixture()

    await fixture.autosave.flush('context')

    expect(fixture.save).toHaveBeenCalledOnce()
    expect(fixture.save.mock.calls[0]![0]).toEqual({
      key: key(),
      viewState: null,
      preferences: null,
    })
  })

  it('late_key_a_success_never_acknowledges_key_b', async () => {
    const first = deferred<ChartWorkspaceSaveResult>()
    const workspaceB = key('ETHUSDT', '15')
    const fixture = makeFixture({
      saveResults: [first.promise, saveResult(workspaceB, 1, 0)],
    })
    fixture.state.capturedView = viewState(1, [segment('a')])
    fixture.source.markViewDirty()
    const flushing = fixture.autosave.flush('timer')
    await vi.waitFor(() => expect(fixture.save).toHaveBeenCalledOnce())
    fixture.state.activeKey = workspaceB
    fixture.state.capturedView = viewStateFor(workspaceB, 1, [segment('b')])
    fixture.autosave.adoptSnapshot(sampleSnapshot([], workspaceB))
    fixture.source.markViewDirty()

    first.resolve(successfulSave(1, 0))
    await flushing

    expect(fixture.save).toHaveBeenCalledTimes(2)
    expect(fixture.save.mock.calls[1]![0].key).toEqual(workspaceB)
    expect(fixture.save.mock.calls[1]![0].viewState?.overlays).toEqual([segment('b')])
    expect(fixture.save.mock.calls[1]![0].preferences).toBeNull()
  })

  it('failed_old_key_drains_before_new_active_key', async () => {
    const workspaceB = key('ETHUSDT', '15')
    const fixture = makeFixture({ firstSaveRejects: true })
    fixture.state.capturedView = viewState(1, [segment('a')])
    fixture.source.markViewDirty()
    await fixture.autosave.flush('timer')
    fixture.state.activeKey = workspaceB
    fixture.state.capturedView = viewStateFor(workspaceB, 1, [segment('b')])
    fixture.autosave.adoptSnapshot(sampleSnapshot([], workspaceB))
    fixture.source.markViewDirty()

    await fixture.autosave.flush('timer')

    expect(fixture.save.mock.calls.map(([request]) => request.key)).toEqual([
      key(),
      key(),
      workspaceB,
    ])
  })

  it('close_flush_continues_past_a_rejected_old_key_without_retrying_it_in_place', async () => {
    const rejectedOldKeyRetry = deferred<ChartWorkspaceSaveResult>()
    const workspaceB = key('ETHUSDT', '15')
    const fixture = makeFixture({
      firstSaveRejects: true,
      saveResults: [rejectedOldKeyRetry.promise],
    })
    fixture.state.capturedView = viewState(1, [segment('a')])
    fixture.source.markViewDirty()
    await fixture.autosave.flush('timer')
    fixture.state.activeKey = workspaceB
    fixture.state.capturedView = viewStateFor(workspaceB, 1, [segment('b')])
    fixture.autosave.adoptSnapshot(sampleSnapshot([], workspaceB))
    fixture.source.markViewDirty()

    const closing = fixture.autosave.flush('close')
    await vi.waitFor(() => expect(fixture.save).toHaveBeenCalledTimes(2))
    rejectedOldKeyRetry.reject(new Error('disk still full'))
    await closing

    expect(fixture.save.mock.calls.map(([request]) => request.key)).toEqual([
      key(),
      key(),
      workspaceB,
    ])

    await fixture.autosave.flush('timer')

    expect(fixture.save).toHaveBeenCalledTimes(4)
    expect(fixture.save.mock.calls[3]![0].key).toEqual(key())
  })

  it('failed_kline_flush_retries_on_the_next_tick', async () => {
    const fixture = makeFixture({
      saveResults: [
        saveResult(key(), 0, 0, { klineSaved: false, klineError: 'busy' }),
        successfulSave(),
      ],
    })
    await fixture.autosave.flush('context')
    fixture.autosave.start()

    await vi.advanceTimersByTimeAsync(5_000)

    expect(fixture.save).toHaveBeenCalledTimes(2)
    expect(fixture.save.mock.calls[1]![0].key).toEqual(key())
    await vi.advanceTimersByTimeAsync(5_000)
    expect(fixture.save).toHaveBeenCalledTimes(2)
  })

  it('stop_cancels_future_ticks', async () => {
    const fixture = makeFixture()
    fixture.source.markViewDirty()
    fixture.autosave.start()
    fixture.autosave.stop()

    await vi.advanceTimersByTimeAsync(15_000)

    expect(fixture.save).not.toHaveBeenCalled()
  })

  it('stale_source_cannot_dirty_or_deactivate_the_new_source', async () => {
    const workspaceA = key()
    const workspaceB = key('ETHUSDT', '15')
    const captureA = vi.fn((
      _key: ChartWorkspaceKey,
      viewRevision: number,
      preferencesRevision: number,
    ) => ({
      viewState: viewStateFor(workspaceA, viewRevision, [segment('a')]),
      preferences: samplePreferences(preferencesRevision),
      viewFingerprint: chartViewContentFingerprint(viewStateFor(workspaceA, 0, [segment('a')])),
      preferencesFingerprint: chartPreferencesContentFingerprint(samplePreferences()),
    }))
    const captureB = vi.fn((
      _key: ChartWorkspaceKey,
      viewRevision: number,
      preferencesRevision: number,
    ) => ({
      viewState: viewStateFor(workspaceB, viewRevision, [segment('b')]),
      preferences: samplePreferences(preferencesRevision),
      viewFingerprint: chartViewContentFingerprint(viewStateFor(workspaceB, 0, [segment('b')])),
      preferencesFingerprint: chartPreferencesContentFingerprint(samplePreferences()),
    }))
    const save = vi.fn(async (request: SaveChartWorkspaceRequest) => saveResult(
      request.key,
      request.viewState?.revision ?? 0,
      request.preferences?.revision ?? 0,
    ))
    const autosave = new ChartWorkspaceAutosave({
      save,
      report: vi.fn(),
      setInterval: globalThis.setInterval,
      clearInterval: globalThis.clearInterval,
    })
    const sourceA = autosave.registerCaptureSource({
      getActiveKey: () => workspaceA,
      capture: { capture: captureA },
    })
    const sourceB = autosave.registerCaptureSource({
      getActiveKey: () => workspaceB,
      capture: { capture: captureB },
    })
    sourceA.setActive(true)
    autosave.adoptSnapshot(sampleSnapshot([], workspaceA))
    sourceB.setActive(true)
    autosave.adoptSnapshot(sampleSnapshot([], workspaceB))
    sourceA.markViewDirty()
    sourceA.markPreferencesDirty()
    sourceA.setActive(false)
    sourceB.markViewDirty()
    autosave.start()

    await vi.advanceTimersByTimeAsync(5_000)

    expect(captureA).not.toHaveBeenCalled()
    expect(captureB).toHaveBeenCalled()
    expect(save).toHaveBeenCalledOnce()
    expect(save.mock.calls[0]![0].key).toEqual(workspaceB)
  })

  it('global_preferences_have_one_queue_across_sources', async () => {
    const workspaceA = key()
    const workspaceB = key('ETHUSDT', '15')
    const lateA = deferred<ChartWorkspaceSaveResult>()
    const preferencesA = { ...samplePreferences(), mainIndicators: ['RSI'] }
    const persistedB = { ...samplePreferences(3), mainIndicators: ['SMA'] }
    const preferencesB = { ...samplePreferences(), mainIndicators: ['BOLL'] }
    const saveResults: Array<ChartWorkspaceSaveResult | Promise<ChartWorkspaceSaveResult>> = [
      lateA.promise,
      saveResult(workspaceB, 0, 10, {
        preferencesSaved: false,
        preferencesError: 'revision conflict',
      }),
      saveResult(workspaceB, 0, 11),
    ]
    const save = vi.fn(async (): Promise<ChartWorkspaceSaveResult> => await saveResults.shift()!)
    const autosave = new ChartWorkspaceAutosave({
      save,
      report: vi.fn(),
      setInterval: globalThis.setInterval,
      clearInterval: globalThis.clearInterval,
    })
    const sourceA = autosave.registerCaptureSource({
      getActiveKey: () => workspaceA,
      capture: {
        capture: (_key, viewRevision, preferencesRevision) => ({
          viewState: viewStateFor(workspaceA, viewRevision),
          preferences: { ...preferencesA, revision: preferencesRevision },
          viewFingerprint: chartViewContentFingerprint(viewStateFor(workspaceA)),
          preferencesFingerprint: chartPreferencesContentFingerprint(preferencesA),
        }),
      },
    })
    const sourceB = autosave.registerCaptureSource({
      getActiveKey: () => workspaceB,
      capture: {
        capture: (_key, viewRevision, preferencesRevision) => ({
          viewState: viewStateFor(workspaceB, viewRevision),
          preferences: { ...preferencesB, revision: preferencesRevision },
          viewFingerprint: chartViewContentFingerprint(viewStateFor(workspaceB)),
          preferencesFingerprint: chartPreferencesContentFingerprint(preferencesB),
        }),
      },
    })
    sourceA.setActive(true)
    autosave.adoptSnapshot(sampleSnapshot([], workspaceA))
    sourceA.markPreferencesDirty()
    const flushing = autosave.flush('timer')
    await vi.waitFor(() => expect(save).toHaveBeenCalledOnce())
    sourceB.setActive(true)
    autosave.adoptSnapshot(snapshotWithPreferences(workspaceB, persistedB))
    sourceA.markPreferencesDirty()
    sourceB.markPreferencesDirty()

    lateA.resolve(saveResult(workspaceA, 0, 10, {
      preferencesSaved: false,
      preferencesError: 'late revision conflict',
    }))
    await flushing

    expect(save.mock.calls[1]![0].key).toEqual(workspaceB)
    expect(save.mock.calls[1]![0].viewState).toBeNull()
    expect(save.mock.calls[1]![0].preferences).toMatchObject({
      revision: 4,
      mainIndicators: ['BOLL'],
    })
    expect(save.mock.calls[2]![0].preferences).toMatchObject({
      revision: 11,
      mainIndicators: ['BOLL'],
    })
  })

  it('capture_key_mismatch_is_reported_and_never_queued', async () => {
    const fixture = makeFixture()
    fixture.state.activeKey = key('ETHUSDT', '15')
    fixture.autosave.start()

    await vi.advanceTimersByTimeAsync(15_000)

    expect(fixture.save).not.toHaveBeenCalled()
    expect(fixture.report).toHaveBeenCalledOnce()
  })

  it('rebases_a_stale_view_after_revision_conflict', async () => {
    const fixture = makeFixture({
      saveResults: [
        saveResult(key(), 4, 0, {
          viewStateSaved: false,
          viewStateError: 'revision conflict',
        }),
        successfulSave(5, 0),
      ],
    })
    fixture.state.capturedView = viewState(1, [segment('unchanged')])
    fixture.source.markViewDirty()
    await fixture.autosave.flush('timer')

    await fixture.autosave.flush('timer')

    expect(fixture.save.mock.calls[1]![0].viewState).toMatchObject({
      revision: 5,
      overlays: [segment('unchanged')],
    })
  })

  it('rebases_global_preferences_after_cross_key_conflict', async () => {
    const workspaceB = key('ETHUSDT', '15')
    const fixture = makeFixture({
      saveResults: [
        saveResult(key(), 0, 3, {
          preferencesSaved: false,
          preferencesError: 'revision conflict',
        }),
        saveResult(key(), 0, 4),
      ],
    })
    fixture.state.capturedPreferences = {
      ...samplePreferences(),
      subIndicators: ['RSI'],
    }
    fixture.source.markPreferencesDirty()
    await fixture.autosave.flush('timer')
    fixture.state.activeKey = workspaceB
    fixture.state.capturedView = viewStateFor(workspaceB)

    await fixture.autosave.flush('timer')

    expect(fixture.save.mock.calls[1]![0].key).toEqual(key())
    expect(fixture.save.mock.calls[1]![0].preferences).toMatchObject({
      revision: 4,
      subIndicators: ['RSI'],
    })
  })

  it('capture_disabled_source_drains_serialized_payload_without_recapturing', async () => {
    const fixture = makeFixture({ firstSaveRejects: true })
    fixture.state.capturedView = viewState(1, [segment('serialized')])
    fixture.source.markViewDirty()
    await fixture.autosave.flush('timer')
    const capturesAfterFailure = fixture.capture.mock.calls.length
    fixture.source.setCaptureEnabled(false)
    fixture.state.capturedView = viewState(2, [segment('hidden')])
    fixture.autosave.start()

    await vi.advanceTimersByTimeAsync(5_000)

    expect(fixture.capture).toHaveBeenCalledTimes(capturesAfterFailure)
    expect(fixture.save.mock.calls[1]![0].viewState?.overlays).toEqual([segment('serialized')])
  })

  it('reports_each_failed_part_once_until_a_successful_retry', async () => {
    const failure = saveResult(key(), 0, 0, {
      klineSaved: false,
      klineError: 'busy',
    })
    const fixture = makeFixture({
      saveResults: [failure, failure, successfulSave(), failure],
    })
    await fixture.autosave.flush('context')
    await fixture.autosave.flush('timer')
    expect(fixture.report).toHaveBeenCalledOnce()
    await fixture.autosave.flush('timer')
    await fixture.autosave.flush('context')

    expect(fixture.report).toHaveBeenCalledTimes(2)
  })
})
