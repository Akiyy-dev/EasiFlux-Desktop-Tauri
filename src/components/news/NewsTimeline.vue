<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { NewsMessageDto, NewsTextSegment } from '../../types/news'
import { openNewsLink, segmentNewsText } from '../../utils/newsLinks'
import AppButton from '../ui/AppButton.vue'
import './newsTimeline.css'

interface TimelineEntry {
  item: NewsMessageDto
  segments: NewsTextSegment[]
  dateKey: string
  dateLabel: string
  timeLabel: string
  showDate: boolean
}

const props = defineProps<{
  items: NewsMessageDto[]
  hasMore: boolean
  loadingMore: boolean
  loadMoreError: string | null
}>()

const emit = defineEmits<{ loadMore: [] }>()
const arrivingIds = ref<Set<string>>(new Set())

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

function localParts(createdAt: string): Omit<TimelineEntry, 'item' | 'segments' | 'showDate'> {
  const date = new Date(createdAt)
  const year = date.getFullYear()
  const month = date.getMonth() + 1
  const day = date.getDate()
  return {
    dateKey: `${year}-${pad(month)}-${pad(day)}`,
    dateLabel: `${year}年${month}月${day}日`,
    timeLabel: `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`,
  }
}

watch(() => props.items, (next, previous) => {
  if (previous.length === 0) {
    arrivingIds.value = new Set()
    return
  }
  const previousIds = new Set(previous.map((item) => item.deliveryId))
  const previousTop = previous.reduce((top, item) => {
    const id = BigInt(item.deliveryId)
    return id > top ? id : top
  }, BigInt(previous[0].deliveryId))
  arrivingIds.value = new Set(next
    .filter((item) => !previousIds.has(item.deliveryId) && BigInt(item.deliveryId) > previousTop)
    .map((item) => item.deliveryId))
})

const entries = computed<TimelineEntry[]>(() => {
  const sorted = [...props.items].sort((left, right) => {
    const leftId = BigInt(left.deliveryId)
    const rightId = BigInt(right.deliveryId)
    return leftId === rightId ? 0 : leftId > rightId ? -1 : 1
  })
  let previousDate = ''
  return sorted.map((item) => {
    const date = localParts(item.createdAt)
    const showDate = date.dateKey !== previousDate
    previousDate = date.dateKey
    const text = item.text.trim() ? item.text : '该消息暂无可展示的文本内容'
    return { item, ...date, showDate, segments: segmentNewsText(text) }
  })
})

function openLink(href?: string): void {
  if (!href) return
  void openNewsLink(href).catch(() => undefined)
}

function clearArrival(deliveryId: string): void {
  if (!arrivingIds.value.has(deliveryId)) return
  const remaining = new Set(arrivingIds.value)
  remaining.delete(deliveryId)
  arrivingIds.value = remaining
}
</script>

<template>
  <section class="news-timeline" aria-label="新闻时间线">
    <ol class="news-list">
      <li v-for="(entry, index) in entries" :key="entry.item.deliveryId" class="news-entry">
        <div v-if="entry.showDate" class="news-date-separator">
          <time class="news-date-label" :datetime="entry.dateKey">{{ entry.dateLabel }}</time>
        </div>
        <article
          class="news-item"
          :class="{
            'is-latest': index === 0,
            'is-arriving': arrivingIds.has(entry.item.deliveryId),
          }"
          @animationend.self="clearArrival(entry.item.deliveryId)"
        >
          <div class="news-time-cell">
            <time class="news-time" :datetime="entry.item.createdAt">{{ entry.timeLabel }}</time>
          </div>
          <p class="news-message-text">
            <template v-for="(segment, segmentIndex) in entry.segments" :key="segmentIndex">
              <a
                v-if="segment.kind === 'link'"
                class="news-link ef-focus-ring"
                :href="segment.href"
                @click.prevent="openLink(segment.href)"
              >{{ segment.value }}</a>
              <span v-else>{{ segment.value }}</span>
            </template>
          </p>
        </article>
      </li>
    </ol>

    <footer class="news-pagination">
      <p v-if="loadMoreError" class="news-pagination-error" role="alert">
        加载更早消息失败，请重试
      </p>
      <AppButton
        v-if="hasMore"
        variant="ghost"
        size="sm"
        :loading="loadingMore"
        @click="emit('loadMore')"
      >
        {{ loadingMore ? '正在加载更早消息' : '加载更早消息' }}
      </AppButton>
      <p v-else class="news-pagination-end" role="status">
        已显示全部新闻
      </p>
    </footer>
  </section>
</template>
