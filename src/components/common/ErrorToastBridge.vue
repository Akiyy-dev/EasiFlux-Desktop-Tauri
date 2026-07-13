<script setup lang="ts">
import { onMounted } from 'vue'
import { useMessage } from 'naive-ui'
import { installMessageApi } from '../../services/errorService'

const message = useMessage()

onMounted(() => {
  installMessageApi(message)
})
</script>

<template>
  <div style="display: none" aria-hidden="true" />
</template>
