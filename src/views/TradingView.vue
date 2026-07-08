<script setup lang="ts">
import AppShell from '../components/layout/AppShell.vue'

const emit = defineEmits<(e: 'openSettings') => void>()
</script>

<template>
  <AppShell @open-settings="emit('openSettings')" />
</template>
