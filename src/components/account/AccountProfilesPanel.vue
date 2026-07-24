<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { AppButton, AppDialog } from '../ui'
import CredentialEditor from './CredentialEditor.vue'
import AccountReconciliationStatus from './AccountReconciliationStatus.vue'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import type { AccountProfile, CredentialState } from '../../types/models'

const store = useAccountProfilesStore()
const accountActionsDisabled = computed(() => store.accountMutationsBlocked)
const editorOpen = ref(false)
const editorMode = ref<'create' | 'edit'>('create')
const editing = ref<AccountProfile | null>(null)
const deleteTarget = ref<AccountProfile | null>(null)
const lastOperation = ref<'switch' | 'delete' | null>(null)
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
.profile-list-error {
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
