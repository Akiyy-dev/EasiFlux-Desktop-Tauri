<script setup lang="ts">
import { NModal } from 'naive-ui'

const props = withDefaults(
  defineProps<{
    show: boolean
    title?: string
    width?: string | number
  }>(),
  {
    title: undefined,
    width: 480,
  },
)

const emit = defineEmits<{
  'update:show': [boolean]
}>()

function modalWidth(): string {
  return typeof props.width === 'number' ? `${props.width}px` : props.width
}
</script>

<template>
  <NModal
    class="ef-dialog"
    :show="show"
    preset="card"
    :title="title"
    :style="{ width: modalWidth() }"
    @update:show="emit('update:show', $event)"
  >
    <slot />
    <template v-if="$slots.footer" #footer>
      <slot name="footer" />
    </template>
  </NModal>
</template>
