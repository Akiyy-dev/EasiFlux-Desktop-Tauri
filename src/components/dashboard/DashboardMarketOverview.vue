<script setup lang="ts">
import { computed } from 'vue'
import AppCard from '../ui/AppCard.vue'
import MonoValue from '../ui/MonoValue.vue'
import { useDashboardTickers } from '../../composables/useDashboardTickers'
import { change24hPctValue, formatChange24hPct } from '../../utils/ticker'

const { tickers, loading } = useDashboardTickers()

function displaySymbol(symbol: string): string {
  return symbol.endsWith('USDT') ? symbol.slice(0, -4) : symbol
}

function changeClass(raw: string | undefined): string {
  const pct = change24hPctValue(raw)
  if (pct > 0) return 'text-up'
  if (pct < 0) return 'text-down'
  return ''
}

const rows = computed(() =>
  tickers.value.map((ticker) => ({
    symbol: displaySymbol(ticker.symbol),
    price: ticker.lastPrice,
    change: formatChange24hPct(ticker.change24hPct),
    changeClass: changeClass(ticker.change24hPct),
  })),
)
</script>

<template>
  <AppCard title="市场概览">
    <div class="market-table-wrap">
      <table class="market-table">
        <thead>
          <tr>
            <th>币种</th>
            <th>最新价格</th>
            <th>24h 涨跌</th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="loading && rows.length === 0">
            <td colspan="3" class="empty">加载行情中…</td>
          </tr>
          <tr v-for="row in rows" :key="row.symbol">
            <td class="symbol">{{ row.symbol }}</td>
            <td>
              <MonoValue size="sm">{{ row.price }}</MonoValue>
            </td>
            <td>
              <MonoValue size="sm" :class="row.changeClass">{{ row.change }}</MonoValue>
            </td>
          </tr>
          <tr v-if="!loading && rows.length === 0">
            <td colspan="3" class="empty">暂无行情数据</td>
          </tr>
        </tbody>
      </table>
    </div>
  </AppCard>
</template>

<style scoped>
.market-table-wrap {
  overflow: auto;
}

.market-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

th,
td {
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
  text-align: left;
}

th {
  color: var(--muted-foreground);
  font-weight: 600;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.symbol {
  font-weight: 600;
}

.empty {
  text-align: center;
  color: var(--muted-foreground);
  padding: 16px 8px;
}
</style>
