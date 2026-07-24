<script setup lang="ts">
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import { AppButton } from '../ui'

const store = useAccountProfilesStore()
</script>

<template>
  <div
    v-if="store.recoveryError"
    data-testid="account-recovery-required"
    role="alert"
  >
    <span>{{ store.recoveryError }}</span>
  </div>
  <p v-else-if="store.reconciliationLoading" role="status">
    正在同步已切换的账户…
  </p>
  <div
    v-else-if="store.reconciliationError"
    data-testid="reconciliation-error"
    role="alert"
  >
    <span>{{ store.reconciliationError }}</span>
    <AppButton
      data-testid="reconciliation-retry"
      size="sm"
      @click="store.retryReconciliation"
    >
      重试同步
    </AppButton>
  </div>
</template>

<style scoped>
p {
  color: var(--muted-foreground);
}

div {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--ef-space-2);
  color: var(--ef-color-danger);
}
</style>
