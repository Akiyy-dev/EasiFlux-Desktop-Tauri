<script setup lang="ts">
import AppButton from '../ui/AppButton.vue'

defineProps<{ count: number }>()
const emit = defineEmits<{ showLatest: [] }>()
</script>

<template>
  <div v-if="count > 0" class="new-messages-announcement" aria-live="polite">
    <AppButton
      class="new-messages-banner"
      variant="primary"
      size="sm"
      @click="emit('showLatest')"
    >
      有 {{ count }} 条新消息
    </AppButton>
  </div>
</template>

<style scoped>
.new-messages-announcement {
  display: flex;
  justify-content: center;
  pointer-events: none;
}

.new-messages-banner {
  pointer-events: auto;
  border-radius: var(--ef-radius-xl);
}
</style>
