<script setup lang="ts">
import { Megaphone, Newspaper, Sparkles } from 'lucide-vue-next'
import type { FunctionalComponent } from 'vue'
import AppCard from '../ui/AppCard.vue'
import AppIcon from '../ui/AppIcon.vue'
import type { DashboardActivityItem, DashboardActivityType } from './types'

const activities: DashboardActivityItem[] = [
  {
    id: 'release-030',
    type: 'update',
    title: 'EasiFlux Desktop v0.3.0 已发布',
    summary: '完成 Tauri 迁移，交易终端、订单中心与 Design System 已落地。',
    timeLabel: '2026-06',
  },
  {
    id: 'welcome',
    type: 'announcement',
    title: '欢迎使用 EasiFlux',
    summary: '连接 API 后即可在交易页进行合约下单、查看持仓与资产。',
    timeLabel: '公告',
  },
  {
    id: 'news-placeholder',
    type: 'news',
    title: '新闻中心即将上线',
    summary: '后续将支持 RSS 与官方公告接口，集中展示市场资讯。',
    timeLabel: '预告',
  },
]

const iconByType: Record<DashboardActivityType, FunctionalComponent> = {
  update: Sparkles,
  announcement: Megaphone,
  news: Newspaper,
}
</script>

<template>
  <AppCard title="最近动态">
    <ul class="activity-list">
      <li v-for="item in activities" :key="item.id" class="activity-item">
        <span class="icon-wrap" :data-type="item.type">
          <AppIcon :icon="iconByType[item.type]" :size="16" />
        </span>
        <div class="content">
          <div class="head">
            <span class="title">{{ item.title }}</span>
            <span class="time">{{ item.timeLabel }}</span>
          </div>
          <p class="summary">{{ item.summary }}</p>
        </div>
      </li>
    </ul>
  </AppCard>
</template>

<style scoped>
.activity-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.activity-item {
  display: flex;
  gap: 10px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  background: color-mix(in srgb, var(--card) 92%, var(--accent) 8%);
}

.icon-wrap {
  width: 32px;
  height: 32px;
  border-radius: 10px;
  display: grid;
  place-items: center;
  flex-shrink: 0;
  background: var(--accent);
  color: var(--muted-foreground);
}

.icon-wrap[data-type='update'] {
  color: var(--primary);
}

.icon-wrap[data-type='announcement'] {
  color: var(--up);
}

.content {
  flex: 1;
  min-width: 0;
}

.head {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(72px, max-content);
  align-items: baseline;
  gap: 8px;
}

.title {
  font-size: 13px;
  font-weight: 600;
  min-width: 0;
}

.time {
  font-size: 11px;
  color: var(--muted-foreground);
  white-space: nowrap;
  justify-self: end;
  text-align: right;
}

.summary {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--muted-foreground);
  line-height: 1.45;
}
</style>
