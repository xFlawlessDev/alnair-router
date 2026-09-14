<script setup lang="ts">
import { computed } from 'vue';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { formatCompact, formatCost, formatLatency, formatNumber, successRate } from '@/lib/format';
import type { UsageSummary } from '@/types/api';

const props = defineProps<{ summary: UsageSummary | null }>();

const items = computed(() => {
  const summary = props.summary;
  if (!summary) {
    return [
      { label: 'Requests', value: '—', hint: 'No data yet' },
      { label: 'Tokens', value: '—', hint: 'Prompt + completion' },
      { label: 'Cost', value: '—', hint: 'Recorded spend' },
      { label: 'Avg latency', value: '—', hint: 'Across attempts' },
    ];
  }
  return [
    {
      label: 'Requests',
      value: formatNumber(summary.requests),
      hint: `${successRate(summary.ok_requests, summary.requests)} success · ${formatNumber(summary.error_requests)} failed`,
    },
    {
      label: 'Tokens',
      value: formatCompact(summary.prompt_tokens + summary.completion_tokens),
      hint: `${formatCompact(summary.prompt_tokens)} prompt · ${formatCompact(summary.completion_tokens)} completion · ${formatCompact(summary.cached_tokens)} cached`,
    },
    { label: 'Cost', value: formatCost(summary.cost_usd), hint: 'Recorded upstream spend' },
    { label: 'Avg latency', value: formatLatency(summary.avg_latency_ms), hint: 'Across all attempts' },
  ];
});
</script>

<template>
  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
    <Card v-for="item in items" :key="item.label">
      <CardHeader class="pb-2">
        <CardTitle class="text-sm font-medium text-muted-foreground">{{ item.label }}</CardTitle>
      </CardHeader>
      <CardContent>
        <p class="text-2xl font-semibold tracking-tight">{{ item.value }}</p>
        <p class="mt-1 truncate text-xs text-muted-foreground" :title="item.hint">{{ item.hint }}</p>
      </CardContent>
    </Card>
  </div>
</template>
