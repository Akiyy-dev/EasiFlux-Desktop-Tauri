<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  NButton,
  NForm,
  NFormItem,
  NInput,
  NModal,
  NSwitch,
  useMessage,
} from 'naive-ui'
import { tauriInvoke } from '../../composables/useTauriCommand'
import { useConfigStore } from '../../stores/config'
import { useConnectionStore } from '../../stores/connection'
import type { ApiCredential } from '../../types/models'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [boolean] }>()

const configStore = useConfigStore()
const connectionStore = useConnectionStore()
const message = useMessage()

const apiKey = ref('')
const apiSecret = ref('')
const baseUrl = ref('https://api.easicoin.io')
const useWebsocket = ref(true)
const testing = ref(false)

watch(
  () => props.show,
  (visible) => {
    if (visible && configStore.config) {
      baseUrl.value = 'https://api.easicoin.io'
      useWebsocket.value = configStore.config.useWebsocket
    }
  },
)

async function save(): Promise<void> {
  const accountId = configStore.config?.activeAccountId ?? 'default'
  await configStore.saveCredentials({
    accountId,
    apiKey: apiKey.value,
    apiSecret: apiSecret.value,
    baseUrl: baseUrl.value,
    label: 'default',
  })
  if (configStore.config) {
    await configStore.saveConfig({ ...configStore.config, useWebsocket: useWebsocket.value })
  }
  message.success('凭据已保存')
}

async function test(): Promise<void> {
  testing.value = true
  try {
    const cred: ApiCredential = {
      apiKey: apiKey.value,
      apiSecret: apiSecret.value,
      baseUrl: baseUrl.value,
      label: 'default',
    }
    await tauriInvoke('test_connection', { credential: cred })
    message.success('连接测试成功')
  } catch (e) {
    message.error(e instanceof Error ? e.message : String(e))
  } finally {
    testing.value = false
  }
}

async function connect(): Promise<void> {
  await save()
  emit('update:show', false)
  await connectionStore.connect(useWebsocket.value)
}
</script>

<template>
  <NModal
    :show="show"
    preset="card"
    title="API 设置"
    style="width: 480px"
    @update:show="emit('update:show', $event)"
  >
    <NForm label-placement="top">
      <NFormItem label="API Key">
        <NInput v-model:value="apiKey" type="password" show-password-on="click" />
      </NFormItem>
      <NFormItem label="API Secret">
        <NInput v-model:value="apiSecret" type="password" show-password-on="click" />
      </NFormItem>
      <NFormItem label="Base URL">
        <NInput v-model:value="baseUrl" />
      </NFormItem>
      <NFormItem label="WebSocket 实时推送">
        <NSwitch v-model:value="useWebsocket" />
      </NFormItem>
    </NForm>
    <template #footer>
      <div class="footer">
        <NButton :loading="testing" @click="test">测试连接</NButton>
        <NButton type="primary" @click="connect">保存并连接</NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
