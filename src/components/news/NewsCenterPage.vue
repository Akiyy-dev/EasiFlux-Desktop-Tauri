<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useNewsPageController } from '../../composables/useNewsPageController'
import NewMessagesBanner from './NewMessagesBanner.vue'
import NewsStatusBar from './NewsStatusBar.vue'
import NewsTimeline from './NewsTimeline.vue'

const props = defineProps<{ active: boolean }>()
const {
  messages, status, hasMore, pendingNewCount, initialLoading, loadingMore,
  initialError, loadMoreError, scrollContainer, handleScroll, showLatest,
  loadMore, recheckCredentials, retrySync,
} = useNewsPageController(toRef(props, 'active'))

const initialSyncComplete = computed(() => status.value?.initialSyncComplete === true)
const hasCachedMessages = computed(() => initialSyncComplete.value && messages.value.length > 0)
const showLoading = computed(() => initialLoading.value && messages.value.length === 0
  && (!status.value || status.value.initialSyncComplete))
const showEmpty = computed(() => initialSyncComplete.value && !initialLoading.value
  && !initialError.value && messages.value.length === 0)
</script>

<template>
  <section class="news-center-page" aria-label="新闻中心">
    <NewsStatusBar
      v-if="status"
      :status="status"
      :has-cached-messages="hasCachedMessages"
      @recheck-credentials="recheckCredentials"
      @retry-sync="retrySync"
    />

    <div
      ref="scrollContainer"
      class="news-scroll-container"
      data-testid="news-scroll-container"
      @scroll="handleScroll"
    >
      <NewMessagesBanner
        class="news-banner-layer"
        :count="pendingNewCount"
        @show-latest="showLatest"
      />
      <div class="news-reading-column" style="max-width: 880px">
        <div v-if="showLoading" class="news-page-state news-loading-state" role="status">
          正在加载新闻
        </div>
        <div
          v-else-if="status && !initialSyncComplete"
          class="news-page-state news-initial-state"
          role="status"
        >
          正在准备最新新闻
        </div>
        <div v-if="initialError" class="news-page-state news-error-state" role="alert">
          新闻加载失败，请稍后重试
        </div>
        <div v-if="showEmpty" class="news-page-state news-empty-state">
          暂无新闻
        </div>
        <NewsTimeline
          v-if="hasCachedMessages"
          :items="messages"
          :has-more="hasMore"
          :loading-more="loadingMore"
          :load-more-error="loadMoreError"
          @load-more="loadMore"
        />
      </div>
    </div>
  </section>
</template>

<style scoped src="./newsCenterPage.css"></style>
