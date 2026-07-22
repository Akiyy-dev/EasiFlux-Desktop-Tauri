import { useTauriEvent } from './useTauriEvent'
import { useAccountProfilesStore } from '../stores/accountProfiles'
import type { AccountSessionEvent } from '../types/models'

export function useAccountSessionEvent<T>(
  event: string,
  handler: (payload: T) => void,
): void {
  const accountProfiles = useAccountProfilesStore()
  useTauriEvent<AccountSessionEvent<T>>(event, (envelope) => {
    accountProfiles.handleSessionEvent(envelope, handler)
  })
}
