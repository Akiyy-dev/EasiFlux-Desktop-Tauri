import type {
  ChartPreferencesV1,
  ChartViewStateV1,
  ChartWorkspaceKey,
  ChartWorkspaceSaveResult,
  ChartWorkspaceSnapshot,
  SaveChartWorkspaceRequest,
} from '../types/chartWorkspace'
import {
  chartPreferencesContentFingerprint,
  chartViewContentFingerprint,
  normalizeChartWorkspaceKey,
} from '../utils/chartWorkspace'
import type { ChartWorkspaceFlushReason } from './chartWorkspaceFlushRegistry'

export interface CapturedWorkspaceState {
  viewState: ChartViewStateV1
  preferences: ChartPreferencesV1
  viewFingerprint: string
  preferencesFingerprint: string
}

export interface ChartWorkspaceCapture {
  capture(
    key: ChartWorkspaceKey,
    viewRevision: number,
    preferencesRevision: number,
  ): CapturedWorkspaceState | null
}

export interface ChartWorkspaceCaptureSource {
  getActiveKey: () => ChartWorkspaceKey
  capture: ChartWorkspaceCapture
}

export interface RegisteredChartWorkspaceCaptureSource {
  setActive(active: boolean): void
  setCaptureEnabled(enabled: boolean): void
  markViewDirty(): void
  markPreferencesDirty(): void
  unregister(): void
}

export interface ChartWorkspaceAutosaveDependencies {
  save: (request: SaveChartWorkspaceRequest) => Promise<ChartWorkspaceSaveResult>
  report: (context: string, error: unknown) => void
  setInterval: typeof globalThis.setInterval
  clearInterval: typeof globalThis.clearInterval
}

interface PendingViewRecord {
  key: ChartWorkspaceKey
  payload: ChartViewStateV1 | null
  viewRevision: number
  viewAckRevision: number
  observedFingerprint: string
  ackFingerprint: string
  forceKlineFlush: boolean
}

interface InternalViewRecord extends PendingViewRecord {
  needsCapture: boolean
  captureRegistration: CaptureRegistration | null
  forceGeneration: number
}

interface PendingPreferencesRecord {
  payload: ChartPreferencesV1 | null
  revision: number
  ackRevision: number
  observedFingerprint: string
  ackFingerprint: string
}

interface InternalPreferencesRecord extends PendingPreferencesRecord {
  needsCapture: boolean
  captureRegistration: CaptureRegistration | null
  captureKeyId: string | null
  keyId: string | null
}

interface CaptureRegistration {
  source: ChartWorkspaceCaptureSource
  captureEnabled: boolean
  removed: boolean
}

interface ActiveCapture {
  registration: CaptureRegistration
  key: ChartWorkspaceKey
  keyId: string
}

interface SaveAttempt {
  key: ChartWorkspaceKey
  keyId: string
  request: SaveChartWorkspaceRequest
  viewRevision: number | null
  viewFingerprint: string | null
  preferencesRevision: number | null
  preferencesFingerprint: string | null
  forceGeneration: number | null
}

const AUTOSAVE_INTERVAL_MS = 5_000

function workspaceId(key: ChartWorkspaceKey): string {
  return `${key.symbol}\u0000${key.interval}`
}

function cloneViewState(viewState: ChartViewStateV1): ChartViewStateV1 {
  return structuredClone(viewState)
}

function clonePreferences(preferences: ChartPreferencesV1): ChartPreferencesV1 {
  return structuredClone(preferences)
}

export class ChartWorkspaceAutosave {
  private readonly dependencies: ChartWorkspaceAutosaveDependencies
  private readonly registrations = new Set<CaptureRegistration>()
  private readonly views = new Map<string, InternalViewRecord>()
  private readonly preferences: InternalPreferencesRecord = {
    payload: null,
    revision: 0,
    ackRevision: 0,
    observedFingerprint: '',
    ackFingerprint: '',
    needsCapture: false,
    captureRegistration: null,
    captureKeyId: null,
    keyId: null,
  }
  private readonly queuedIds = new Set<string>()
  private readonly queue: string[] = []
  private readonly reportedFailures = new Set<string>()
  private activeRegistration: CaptureRegistration | null = null
  private timer: ReturnType<typeof globalThis.setInterval> | null = null
  private drainPromise: Promise<void> | null = null
  private drainRequested = false

  constructor(dependencies: ChartWorkspaceAutosaveDependencies) {
    this.dependencies = dependencies
  }

  registerCaptureSource(
    source: ChartWorkspaceCaptureSource,
  ): RegisteredChartWorkspaceCaptureSource {
    const registration: CaptureRegistration = {
      source,
      captureEnabled: true,
      removed: false,
    }
    this.registrations.add(registration)

    const isCurrent = (): boolean => (
      !registration.removed
      && registration.captureEnabled
      && this.activeRegistration === registration
    )

    return {
      setActive: (active): void => {
        if (registration.removed) return
        if (active) {
          this.activeRegistration = registration
        } else if (this.activeRegistration === registration) {
          this.activeRegistration = null
        }
      },
      setCaptureEnabled: (enabled): void => {
        if (!registration.removed) registration.captureEnabled = enabled
      },
      markViewDirty: (): void => {
        if (!isCurrent()) return
        const active = this.readActiveCapture()
        if (!active || active.registration !== registration) return
        const record = this.ensureView(active.key)
        record.viewRevision = Math.max(record.viewRevision, record.viewAckRevision) + 1
        record.needsCapture = true
        record.captureRegistration = registration
        this.requestFollowupDrain()
      },
      markPreferencesDirty: (): void => {
        if (!isCurrent()) return
        const active = this.readActiveCapture()
        if (!active || active.registration !== registration) return
        this.preferences.revision = Math.max(
          this.preferences.revision,
          this.preferences.ackRevision,
        ) + 1
        this.preferences.needsCapture = true
        this.preferences.captureRegistration = registration
        this.preferences.captureKeyId = active.keyId
        this.requestFollowupDrain()
      },
      unregister: (): void => {
        if (registration.removed) return
        registration.removed = true
        this.registrations.delete(registration)
        if (this.activeRegistration === registration) {
          this.activeRegistration = null
        }
      },
    }
  }

  adoptSnapshot(snapshot: ChartWorkspaceSnapshot): void {
    const key = normalizeChartWorkspaceKey(snapshot.key)
    const keyId = workspaceId(key)
    const record = this.ensureView(key)
    const incomingView = cloneViewState(snapshot.viewState)
    const incomingViewFingerprint = chartViewContentFingerprint(incomingView)

    if (incomingView.revision >= record.viewRevision) {
      record.payload = null
      record.viewRevision = incomingView.revision
      record.viewAckRevision = incomingView.revision
      record.observedFingerprint = incomingViewFingerprint
      record.ackFingerprint = incomingViewFingerprint
      record.needsCapture = false
      record.captureRegistration = null
    } else if (incomingView.revision >= record.viewAckRevision) {
      record.viewAckRevision = incomingView.revision
      record.ackFingerprint = incomingViewFingerprint
    }

    const incomingPreferences = clonePreferences(snapshot.preferences)
    const incomingPreferencesFingerprint = chartPreferencesContentFingerprint(incomingPreferences)
    if (incomingPreferences.revision >= this.preferences.revision) {
      const oldKeyId = this.preferences.keyId
      this.preferences.payload = null
      this.preferences.revision = incomingPreferences.revision
      this.preferences.ackRevision = incomingPreferences.revision
      this.preferences.observedFingerprint = incomingPreferencesFingerprint
      this.preferences.ackFingerprint = incomingPreferencesFingerprint
      this.preferences.needsCapture = false
      this.preferences.captureRegistration = null
      this.preferences.captureKeyId = null
      this.preferences.keyId = null
      if (oldKeyId) this.removeFromQueueIfClean(oldKeyId)
    } else if (incomingPreferences.revision >= this.preferences.ackRevision) {
      this.preferences.ackRevision = incomingPreferences.revision
      this.preferences.ackFingerprint = incomingPreferencesFingerprint
    }

    this.removeFromQueueIfClean(keyId)
  }

  start(): void {
    if (this.timer !== null) return
    this.timer = this.dependencies.setInterval(() => {
      void this.flush('timer')
    }, AUTOSAVE_INTERVAL_MS)
  }

  stop(): void {
    if (this.timer === null) return
    this.dependencies.clearInterval(this.timer)
    this.timer = null
  }

  async flush(reason: ChartWorkspaceFlushReason): Promise<void> {
    const active = this.captureActive()
    if (reason !== 'timer' && active) {
      const record = this.ensureView(active.key)
      record.forceKlineFlush = true
      record.forceGeneration += 1
      this.enqueue(active.keyId)
    }

    this.drainRequested = true
    if (!this.drainPromise) {
      const draining = this.drain()
      this.drainPromise = draining
      void draining.finally(() => {
        if (this.drainPromise === draining) this.drainPromise = null
      })
    }
    await this.drainPromise
  }

  private requestFollowupDrain(): void {
    if (this.drainPromise) this.drainRequested = true
  }

  private readActiveCapture(): ActiveCapture | null {
    const registration = this.activeRegistration
    if (!registration || registration.removed) return null
    try {
      const key = normalizeChartWorkspaceKey(registration.source.getActiveKey())
      return { registration, key, keyId: workspaceId(key) }
    } catch (error) {
      this.reportFailure('capture', { symbol: 'unknown', interval: 'unknown' }, 'key', error)
      return null
    }
  }

  private captureActive(): ActiveCapture | null {
    const active = this.readActiveCapture()
    if (!active || !active.registration.captureEnabled) return active
    const record = this.ensureView(active.key)
    const initial = this.readCapture(active, record.viewRevision, this.preferences.revision)
    if (!initial) return active

    const initialViewFingerprint = initial.viewFingerprint
    const initialPreferencesFingerprint = initial.preferencesFingerprint
    if (!record.needsCapture && initialViewFingerprint !== record.observedFingerprint) {
      record.viewRevision = Math.max(record.viewRevision, record.viewAckRevision) + 1
      record.needsCapture = true
      record.captureRegistration = active.registration
    }
    if (!this.preferences.needsCapture
      && initialPreferencesFingerprint !== this.preferences.observedFingerprint) {
      this.preferences.revision = Math.max(
        this.preferences.revision,
        this.preferences.ackRevision,
      ) + 1
      this.preferences.needsCapture = true
      this.preferences.captureRegistration = active.registration
      this.preferences.captureKeyId = active.keyId
    }

    const capturesView = record.needsCapture
      && record.captureRegistration === active.registration
    const capturesPreferences = this.preferences.needsCapture
      && this.preferences.captureRegistration === active.registration
      && this.preferences.captureKeyId === active.keyId
    let captured = initial
    if ((capturesView && initial.viewState.revision !== record.viewRevision)
      || (capturesPreferences
        && initial.preferences.revision !== this.preferences.revision)) {
      const recaptured = this.readCapture(active, record.viewRevision, this.preferences.revision)
      if (!recaptured) return active
      captured = recaptured
    }

    if (capturesView) {
      const payload = cloneViewState(captured.viewState)
      payload.revision = record.viewRevision
      record.payload = payload
      record.observedFingerprint = captured.viewFingerprint
      record.needsCapture = false
      record.captureRegistration = null
      this.enqueue(active.keyId)
    }

    if (capturesPreferences) {
      const oldKeyId = this.preferences.keyId
      const payload = clonePreferences(captured.preferences)
      payload.revision = this.preferences.revision
      this.preferences.payload = payload
      this.preferences.observedFingerprint = captured.preferencesFingerprint
      this.preferences.needsCapture = false
      this.preferences.captureRegistration = null
      this.preferences.captureKeyId = null
      this.preferences.keyId = active.keyId
      this.enqueue(active.keyId)
      if (oldKeyId && oldKeyId !== active.keyId) this.removeFromQueueIfClean(oldKeyId)
    }

    return active
  }

  private readCapture(
    active: ActiveCapture,
    viewRevision: number,
    preferencesRevision: number,
  ): CapturedWorkspaceState | null {
    let captured: CapturedWorkspaceState | null
    try {
      captured = active.registration.source.capture.capture(
        active.key,
        viewRevision,
        preferencesRevision,
      )
    } catch (error) {
      this.reportFailure('capture', active.key, 'source', error)
      return null
    }
    if (!captured) return null
    if (captured.viewState.schemaVersion !== 1
      || captured.preferences.schemaVersion !== 1
      || captured.viewState.symbol !== active.key.symbol
      || captured.viewState.interval !== active.key.interval) {
      this.reportFailure(
        'capture',
        active.key,
        'compatibility',
        new Error('Captured chart workspace is incompatible with the active key or schema'),
      )
      return null
    }
    this.clearFailure('capture', active.key, 'source')
    this.clearFailure('capture', active.key, 'compatibility')
    return captured
  }

  private async drain(): Promise<void> {
    do {
      this.drainRequested = false
      this.captureActive()
      const cycle = [...this.queue]
      for (const keyId of cycle) {
        const attempt = this.createAttempt(keyId)
        if (!attempt) {
          this.removeFromQueueIfClean(keyId)
          continue
        }
        try {
          const result = await this.dependencies.save(attempt.request)
          this.applyResult(attempt, result)
        } catch (error) {
          this.reportAttemptFailure(attempt, error)
          continue
        }
      }
    } while (this.drainRequested)
  }

  private createAttempt(keyId: string): SaveAttempt | null {
    const record = this.views.get(keyId)
    if (!record) return null
    const viewState = record.payload ? cloneViewState(record.payload) : null
    const carriesPreferences = this.preferences.payload !== null
      && this.preferences.keyId === keyId
    const preferences = carriesPreferences && this.preferences.payload
      ? clonePreferences(this.preferences.payload)
      : null
    const forceGeneration = record.forceKlineFlush ? record.forceGeneration : null
    if (!viewState && !preferences && forceGeneration === null) return null
    return {
      key: { ...record.key },
      keyId,
      request: {
        key: { ...record.key },
        viewState,
        preferences,
      },
      viewRevision: viewState?.revision ?? null,
      viewFingerprint: viewState ? record.observedFingerprint : null,
      preferencesRevision: preferences?.revision ?? null,
      preferencesFingerprint: preferences ? this.preferences.observedFingerprint : null,
      forceGeneration,
    }
  }

  private applyResult(attempt: SaveAttempt, result: ChartWorkspaceSaveResult): void {
    const record = this.views.get(attempt.keyId)
    if (!record) return

    if (attempt.forceGeneration !== null) {
      if (result.klineSaved) {
        this.clearFailure('save', attempt.key, 'kline')
        if (record.forceGeneration === attempt.forceGeneration) {
          record.forceKlineFlush = false
        }
      } else {
        this.reportFailure('save', attempt.key, 'kline', result.klineError)
      }
    }

    if (attempt.viewRevision !== null && attempt.viewFingerprint !== null) {
      const stillCurrent = record.payload !== null
        && !record.needsCapture
        && record.viewRevision === attempt.viewRevision
        && record.payload.revision === attempt.viewRevision
        && record.observedFingerprint === attempt.viewFingerprint
      if (result.viewStateSaved && result.viewRevision >= attempt.viewRevision) {
        this.clearFailure('save', attempt.key, 'view')
        if (stillCurrent) {
          record.payload = null
          record.viewRevision = result.viewRevision
          record.viewAckRevision = result.viewRevision
          record.ackFingerprint = attempt.viewFingerprint
        }
      } else {
        this.reportFailure(
          'save',
          attempt.key,
          'view',
          result.viewStateError ?? new Error('View state durable revision did not cover the attempt'),
        )
        if (stillCurrent && result.viewRevision >= attempt.viewRevision) {
          const revision = result.viewRevision + 1
          record.viewRevision = revision
          record.payload = { ...record.payload!, revision }
          this.drainRequested = true
        }
      }
    }

    if (attempt.preferencesRevision !== null && attempt.preferencesFingerprint !== null) {
      const stillCurrent = this.preferences.payload !== null
        && !this.preferences.needsCapture
        && this.preferences.revision === attempt.preferencesRevision
        && this.preferences.keyId === attempt.keyId
        && this.preferences.payload.revision === attempt.preferencesRevision
        && this.preferences.observedFingerprint === attempt.preferencesFingerprint
      if (result.preferencesSaved
        && result.preferencesRevision >= attempt.preferencesRevision) {
        this.clearFailure('save', attempt.key, 'preferences')
        if (stillCurrent) {
          this.preferences.payload = null
          this.preferences.revision = result.preferencesRevision
          this.preferences.ackRevision = result.preferencesRevision
          this.preferences.ackFingerprint = attempt.preferencesFingerprint
          this.preferences.keyId = null
        }
      } else {
        this.reportFailure(
          'save',
          attempt.key,
          'preferences',
          result.preferencesError
            ?? new Error('Preferences durable revision did not cover the attempt'),
        )
        if (stillCurrent && result.preferencesRevision >= attempt.preferencesRevision) {
          const revision = result.preferencesRevision + 1
          this.preferences.revision = revision
          this.preferences.payload = { ...this.preferences.payload!, revision }
          this.drainRequested = true
        }
      }
    }

    this.removeFromQueueIfClean(attempt.keyId)
  }

  private reportAttemptFailure(attempt: SaveAttempt, error: unknown): void {
    if (attempt.forceGeneration !== null) {
      this.reportFailure('save', attempt.key, 'kline', error)
    }
    if (attempt.viewRevision !== null) {
      this.reportFailure('save', attempt.key, 'view', error)
    }
    if (attempt.preferencesRevision !== null) {
      this.reportFailure('save', attempt.key, 'preferences', error)
    }
  }

  private ensureView(key: ChartWorkspaceKey): InternalViewRecord {
    const normalized = normalizeChartWorkspaceKey(key)
    const id = workspaceId(normalized)
    const existing = this.views.get(id)
    if (existing) return existing
    const record: InternalViewRecord = {
      key: normalized,
      payload: null,
      viewRevision: 0,
      viewAckRevision: 0,
      observedFingerprint: '',
      ackFingerprint: '',
      forceKlineFlush: false,
      needsCapture: false,
      captureRegistration: null,
      forceGeneration: 0,
    }
    this.views.set(id, record)
    return record
  }

  private enqueue(keyId: string): void {
    if (this.queuedIds.has(keyId)) return
    this.queuedIds.add(keyId)
    this.queue.push(keyId)
  }

  private removeFromQueueIfClean(keyId: string): void {
    const record = this.views.get(keyId)
    const hasViewWork = Boolean(record?.payload || record?.forceKlineFlush)
    const hasPreferencesWork = this.preferences.payload !== null
      && this.preferences.keyId === keyId
    if (hasViewWork || hasPreferencesWork || !this.queuedIds.delete(keyId)) return
    const index = this.queue.indexOf(keyId)
    if (index >= 0) this.queue.splice(index, 1)
  }

  private reportFailure(
    operation: string,
    key: ChartWorkspaceKey,
    part: string,
    error: unknown,
  ): void {
    const latch = `${operation}|${workspaceId(key)}|${part}`
    if (this.reportedFailures.has(latch)) return
    this.reportedFailures.add(latch)
    this.dependencies.report(`chart workspace ${operation} ${part}`, error)
  }

  private clearFailure(operation: string, key: ChartWorkspaceKey, part: string): void {
    this.reportedFailures.delete(`${operation}|${workspaceId(key)}|${part}`)
  }
}
