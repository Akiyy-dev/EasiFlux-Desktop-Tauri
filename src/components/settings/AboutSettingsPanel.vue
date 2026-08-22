<script setup lang="ts">
import { useAppStore } from '../../stores/app'

const appStore = useAppStore()
</script>

<template>
  <section class="about-settings-panel" aria-labelledby="about-settings-title">
    <h1 id="about-settings-title">
      EasiFlux
    </h1>
    <p>版本 {{ appStore.version }}</p>
  </section>
</template>
