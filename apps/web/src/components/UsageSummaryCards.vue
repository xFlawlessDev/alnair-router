<script setup lang="ts">
import { computed } from 'vue';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import UsageBreakdownPopover, {
  type BreakdownRow,
} from '@/components/usage/UsageBreakdownPopover.vue';
import { formatCompact, formatCost, formatLatency, formatNumber, successRate } from '@/lib/format';
import type { UsageSummary } from '@/types/api';

const props = defineProps<{ summary: UsageSummary | null }>();

interface SummaryBreakdown {
  title: string;
  total: number;
  totalFormat: 'tokens' | 'cost';
  rows: BreakdownRow[];
}

interface SummaryItem {
  label: string;
  value: string;
  hint: string;
  breakdown?: SummaryBreakdown;
}

const items = computed<SummaryItem[]>(() => {
  const summary = props.summary;
  if (!summary) {
    return [
      { label: 'Requests', value: '-', hint: 'No data yet' },
      { label: 'Tokens', value: '-', hint: 'Prompt + completion' },
      { label: 'Cost', value: '-', hint: 'Recorded spend' },
      { label: 'Avg latency', value: '-', hint: 'Across attempts' },
    ];
  }

  const tokenRows: BreakdownRow[] = [
    { label: 'Prompt', value: summary.prompt_tokens },
    {
      label: 'Cached read',
      value: summary.cached_tokens,
      hint: 'Included in prompt tokens; billed at the cache-read rate.',
    },
    { label: 'Completion', value: summary.completion_tokens },
    {
      label: 'Reasoning',
      value: summary.reasoning_tokens,
      hint: 'Included in completion tokens; billed at the reasoning rate.',
    },
  ].filter((row) => row.value > 0 || row.label === 'Prompt' || row.label === 'Completion');

  const costRows: BreakdownRow[] = [
    { label: 'Input', value: summary.cost_input_usd, format: 'cost' },
    {
      label: 'Output',
      value: summary.cost_output_usd,
      format: 'cost',
      hint: 'Completion tokens at the output rate.',
    },
    {
      label: 'Reasoning premium',
      value: summary.cost_reasoning_usd,
      format: 'cost',
      hint: 'Extra rate charged for reasoning tokens.',
    },
  ];

  return [
    {
      label: 'Requests',
      value: formatNumber(summary.requests),
      hint: `${successRate(summary.ok_requests, summary.requests)} success · ${formatNumber(summary.error_requests)} failed`,
    },
    {
      label: 'Tokens',
      value: formatCompact(summary.prompt_tokens + summary.completion_tokens),
      hint: `${formatCompact(summary.prompt_tokens)} prompt · ${formatCompact(summary.completion_tokens)} completion · ${formatCompact(summary.cached_tokens)} cached · ${formatCompact(summary.reasoning_tokens)} reasoning`,
      breakdown: {
        title: 'Token breakdown',
        total: summary.prompt_tokens + summary.completion_tokens,
        totalFormat: 'tokens',
        rows: tokenRows,
      },
    },
    {
      label: 'Cost',
      value: formatCost(summary.cost_usd),
      hint: 'Recorded upstream spend',
      breakdown: {
        title: 'Cost breakdown',
        total: summary.cost_usd,
        totalFormat: 'cost',
        rows: costRows,
      },
    },
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
        <p class="text-2xl font-semibold tracking-tight">
          <UsageBreakdownPopover
            v-if="item.breakdown"
            :title="item.breakdown.title"
            :total="item.breakdown.total"
            :total-format="item.breakdown.totalFormat"
            :rows="item.breakdown.rows"
          >
            {{ item.value }}
          </UsageBreakdownPopover>
          <template v-else>{{ item.value }}</template>
        </p>
        <p class="mt-1 truncate text-xs text-muted-foreground" :title="item.hint">{{ item.hint }}</p>
      </CardContent>
    </Card>
  </div>
</template>
