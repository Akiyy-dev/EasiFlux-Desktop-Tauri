<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NForm, NFormItem, NInputNumber, NSwitch } from 'naive-ui'
import CredentialEditor from '../account/CredentialEditor.vue'
import AccountReconciliationStatus from '../account/AccountReconciliationStatus.vue'
import { AppButton, AppDialog } from '../ui'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import { useConfigStore } from '../../stores/config'
import { useConnectionStore } from '../../stores/connection'
import { notifySuccess, reportError } from '../../services/errorService'
import type { AppConfig } from '../../types/models'
const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [boolean] }>()
const accountProfilesStore = useAccountProfilesStore()
const configStore = useConfigStore()
const connectionStore = useConnectionStore()
const editorOpen = ref(false)
const applyingCredentialSave = ref(false)
const useWebsocket = ref(true)
const tickerPollInterval = ref(1)
const flowError = ref<string | null>(null)
const activeAccountId = computed(() => accountProfilesStore.activeAccountId)
const activeProfile = computed(() =>
  accountProfilesStore.profiles.find((profile) => profile.accountId === activeAccountId.value),
)
const profileLabel = computed(() => activeProfile.value?.label ?? activeAccountId.value)
const profileBaseUrl = computed(() =>
  activeProfile.value?.baseUrl ?? 'https://api.easicoin.io',
)
const canOpenEditor = computed(() => Boolean(activeProfile.value)
  && !accountProfilesStore.switching
  && !accountProfilesStore.loading
  && !accountProfilesStore.listError
  && !accountProfilesStore.reconciliationLoading
  && !accountProfilesStore.reconciliationError
  && !accountProfilesStore.recoveryRequired
  && activeProfile.value?.credentialState !== 'unavailable')
const profileStatus = computed(() => {
  if (accountProfilesStore.switching && !accountProfilesStore.reconciliationLoading) {
    return 'Switching account'
  }
  if (accountProfilesStore.reconciliationLoading || accountProfilesStore.reconciliationError) {
    return null
  }
  if (accountProfilesStore.loading) return 'Loading account profile'
  if (!activeProfile.value) return 'Active account profile is unavailable'
  if (activeProfile.value.credentialState === 'unavailable') {
    return 'Credential storage is unavailable'
  }
  if (activeProfile.value.credentialState === 'missing') return 'Credentials are required'
  return null
})
watch(
  () => props.show,
  (visible) => {
    if (!visible) {
      editorOpen.value = false
      applyingCredentialSave.value = false
      flowError.value = null
      return
    }
    editorOpen.value = false
    applyingCredentialSave.value = false
    flowError.value = null
    if (configStore.config) {
      useWebsocket.value = configStore.config.useWebsocket
      tickerPollInterval.value = configStore.config.tickerPollInterval
    }
    void accountProfilesStore.refreshProfiles().catch(reportError)
  },
  { immediate: true },
)
watch(
  () => accountProfilesStore.switching
    || Boolean(accountProfilesStore.reconciliationError)
    || accountProfilesStore.recoveryRequired,
  (unhealthy) => {
    if (unhealthy) editorOpen.value = false
  },
)
async function saveGeneralSettings(): Promise<void> {
  if (!configStore.config) {
    await configStore.fetchConfig()
  }
  const currentConfig = configStore.config as AppConfig | null
  if (!currentConfig) throw new Error('Settings configuration is unavailable')
  await configStore.saveConfig({
    ...currentConfig,
    useWebsocket: useWebsocket.value,
    tickerPollInterval: Math.max(1, tickerPollInterval.value),
  })
}
async function handleCredentialSaved(): Promise<void> {
  applyingCredentialSave.value = true
  flowError.value = null
  try {
    await saveGeneralSettings()
    await connectionStore.connect(useWebsocket.value)
    notifySuccess('Settings saved')
    emit('update:show', false)
  } catch (error) {
    flowError.value = reportError(error)
    applyingCredentialSave.value = false
    editorOpen.value = false
  }
}
function openEditor(): void {
  if (!canOpenEditor.value) return
  flowError.value = null
  editorOpen.value = true
}
</script>
<template>
  <AppDialog
    :show="props.show && !editorOpen && !applyingCredentialSave"
    title="API settings"
    @update:show="emit('update:show', $event)"
  >
    <section class="account-summary">
      <div>
        <span>Current account</span>
        <strong>{{ profileLabel }}</strong>
        <small>{{ activeAccountId }} / {{ profileBaseUrl }}</small>
      </div>
      <AppButton
        :disabled="!canOpenEditor"
        @click="openEditor"
      >
        Edit credentials
      </AppButton>
    </section>
    <AccountReconciliationStatus />
    <p
      v-if="!accountProfilesStore.switching
        && !accountProfilesStore.reconciliationError
        && accountProfilesStore.listError"
      role="alert"
    >
      {{ accountProfilesStore.listError }}
    </p>
    <p v-else-if="profileStatus" class="status-message">
      {{ profileStatus }}
    </p>
    <p v-if="flowError" role="alert">
      {{ flowError }}
    </p>
    <NForm label-placement="top">
      <NFormItem label="WebSocket realtime updates">
        <NSwitch v-model:value="useWebsocket" />
      </NFormItem>
      <NFormItem label="Ticker polling interval (seconds)">
        <NInputNumber
          v-model:value="tickerPollInterval"
          :min="1"
          :step="1"
          style="width: 100%"
        />
      </NFormItem>
    </NForm>
    <template #footer>
      <div class="footer">
        <AppButton variant="primary" :disabled="!canOpenEditor" @click="openEditor">
          Save credentials and connect
        </AppButton>
      </div>
    </template>
  </AppDialog>
  <CredentialEditor
    v-if="activeProfile"
    :show="editorOpen
      && !applyingCredentialSave
      && !accountProfilesStore.switching
      && !accountProfilesStore.reconciliationError
      && !accountProfilesStore.recoveryRequired"
    mode="edit"
    :account-id="activeProfile.accountId"
    :initial-label="activeProfile.label"
    :initial-base-url="activeProfile.baseUrl"
    :require-credentials="activeProfile.credentialState === 'missing'"
    @saved="handleCredentialSaved"
    @update:show="editorOpen = $event"
  />
</template>
<style scoped src="./SettingsDialog.css"></style>
