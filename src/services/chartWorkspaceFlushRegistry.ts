export type ChartWorkspaceFlushReason = 'timer' | 'context' | 'page' | 'close'
export type ChartWorkspaceFlushHandler = (
  reason: ChartWorkspaceFlushReason,
) => Promise<void>

export interface RegisteredChartWorkspaceFlusher {
  setActive(active: boolean): void
  unregister(): void
}

interface Registration {
  handler: ChartWorkspaceFlushHandler
}

const registrations = new Set<Registration>()
let activeRegistration: Registration | null = null

export function registerChartWorkspaceFlusher(
  handler: ChartWorkspaceFlushHandler,
): RegisteredChartWorkspaceFlusher {
  const registration: Registration = { handler }
  registrations.add(registration)

  return {
    setActive(active: boolean): void {
      if (active) {
        activeRegistration = registration
      } else if (activeRegistration === registration) {
        activeRegistration = null
      }
    },
    unregister(): void {
      registrations.delete(registration)
      if (activeRegistration === registration) {
        activeRegistration = null
      }
    },
  }
}

export async function flushActiveChartWorkspace(
  reason: ChartWorkspaceFlushReason,
): Promise<void> {
  const registration = activeRegistration
  if (registration) {
    await registration.handler(reason)
  }
}

export async function flushAllChartWorkspaces(
  reason: Extract<ChartWorkspaceFlushReason, 'close'>,
): Promise<void> {
  const snapshot = [...registrations]
  await Promise.allSettled(snapshot.map(({ handler }) => handler(reason)))
}
