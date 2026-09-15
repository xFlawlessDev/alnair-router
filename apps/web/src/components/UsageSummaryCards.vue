<script setup lang="ts">
import { computed } from 'vue';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import UsageBreakdownPopover, {
  type BreakdownRow,
} from '@/components/usage/UsageBreakdownPopover.vue';
import { formatCompact, formatCost, formatLatency, formatNumber, successRate } from '@/lib/format';
import type { PublicModelUsage, UsageSummary } from '@/types/api';

const props = defineProps<{ summary: UsageSummary | null; models?: PublicModelUsage[] }>();

/** Model rows past this many are folded into one "Other" line. */
const MAX_MODEL_ROWS = 6;

interface SummaryBreakdown {
  title: string;
  total: number;
  totalFormat: 'tokens' | 'cost';
  rows: BreakdownRow[];
  byModel?: BreakdownRow[];
}

interface SummaryItem {
  label: string;
  value: string;
  hint: string;
  breakdown?: SummaryBreakdown;
}

/** Ranks models by the chosen metric and folds the tail into "Other". */
function modelRows(
  pick: (model: PublicModelUsage) => number,
  format: 'tokens' | 'cost',
  hint: (model: PublicModelUsage) => string,
): BreakdownRow[] {
  const models = props.models ?? [];
  if (!models.length) return [];

  const ranked = [...models].sort((a, b) => pick(b) - pick(a));
  const rows: BreakdownRow[] = ranked.slice(0, MAX_MODEL_ROWS).map((model) => ({
    label: model.model,
    value: pick(model),
    format,
    hint: hint(model),
  }));

  const rest = ranked.slice(MAX_MODEL_ROWS);
  if (rest.length) {
    rows.push({
      label: `Other (${rest.length})`,
      value: rest.reduce((sum, model) => sum + pick(model), 0),
      format,
      hint: 'Remaining models',
    });
  }

  return rows;
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
        byModel: modelRows(
          (model) => model.prompt_tokens + model.completion_tokens,
          'tokens',
          (model) =>
            `${formatNumber(model.prompt_tokens)} prompt · ${formatNumber(model.completion_tokens)} completion`,
        ),
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
        byModel: modelRows(
          (model) => model.cost_usd,
          'cost',
          (model) => `${formatNumber(model.requests)} requests`,
        ),
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
            :by-model="item.breakdown.byModel"
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
