<script setup lang="ts">
import AppShell from '../components/layout/AppShell.vue'

const props = defineProps<{
  version: string
}>()

const emit = defineEmits<(e: 'openSettings') => void>()
</script>

<template>
  <AppShell :version="props.version" @open-settings="emit('openSettings')" />
</template>
