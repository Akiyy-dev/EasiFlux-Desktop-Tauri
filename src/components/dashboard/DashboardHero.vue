<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import AppCard from '../ui/AppCard.vue'
import ConnectionStatus from '../common/ConnectionStatus.vue'
import MonoValue from '../ui/MonoValue.vue'
import { useAppStore } from '../../stores/app'
import { useConnectionStore } from '../../stores/connection'

const connectionStore = useConnectionStore()
const appStore = useAppStore()
const { connected } = storeToRefs(connectionStore)
const { environment, environmentLoading, environmentError, version } = storeToRefs(appStore)

const environmentLabel = computed(() => {
  if (environmentLoading.value) {
    return '检测中'
  }
  if (environmentError.value) {
    return `检测失败: ${environmentError.value}`
  }
  const payload = environment.value?.data
  if (!payload) {
    return '未检测'
  }
  if (!payload.reachable) {
    return payload.error ? `检测失败: ${payload.error}` : '检测失败'
  }
  return payload.label
})
</script>

<template>
  <AppCard class="hero" flush>
    <div class="hero-inner">
      <div class="brand-block">
        <div class="logo-mark" aria-hidden="true">EF</div>
        <div class="brand-text">
          <h1 class="name">EasiFlux</h1>
          <p class="tagline">EasiCoin 合约交易桌面客户端</p>
        </div>
      </div>

      <div class="meta">
        <div class="meta-item">
          <span class="meta-label">版本</span>
          <MonoValue class="meta-value" size="sm">v{{ version }}</MonoValue>
        </div>
        <div class="meta-item">
          <span class="meta-label">环境</span>
          <span class="meta-value env">{{ environmentLabel }}</span>
        </div>
        <div class="meta-item status">
          <span class="meta-label">连接</span>
          <ConnectionStatus />
        </div>
        <div v-if="connected" class="meta-item live">
          <span class="live-dot" aria-hidden="true" />
          <span class="live-text">实时数据已就绪</span>
        </div>
      </div>
    </div>
  </AppCard>
</template>

<style scoped>
.hero-inner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px 18px;
  flex-wrap: wrap;
}

.brand-block {
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
}

.logo-mark {
  width: 44px;
  height: 44px;
  border-radius: 12px;
  display: grid;
  place-items: center;
  font-weight: 800;
  font-size: 15px;
  letter-spacing: 0.04em;
  color: var(--primary-foreground);
  background: linear-gradient(135deg, var(--primary), color-mix(in srgb, var(--primary) 60%, var(--up)));
  box-shadow: var(--ef-shadow-sm);
}

.brand-text {
  min-width: 0;
}

.name {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
  letter-spacing: 0.02em;
}

.tagline {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--muted-foreground);
}

.meta {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}

.meta-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.meta-label {
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--muted-foreground);
}

.meta-value {
  font-size: 12px;
  font-weight: 600;
}

.env {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--accent);
  width: fit-content;
}

.status :deep(.connection-status) {
  font-size: 12px;
}

.live {
  flex-direction: row;
  align-items: center;
  gap: 6px;
}

.live-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--up);
}

.live-text {
  font-size: 12px;
  color: var(--muted-foreground);
}
</style>
