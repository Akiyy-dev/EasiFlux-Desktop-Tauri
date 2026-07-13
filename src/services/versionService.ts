import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'

const version = ref('')
const initialized = ref(false)

export function useVersionRef() {
  return version
}

export function currentVersion(): string {
  return version.value
}

export async function ensureVersionLoaded(): Promise<void> {
  if (initialized.value && version.value) {
    return
  }
  version.value = await tauriInvoke<string>('get_version')
  initialized.value = true
}

export function applyReadyVersion(next: string): void {
  if (!next) {
    return
  }
  version.value = next
  initialized.value = true
}
