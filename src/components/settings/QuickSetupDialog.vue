<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import CredentialEditor from '../account/CredentialEditor.vue'
import AccountReconciliationStatus from '../account/AccountReconciliationStatus.vue'
import { AppButton, AppDialog } from '../ui'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import { useConfigStore } from '../../stores/config'
import { useConnectionStore } from '../../stores/connection'
import { reportError } from '../../services/errorService'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()
const accountProfilesStore = useAccountProfilesStore()
const configStore = useConfigStore()
const connectionStore = useConnectionStore()
const editorOpen = ref(false)
const credentialsSaved = ref(false)
const connecting = ref(false)
const connectionError = ref<string | null>(null)
let flowSession = 0

const activeAccountId = computed(() => accountProfilesStore.activeAccountId)
const activeProfile = computed(() =>
  accountProfilesStore.profiles.find((profile) => profile.accountId === activeAccountId.value),
)
const profileLabel = computed(() => activeProfile.value?.label ?? activeAccountId.value)
const profileBaseUrl = computed(() =>
  activeProfile.value?.baseUrl ?? 'https://api.easicoin.io',
)
const canOpenEditor = computed(() => Boolean(activeProfile.value)
  && !credentialsSaved.value
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
  if (activeProfile.value.credentialState === 'unavailable') return '凭据存储不可用'
  if (activeProfile.value.credentialState === 'missing') return '需要配置账户凭据'
  return null
})

function resetFlow(): void {
  editorOpen.value = false
  credentialsSaved.value = false
  connecting.value = false
  connectionError.value = null
}

watch(
  () => props.show,
  (visible) => {
    flowSession += 1
    resetFlow()
    if (visible) {
      void accountProfilesStore.refreshProfiles().catch((error) => {
        reportError(error, '加载账户配置失败')
      })
    }
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

function closeFlow(): void {
  flowSession += 1
  resetFlow()
  emit('update:show', false)
}

async function connectStoredCredentials(session = flowSession): Promise<void> {
  connecting.value = true
  connectionError.value = null
  try {
    const config = configStore.config ?? await configStore.fetchConfig()
    await connectionStore.connect(config.useWebsocket)
    if (session === flowSession && props.show) emit('update:show', false)
  } catch (error) {
    if (session === flowSession && props.show) {
      connectionError.value = reportError(error, '凭据已保存，连接失败')
    }
  } finally {
    if (session === flowSession) connecting.value = false
  }
}

function handleCredentialSaved(): void {
  credentialsSaved.value = true
  editorOpen.value = false
  void connectStoredCredentials()
}

function retryConnection(): void {
  if (!credentialsSaved.value || connecting.value) return
  void connectStoredCredentials()
}

function openEditor(): void {
  if (!canOpenEditor.value) return
  connectionError.value = null
  editorOpen.value = true
}

function handleDialogVisibility(visible: boolean): void {
  if (!visible) closeFlow()
}
</script>

<template>
  <AppDialog
    :show="props.show && !editorOpen"
    title="账户快速设置"
    @update:show="handleDialogVisibility"
  >
    <section class="account-summary">
      <div>
        <span>当前账户</span>
        <strong>{{ profileLabel }}</strong>
        <small>{{ activeAccountId }} / {{ profileBaseUrl }}</small>
      </div>
      <AppButton
        v-if="!credentialsSaved"
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
    <p v-if="connecting" class="status-message" role="status">
      正在连接账户…
    </p>
    <p v-if="connectionError" role="alert">
      {{ connectionError }}
    </p>
    <AppButton
      v-if="credentialsSaved && connectionError"
      data-testid="quick-setup-retry"
      :loading="connecting"
      @click="retryConnection"
    >
      重试连接
    </AppButton>
    <template #footer>
      <div v-if="!credentialsSaved" class="footer">
        <AppButton variant="primary" :disabled="!canOpenEditor" @click="openEditor">
          保存凭据并连接
        </AppButton>
      </div>
    </template>
  </AppDialog>
  <CredentialEditor
    v-if="props.show && activeProfile && !credentialsSaved"
    :show="editorOpen
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

<style scoped src="./QuickSetupDialog.css"></style>
