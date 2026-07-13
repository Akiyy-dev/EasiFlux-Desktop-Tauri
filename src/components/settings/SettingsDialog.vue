<script setup lang="ts">
import { ref, watch } from 'vue'
import { NForm, NFormItem, NInput, NInputNumber, NSelect, NSwitch } from 'naive-ui'
import { AppButton, AppDialog } from '../ui'
import { tauriInvoke } from '../../composables/useTauriCommand'
import { useConfigStore } from '../../stores/config'
import { useConnectionStore } from '../../stores/connection'
import { normalizeAccountId } from '../../utils/account'
import type { ApiCredential } from '../../types/models'
import { notifySuccess, notifyWarning, reportError } from '../../services/errorService'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [boolean] }>()

const configStore = useConfigStore()
const connectionStore = useConnectionStore()

const apiKey = ref('')
const apiSecret = ref('')
const baseUrl = ref('https://api.easicoin.io')
const useWebsocket = ref(true)
const tickerPollInterval = ref(1)
const tradingDayTimezone = ref('Asia/Shanghai')
const testing = ref(false)

const timezoneOptions = [
  { label: 'Asia/Shanghai (UTC+8)', value: 'Asia/Shanghai' },
  { label: 'UTC', value: 'UTC' },
  { label: 'America/New_York', value: 'America/New_York' },
  { label: 'Europe/London', value: 'Europe/London' },
]

watch(
  () => props.show,
  (visible) => {
    if (visible && configStore.config) {
      baseUrl.value = 'https://api.easicoin.io'
      useWebsocket.value = configStore.config.useWebsocket
      tickerPollInterval.value = configStore.config.tickerPollInterval
      tradingDayTimezone.value = configStore.config.tradingDayTimezone ?? 'Asia/Shanghai'
    }
  },
)

function buildCredential(): ApiCredential {
  return {
    apiKey: apiKey.value.trim(),
    apiSecret: apiSecret.value.trim(),
    baseUrl: baseUrl.value.trim(),
    label: 'default',
  }
}

async function saveConfigOnly(): Promise<void> {
  if (configStore.config) {
    await configStore.saveConfig({
      ...configStore.config,
      useWebsocket: useWebsocket.value,
      tickerPollInterval: Math.max(1, tickerPollInterval.value),
      tradingDayTimezone: tradingDayTimezone.value,
    })
  }
}

async function save(): Promise<void> {
  const accountId = normalizeAccountId(configStore.config?.activeAccountId)
  const key = apiKey.value.trim()
  const secret = apiSecret.value.trim()

  if (key || secret) {
    await configStore.saveCredentials({
      accountId,
      apiKey: key,
      apiSecret: secret,
      baseUrl: baseUrl.value.trim(),
      label: 'default',
    })
  }

  await saveConfigOnly()
  notifySuccess('设置已保存')
}

async function test(): Promise<void> {
  if (!apiKey.value.trim() || !apiSecret.value.trim()) {
    notifyWarning('测试连接需要填写 API Key 和 Secret')
    return
  }
  testing.value = true
  try {
    await tauriInvoke('test_connection', { credential: buildCredential() })
    notifySuccess('连接测试成功')
  } catch (e) {
    reportError(e)
  } finally {
    testing.value = false
  }
}

async function connect(): Promise<void> {
  try {
    await save()
  } catch (e) {
    reportError(e)
    return
  }
  emit('update:show', false)
  const credential = apiSecret.value.trim() ? buildCredential() : undefined
  try {
    await connectionStore.connect(useWebsocket.value, credential)
  } catch (e) {
    reportError(e)
  }
}
</script>

<template>
  <AppDialog :show="show" title="API 设置" @update:show="emit('update:show', $event)">
    <NForm label-placement="top">
      <NFormItem label="API Key">
        <NInput v-model:value="apiKey" type="password" show-password-on="click" />
      </NFormItem>
      <NFormItem label="API Secret">
        <NInput
          v-model:value="apiSecret"
          type="password"
          show-password-on="click"
          placeholder="留空则保留已保存的 Secret"
        />
      </NFormItem>
      <NFormItem label="Base URL">
        <NInput v-model:value="baseUrl" />
      </NFormItem>
      <NFormItem label="WebSocket 实时推送">
        <NSwitch v-model:value="useWebsocket" />
      </NFormItem>
      <NFormItem label="行情刷新间隔（秒）">
        <NInputNumber v-model:value="tickerPollInterval" :min="1" :step="1" style="width: 100%" />
      </NFormItem>
      <NFormItem label="交易日时区">
        <NSelect v-model:value="tradingDayTimezone" :options="timezoneOptions" />
      </NFormItem>
    </NForm>
    <template #footer>
      <div class="footer">
        <AppButton variant="ghost" :loading="testing" @click="test">测试连接</AppButton>
        <AppButton variant="primary" @click="connect">保存并连接</AppButton>
      </div>
    </template>
  </AppDialog>
</template>

<style scoped>
.footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--ef-space-2);
}
</style>
