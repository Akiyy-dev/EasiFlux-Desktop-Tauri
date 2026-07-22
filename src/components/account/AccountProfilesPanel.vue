<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { AppButton, AppDialog } from '../ui'
import CredentialEditor from './CredentialEditor.vue'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import type { AccountProfile } from '../../types/models'

const store = useAccountProfilesStore()
const editorOpen = ref(false)
const editorMode = ref<'create' | 'edit'>('create')
const editing = ref<AccountProfile | null>(null)
const deleteTarget = ref<AccountProfile | null>(null)

onMounted(() => void store.refreshProfiles())

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

async function confirmDelete(): Promise<void> {
  const target = deleteTarget.value
  if (!target) return
  await store.deleteAccount(target.accountId)
  deleteTarget.value = null
}
</script>

<template>
  <section class="account-profiles">
    <header>
      <h2>Account profiles</h2>
      <AppButton :disabled="store.mutating" @click="addAccount">
        Add account
      </AppButton>
    </header>
    <p v-if="store.listError" role="alert">
      {{ store.listError }}
    </p>
    <ul>
      <li v-for="profile in store.profiles" :key="profile.accountId">
        <div>
          <strong>{{ profile.label }}</strong>
          <span>{{ profile.accountId }}</span>
          <span>{{ profile.baseUrl }}</span>
          <span>{{ profile.credentialState }}</span>
          <span v-if="profile.active">active</span>
        </div>
        <div class="actions">
          <AppButton
            :disabled="store.mutating || profile.credentialState === 'unavailable'"
            @click="editAccount(profile)"
          >
            Edit
          </AppButton>
          <AppButton
            :disabled="store.mutating || profile.active || profile.credentialState !== 'present'"
            @click="store.switchAccount(profile.accountId)"
          >
            Switch
          </AppButton>
          <AppButton
            :data-testid="`delete-${profile.accountId}`"
            variant="danger"
            :disabled="store.mutating || profile.active || profile.credentialState === 'unavailable'"
            @click="deleteTarget = profile"
          >
            Delete
          </AppButton>
        </div>
      </li>
    </ul>
  </section>

  <CredentialEditor
    :show="editorOpen"
    :mode="editorMode"
    :account-id="editing?.accountId ?? ''"
    :initial-label="editing?.label ?? ''"
    :initial-base-url="editing?.baseUrl ?? 'https://api.easicoin.io'"
    :require-credentials="editing?.credentialState === 'missing'"
    @update:show="editorOpen = $event"
  />

  <AppDialog
    :show="Boolean(deleteTarget)"
    title="Delete account"
    @update:show="!$event && (deleteTarget = null)"
  >
    <p>
      Delete account {{ deleteTarget?.accountId }}?
    </p>
    <template #footer>
      <AppButton @click="deleteTarget = null">
        Cancel
      </AppButton>
      <AppButton
        data-testid="confirm-delete"
        variant="danger"
        :loading="store.deleting"
        @click="confirmDelete"
      >
        Confirm delete
      </AppButton>
    </template>
  </AppDialog>
</template>

<style scoped>
.account-profiles header,
.actions {
  display: flex;
  align-items: center;
  gap: var(--ef-space-2);
}
.account-profiles header {
  justify-content: space-between;
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
