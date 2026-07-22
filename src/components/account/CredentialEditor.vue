<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NForm, NFormItem, NInput } from 'naive-ui'
import { AppButton, AppDialog } from '../ui'
import { tauriInvoke } from '../../composables/useTauriCommand'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import { normalizeAccountId } from '../../utils/account'
import { validateCredentialDraft } from '../../utils/credentials'

const props = withDefaults(defineProps<{
  show: boolean
  mode: 'create' | 'edit'
  accountId: string
  initialLabel: string
  initialBaseUrl: string
  requireCredentials?: boolean
}>(), { requireCredentials: false })
const emit = defineEmits<{
  'update:show': [boolean]
  saved: [string]
}>()

const store = useAccountProfilesStore()
const draftAccountId = ref('')
const label = ref('')
const baseUrl = ref('')
const apiKey = ref('')
const apiSecret = ref('')
const validationError = ref<string | null>(null)
const testing = ref(false)
const hasCompletePair = computed(() => Boolean(apiKey.value.trim() && apiSecret.value.trim()))

watch(() => props.show, (show) => {
  if (!show) return
  draftAccountId.value = props.accountId
  label.value = props.initialLabel
  baseUrl.value = props.initialBaseUrl || 'https://api.easicoin.io'
  apiKey.value = ''
  apiSecret.value = ''
  validationError.value = null
}, { immediate: true })

function validate(): string | null {
  return validateCredentialDraft({
    mode: props.requireCredentials ? 'create' : props.mode,
    apiKey: apiKey.value,
    apiSecret: apiSecret.value,
  })
}

async function save(): Promise<void> {
  validationError.value = validate()
  if (validationError.value) return
  const normalizedId = normalizeAccountId(draftAccountId.value)
  await store.saveCredentials({
    accountId: normalizedId,
    apiKey: apiKey.value.trim(),
    apiSecret: apiSecret.value.trim(),
    label: label.value.trim(),
    baseUrl: baseUrl.value.trim(),
  })
  emit('saved', normalizedId)
  emit('update:show', false)
}

async function testConnection(): Promise<void> {
  if (!hasCompletePair.value) return
  testing.value = true
  try {
    await tauriInvoke('test_connection', {
      credential: {
        apiKey: apiKey.value.trim(),
        apiSecret: apiSecret.value.trim(),
        label: label.value.trim(),
        baseUrl: baseUrl.value.trim(),
      },
    })
  } finally {
    testing.value = false
  }
}
</script>

<template>
  <AppDialog
    :show="show"
    :title="mode === 'create' ? 'Add account' : 'Edit account'"
    @update:show="emit('update:show', $event)"
  >
    <NForm label-placement="top">
      <NFormItem label="Account ID">
        <NInput v-model:value="draftAccountId" :disabled="mode !== 'create'" />
      </NFormItem>
      <NFormItem label="Label">
        <NInput v-model:value="label" />
      </NFormItem>
      <NFormItem label="Base URL">
        <NInput v-model:value="baseUrl" />
      </NFormItem>
      <NFormItem label="API Key">
        <NInput v-model:value="apiKey" type="password" />
      </NFormItem>
      <NFormItem label="API Secret">
        <NInput v-model:value="apiSecret" type="password" />
      </NFormItem>
      <p v-if="validationError" role="alert">
        {{ validationError }}
      </p>
    </NForm>
    <template #footer>
      <div class="actions">
        <AppButton v-if="hasCompletePair" :loading="testing" @click="testConnection">
          Test connection
        </AppButton>
        <AppButton variant="primary" :loading="store.saving" @click="save">
          Save
        </AppButton>
      </div>
    </template>
  </AppDialog>
</template>

<style scoped>
.actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--ef-space-2);
}
[role='alert'] {
  color: var(--ef-color-danger);
}
</style>
