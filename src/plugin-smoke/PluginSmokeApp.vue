<script setup lang="ts">
import { onMounted, ref } from 'vue'
import PluginMarketplacePage from '../components/plugins/PluginMarketplacePage.vue'
import type { PluginSection } from '../types/navigation'
import { runPluginSmokeSelfTest } from './selfTest'

const section = ref<PluginSection>('manage')
const automatic = new globalThis.URLSearchParams(globalThis.window.location.search).get('self-test') === '1'
onMounted(() => {
  if (automatic) void runPluginSmokeSelfTest()
})
</script>

<template>
  <main class="plugin-smoke-host">
    <aside role="note">
      Isolated fixture profile only. Source: select the exact manifest path printed in the host log.
      No accounts, credentials, scheduler, or provider connections are started.
      <p v-if="automatic">
        Automatic fixture test; native picker and whole app are not tested.
      </p>
      <p v-else>
        Manual lane: import, enable/disable, and remove only Plugin Smoke Fixture. Closing does not record an automatic pass.
      </p>
    </aside>
    <nav v-if="!automatic" aria-label="Plugin section">
      <button @click="section = 'installed'">
        Installed
      </button>
      <button @click="section = 'market'">
        Market
      </button>
      <button @click="section = 'manage'">
        Manage
      </button>
    </nav>
    <PluginMarketplacePage :section="section" />
  </main>
</template>

<style scoped>
.plugin-smoke-host { min-height: 100vh; padding: 24px; }
aside { margin-bottom: 20px; }
nav { display: flex; gap: 12px; margin-bottom: 16px; }
</style>
