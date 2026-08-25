<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { AppButton, AppDialog } from '../ui'
import CredentialEditor from './CredentialEditor.vue'
import AccountReconciliationStatus from './AccountReconciliationStatus.vue'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import { useConfigStore } from '../../stores/config'
import { useConnectionStore } from '../../stores/connection'
import { reportError } from '../../services/errorService'
import { decodeCommandError } from '../../services/notificationService'
import type { AccountProfile, CredentialState } from '../../types/models'

const store = useAccountProfilesStore()
const configStore = useConfigStore()
const connectionStore = useConnectionStore()
const accountActionsDisabled = computed(() => store.accountMutationsBlocked)
const editorOpen = ref(false)
const editorMode = ref<'create' | 'edit'>('create')
const editing = ref<AccountProfile | null>(null)
const deleteTarget = ref<AccountProfile | null>(null)
const lastOperation = ref<'switch' | 'delete' | null>(null)
const pendingReconnect = ref<{ accountId: string; generation: number } | null>(null)
const reconnectError = ref<string | null>(null)
const reconnectPending = computed(() =>
  pendingReconnect.value?.accountId === store.activeAccountId,
)
let reconnectGeneration = 0
const panelError = computed(() => {
  if (deleteTarget.value) return null
  if (store.switching || store.reconciliationError || store.recoveryError) return null
  return lastOperation.value === 'switch'
    ? store.switchError
    : null
})
const deleteDialogError = computed(() =>
  lastOperation.value === 'delete' ? store.deleteError : null,
)

onMounted(() => void store.refreshProfiles().catch(() => undefined))
watch(
  () => store.switching || Boolean(store.reconciliationError) || store.recoveryRequired,
  (unhealthy) => {
    if (unhealthy) editorOpen.value = false
  },
)
watch(() => store.activeAccountId, () => {
  invalidateReconnect()
}, { flush: 'sync' })
watch(() => connectionStore.connected, (connected, wasConnected) => {
  if (!pendingReconnect.value) return
  if (!connectionStore.reconnecting && (connected || wasConnected)) {
    invalidateReconnect()
  }
}, { flush: 'sync' })

function retryProfileList(): void {
  void store.refreshProfiles().catch(() => undefined)
}

function addAccount(): void {
  editing.value = null
  editorMode.value = 'create'
  editorOpen.value = true
}

function editAccount(profile: AccountProfile): void {
  editing.value = profile
  editorMode.value = 'edit'
  editorOpen.value = true
}

function openDelete(profile: AccountProfile): void {
  deleteTarget.value = profile
  lastOperation.value = 'delete'
}

function closeDelete(): void {
  deleteTarget.value = null
  if (lastOperation.value === 'delete') lastOperation.value = null
}

function credentialStateLabel(state: CredentialState): string {
  return {
    present: '已配置',
    missing: '未配置',
    unavailable: '不可用',
  }[state]
}

function handleCredentialSaved(accountId: string): void {
  const generation = ++reconnectGeneration
  reconnectError.value = null
  pendingReconnect.value = accountId === store.activeAccountId
    && connectionStore.connected
    ? { accountId, generation }
    : null
}

function invalidateReconnect(): void {
  reconnectGeneration += 1
  pendingReconnect.value = null
  reconnectError.value = null
}

function ownsReconnect(attempt: { accountId: string; generation: number }): boolean {
  return reconnectGeneration === attempt.generation
    && pendingReconnect.value?.generation === attempt.generation
    && pendingReconnect.value.accountId === attempt.accountId
    && store.activeAccountId === attempt.accountId
}

async function reconnectActiveAccount(): Promise<void> {
  const attempt = pendingReconnect.value
  if (!attempt || !ownsReconnect(attempt)) {
    invalidateReconnect()
    return
  }
  reconnectError.value = null
  try {
    const config = configStore.config ?? await configStore.fetchConfig()
    if (!ownsReconnect(attempt)) return
    await connectionStore.reconnect(config.useWebsocket, () => ownsReconnect(attempt))
    if (ownsReconnect(attempt)) invalidateReconnect()
  } catch (error) {
    if (!ownsReconnect(attempt)) return
    const decoded = decodeCommandError(error)
    reconnectError.value = decoded.notificationId
      ? decoded.message
      : reportError(error, '账户重新连接失败')
  }
}

async function switchAccount(accountId: string): Promise<void> {
  lastOperation.value = 'switch'
  try {
    await store.switchAccount(accountId)
  } catch {
    return
  }
}

async function confirmDelete(): Promise<void> {
  const target = deleteTarget.value
  if (!target) return
  lastOperation.value = 'delete'
  try {
    await store.deleteAccount(target.accountId)
  } catch {
    return
  }
  closeDelete()
}
</script>

<template>
  <section class="account-profiles">
    <header>
      <h2>账户管理</h2>
      <AppButton :disabled="accountActionsDisabled" @click="addAccount">
        添加账户
      </AppButton>
    </header>
    <AccountReconciliationStatus />
    <div v-if="reconnectPending" class="credential-reconnect">
      <p>凭据已保存，重新连接后生效</p>
      <p v-if="reconnectError" role="alert">
        {{ reconnectError }}
      </p>
      <AppButton
        data-testid="account-reconnect"
        :loading="connectionStore.reconnecting"
        @click="reconnectActiveAccount"
      >
        重新连接
      </AppButton>
    </div>
    <p v-if="panelError" role="alert">
      {{ panelError }}
    </p>
    <div
      v-if="store.listError && !store.switching && !store.reconciliationError
        && !store.recoveryError"
      class="profile-list-error"
    >
      <p role="alert">
        {{ store.listError }}
      </p>
      <AppButton
        data-testid="profile-list-retry"
        size="sm"
        :loading="store.loading"
        @click="retryProfileList"
      >
        重试刷新账户
      </AppButton>
    </div>
    <ul>
      <li v-for="profile in store.profiles" :key="profile.accountId">
        <div>
          <strong>{{ profile.label }}</strong>
          <span>{{ profile.accountId }}</span>
          <span>{{ profile.baseUrl }}</span>
          <span>{{ credentialStateLabel(profile.credentialState) }}</span>
          <span v-if="profile.active">当前账户</span>
        </div>
        <div class="actions">
          <AppButton
            :disabled="accountActionsDisabled || profile.credentialState === 'unavailable'"
            @click="editAccount(profile)"
          >
            编辑
          </AppButton>
          <AppButton
            :disabled="accountActionsDisabled || profile.active
              || profile.credentialState !== 'present'"
            @click="switchAccount(profile.accountId)"
          >
            切换
          </AppButton>
          <AppButton
            :data-testid="`delete-${profile.accountId}`"
            variant="danger"
            :disabled="accountActionsDisabled || profile.active
              || profile.credentialState === 'unavailable'"
            @click="openDelete(profile)"
          >
            删除
          </AppButton>
        </div>
      </li>
    </ul>
  </section>

  <CredentialEditor
    :show="editorOpen && !store.switching && !store.reconciliationError
      && !store.recoveryRequired"
    :mode="editorMode"
    :account-id="editing?.accountId ?? ''"
    :initial-label="editing?.label ?? ''"
    :initial-base-url="editing?.baseUrl ?? 'https://api.easicoin.io'"
    :require-credentials="editing?.credentialState === 'missing'"
    @update:show="editorOpen = $event"
    @saved="handleCredentialSaved"
  />

  <AppDialog
    :show="Boolean(deleteTarget)"
    title="删除账户"
    @update:show="!$event && closeDelete()"
  >
    <p>
      确定删除账户 {{ deleteTarget?.accountId }} 吗？
    </p>
    <p v-if="deleteDialogError" role="alert">
      {{ deleteDialogError }}
    </p>
    <template #footer>
      <AppButton @click="closeDelete">
        取消
      </AppButton>
      <AppButton
        data-testid="confirm-delete"
        variant="danger"
        :loading="store.deleting"
        :disabled="store.accountMutationsBlocked"
        @click="confirmDelete"
      >
        确认删除
      </AppButton>
    </template>
  </AppDialog>
</template>

<style scoped>
.account-profiles header,
.actions,
.profile-list-error,
.credential-reconnect {
  display: flex;
  align-items: center;
  gap: var(--ef-space-2);
}
.account-profiles header {
  justify-content: space-between;
}
.profile-list-error {
  justify-content: space-between;
  color: var(--ef-color-danger);
}
.credential-reconnect {
  justify-content: space-between;
}
.credential-reconnect p {
  margin: 0;
}
ul {
  display: grid;
  gap: var(--ef-space-2);
  margin: 0;
  padding: 0;
  list-style: none;
}
li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--ef-space-3);
}
li > div:first-child {
  display: grid;
  gap: 2px;
}
</style>
