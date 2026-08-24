<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { AppButton, AppDialog } from '../ui'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import { useNotificationStore } from '../../stores/notification'
import type { NotificationSettings } from '../../types/notification'

type ToggleKey = keyof NotificationSettings

interface ClearTarget {
  accountId: string
  displayName: string
}

const notificationStore = useNotificationStore()
const accountProfilesStore = useAccountProfilesStore()
const confirmationInput = ref<{ focus: () => void } | null>(null)
const confirmationText = ref('')
const clearTarget = ref<ClearTarget | null>(null)
const clearError = ref<string | null>(null)

const currentAccountDisplayName = computed(() => {
  const accountId = notificationStore.accountId
  if (accountId === null) return 'Global'
  const profile = accountProfilesStore.profiles.find((item) => item.accountId === accountId)
  return profile ? `${profile.label}（${accountId}）` : accountId
})
const clearContextChanged = computed(() => clearTarget.value !== null
  && notificationStore.accountId !== clearTarget.value.accountId)
const canConfirmClear = computed(() => clearTarget.value !== null
  && confirmationText.value === clearTarget.value.accountId
  && !clearContextChanged.value
  && !notificationStore.clearCurrentAccountPending)
const statusText = computed(() => {
  if (notificationStore.settingsSaveStatus === 'loading') return '正在加载通知设置'
  if (notificationStore.settingsSaveStatus === 'saving') return '正在保存通知设置'
  if (notificationStore.settingsSaveStatus === 'saved') return '通知设置已保存'
  return null
})

function updateToggle(key: ToggleKey, value: boolean): void {
  const draft = notificationStore.settingsDraft
  if (draft === null) return
  void notificationStore.updateSettings({ ...draft, [key]: value })
}

function updateToggleFromEvent(key: ToggleKey, event: { target: unknown }): void {
  const target = event.target
  if (
    typeof target !== 'object'
    || target === null
    || !('checked' in target)
    || typeof target.checked !== 'boolean'
  ) return
  updateToggle(key, target.checked)
}

function retryLoad(): void {
  if (notificationStore.settingsDraft === null) void notificationStore.loadSettings()
}

function retrySave(): void {
  const draft = notificationStore.settingsDraft
  if (draft !== null) void notificationStore.updateSettings({ ...draft })
}

function openClearConfirmation(): void {
  const accountId = notificationStore.accountId
  if (accountId === null) return
  clearTarget.value = { accountId, displayName: currentAccountDisplayName.value }
  confirmationText.value = ''
  clearError.value = null
  void nextTick(() => confirmationInput.value?.focus())
}

function closeClearConfirmation(): void {
  if (notificationStore.clearCurrentAccountPending) return
  clearTarget.value = null
  confirmationText.value = ''
  clearError.value = null
}

async function confirmClear(): Promise<void> {
  if (!canConfirmClear.value) return
  const cleared = await notificationStore.clearCurrentAccount()
  if (cleared) {
    closeClearConfirmation()
    return
  }
  clearError.value = notificationStore.error
}

onMounted(() => {
  if (notificationStore.settingsDraft === null) void notificationStore.loadSettings()
})
</script>

<template>
  <section class="notification-settings-panel" aria-labelledby="notification-settings-title">
    <header class="notification-settings-header">
      <div>
        <h2 id="notification-settings-title">
          通知设置
        </h2>
        <p>调整实时应用内 Toast 偏好。更改会自动保存。</p>
      </div>
      <span
        v-if="statusText"
        class="notification-settings-status"
        data-testid="notification-settings-status"
        role="status"
        aria-live="polite"
      >
        {{ statusText }}
      </span>
    </header>

    <template v-if="notificationStore.settingsDraft === null">
      <div v-if="notificationStore.settingsSaveStatus === 'error'" class="notification-settings-error-row">
        <p data-testid="notification-settings-load-error" role="alert">
          加载失败：{{ notificationStore.settingsError }}
        </p>
        <AppButton data-testid="notification-settings-load-retry" aria-label="重试加载通知设置" @click="retryLoad">
          重试加载
        </AppButton>
      </div>
      <p v-else class="notification-settings-muted" role="status" aria-live="polite">
        正在加载通知设置
      </p>
    </template>

    <template v-else>
      <div class="notification-settings-switches">
        <label class="notification-settings-switch-row" for="notification-toggle-trading">
          <span>交易 Toast</span>
          <input id="notification-toggle-trading" data-testid="notification-toggle-trading" type="checkbox" role="switch" :checked="notificationStore.settingsDraft.tradingToast" aria-label="交易实时应用内 Toast" @change="updateToggleFromEvent('tradingToast', $event)">
        </label>
        <label class="notification-settings-switch-row" for="notification-toggle-risk-account">
          <span>风险与账户 Toast</span>
          <input id="notification-toggle-risk-account" data-testid="notification-toggle-risk-account" type="checkbox" role="switch" :checked="notificationStore.settingsDraft.riskAccountToast" aria-label="风险与账户实时应用内 Toast" @change="updateToggleFromEvent('riskAccountToast', $event)">
        </label>
        <label class="notification-settings-switch-row" for="notification-toggle-connection-system">
          <span>连接与系统 Toast</span>
          <input id="notification-toggle-connection-system" data-testid="notification-toggle-connection-system" type="checkbox" role="switch" :checked="notificationStore.settingsDraft.connectionSystemToast" aria-label="连接与系统实时应用内 Toast" @change="updateToggleFromEvent('connectionSystemToast', $event)">
        </label>
      </div>

      <div v-if="notificationStore.settingsSaveStatus === 'error'" class="notification-settings-error-row">
        <p data-testid="notification-settings-save-error" role="alert">
          保存失败：{{ notificationStore.settingsError }}
        </p>
        <AppButton data-testid="notification-settings-save-retry" aria-label="重试保存通知设置" @click="retrySave">
          重试保存
        </AppButton>
      </div>
    </template>

    <section class="notification-settings-clear" aria-labelledby="notification-clear-title">
      <h3 id="notification-clear-title">
        清理当前账户通知
      </h3>
      <p v-if="notificationStore.accountId === null">
        当前为 Global-only；Global 通知不会被批量清理。
      </p>
      <p v-else>
        将仅清理 {{ currentAccountDisplayName }} 的通知；Global 通知会保留。
      </p>
      <AppButton data-testid="notification-clear-current-account" variant="danger" :loading="notificationStore.clearCurrentAccountPending" :disabled="notificationStore.accountId === null || notificationStore.clearCurrentAccountPending" @click="openClearConfirmation">
        清理当前账户通知
      </AppButton>
    </section>

    <AppDialog :show="clearTarget !== null" title="清理当前账户通知" @update:show="!$event && closeClearConfirmation()">
      <template v-if="clearTarget">
        <p>将仅清理 {{ clearTarget.displayName }} 的通知；Global 通知会保留。</p>
        <p>请输入精确账户 ID <code>{{ clearTarget.accountId }}</code> 以确认。</p>
        <p v-if="clearContextChanged" class="notification-settings-error" role="alert">
          账户上下文已变更，无法清理已捕获的账户目标。
        </p>
        <p v-if="clearError" data-testid="notification-clear-error" class="notification-settings-error" role="alert">
          {{ clearError }}
        </p>
        <input ref="confirmationInput" v-model="confirmationText" data-testid="notification-clear-confirmation-input" :disabled="notificationStore.clearCurrentAccountPending" :aria-label="`输入 ${clearTarget.accountId} 以确认清理当前账户通知`">
      </template>
      <template #footer>
        <AppButton data-testid="notification-clear-cancel" :disabled="notificationStore.clearCurrentAccountPending" @click="closeClearConfirmation">
          取消
        </AppButton>
        <AppButton data-testid="notification-clear-confirm" variant="danger" :loading="notificationStore.clearCurrentAccountPending" :disabled="!canConfirmClear" @click="confirmClear">
          确认清理
        </AppButton>
      </template>
    </AppDialog>
  </section>
</template>

<style scoped src="./NotificationSettingsPanel.css"></style>
