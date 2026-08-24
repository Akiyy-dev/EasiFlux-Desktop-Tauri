<script setup lang="ts">
import { onMounted } from 'vue'
import { useMessage } from 'naive-ui'
import { installMessageApi } from '../../services/errorService'
import { installNotificationToastApi } from '../../services/notificationToastService'

const message = useMessage()

onMounted(() => {
  installMessageApi(message)
  installNotificationToastApi(message)
})
</script>

<template>
  <div style="display: none" aria-hidden="true" />
</template>
