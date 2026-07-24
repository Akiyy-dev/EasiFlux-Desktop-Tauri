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
    return '正在切换账户'
  }
  if (accountProfilesStore.reconciliationLoading || accountProfilesStore.reconciliationError) {
    return null
  }
  if (accountProfilesStore.loading) return '正在加载账户配置'
  if (!activeProfile.value) return '当前账户配置不可用'
  if (activeProfile.value.credentialState === 'unavailable') {
    return '凭据存储不可用'
  }
  if (activeProfile.value.credentialState === 'missing') return '需要配置账户凭据'
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
  if (!currentConfig) throw new Error('设置配置不可用')
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
    notifySuccess('设置已保存')
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
    title="API 设置"
    @update:show="emit('update:show', $event)"
  >
    <section class="account-summary">
      <div>
        <span>当前账户</span>
        <strong>{{ profileLabel }}</strong>
        <small>{{ activeAccountId }} / {{ profileBaseUrl }}</small>
      </div>
      <AppButton
        :disabled="!canOpenEditor"
        @click="openEditor"
      >
        编辑凭据
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
      <NFormItem label="WebSocket 实时更新">
        <NSwitch v-model:value="useWebsocket" />
      </NFormItem>
      <NFormItem label="行情轮询间隔（秒）">
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
          保存凭据并连接
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
