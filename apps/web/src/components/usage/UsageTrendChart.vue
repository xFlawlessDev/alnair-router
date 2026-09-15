<script setup lang="ts">
import { VisAxis, VisStackedBar, VisXYContainer } from '@unovis/vue';
import { computed, ref } from 'vue';

import { ChartCrosshair } from '@/components/ui/chart';
import UsageTrendTooltip from '@/components/usage/UsageTrendTooltip.vue';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { formatCompact, formatCost, formatNumber } from '@/lib/format';
import type { UsageBucket, UsageBucketSize } from '@/types/api';

const props = defineProps<{
  points: UsageBucket[];
  bucket: UsageBucketSize;
  since: string | null;
  until: string | null;
}>();

type Metric = 'requests' | 'tokens' | 'cost';

const METRICS: { value: Metric; label: string }[] = [
  { value: 'requests', label: 'Requests' },
  { value: 'tokens', label: 'Tokens' },
  { value: 'cost', label: 'Cost' },
];

/** Models past this many are folded into one "Other" series. */
const MAX_SERIES = 6;
/** Cap the gap fill; very long windows keep only the buckets that have data. */
const MAX_FILLED_BUCKETS = 400;

const PALETTE = [
  'oklch(0.62 0.17 255)',
  'oklch(0.68 0.15 150)',
  'oklch(0.74 0.15 75)',
  'oklch(0.62 0.19 25)',
  'oklch(0.64 0.18 310)',
  'oklch(0.7 0.12 200)',
];
const OTHER_COLOR = 'oklch(0.7 0.02 250)';

interface SeriesDef {
  label: string;
  color: string;
  models: Set<string>;
}

interface TrendPoint {
  x: number;
  label: string;
  values: Record<string, number>;
  [display: string]: number | string | Record<string, number>;
}

const metric = ref<Metric>('tokens');

/** Width of one bucket in milliseconds, so bars stay uniform across gaps. */
const bucketSpanMs = computed(() => (props.bucket === 'hour' ? 3_600_000 : 86_400_000));

function floorToBucket(at: number): number {
  return Math.floor(at / bucketSpanMs.value) * bucketSpanMs.value;
}

function metricValue(point: UsageBucket): number {
  switch (metric.value) {
    case 'cost':
      return point.cost_usd;
    case 'tokens':
      return point.prompt_tokens + point.completion_tokens;
    default:
      return point.requests;
  }
}

function formatMetric(value: number): string {
  switch (metric.value) {
    case 'cost':
      return formatCost(value);
    case 'tokens':
      return formatCompact(value);
    default:
      return formatNumber(value);
  }
}

/** Ranked model series for the selected metric, with a folded "Other" tail. */
const seriesDefs = computed<SeriesDef[]>(() => {
  const totals = new Map<string, number>();
  for (const point of props.points) {
    totals.set(point.model, (totals.get(point.model) ?? 0) + metricValue(point));
  }

  const ranked = [...totals.entries()].sort((a, b) => b[1] - a[1]);
  const defs: SeriesDef[] = ranked.slice(0, MAX_SERIES).map(([model], index) => ({
    label: model,
    color: PALETTE[index % PALETTE.length],
    models: new Set([model]),
  }));

  const rest = ranked.slice(MAX_SERIES);
  if (rest.length) {
    defs.push({
      label: `Other (${rest.length})`,
      color: OTHER_COLOR,
      models: new Set(rest.map(([model]) => model)),
    });
  }

  return defs;
});

function buildPoint(at: number, rows: UsageBucket[], defs: SeriesDef[]): TrendPoint {
  const totals: Record<string, number> = {};
  for (const def of defs) totals[def.label] = 0;

  for (const row of rows) {
    const value = metricValue(row);
    for (const def of defs) {
      if (def.models.has(row.model)) {
        totals[def.label] += value;
        break;
      }
    }
  }

  const total = Object.values(totals).reduce((sum, value) => sum + value, 0);
  const point = { x: at, label: pointLabel(at), values: totals } as TrendPoint;
  for (const def of defs) point[def.label] = formatMetric(totals[def.label]);
  point.Total = formatMetric(total);
  return point;
}

/** Render the whole selected window, not just the buckets that saw traffic. */
const series = computed<TrendPoint[]>(() => {
  const points = props.points;
  if (!points.length) return [];

  const defs = seriesDefs.value;
  const byBucket = new Map<number, UsageBucket[]>();
  for (const point of points) {
    const at = floorToBucket(new Date(point.bucket).getTime());
    const rows = byBucket.get(at);
    if (rows) rows.push(point);
    else byBucket.set(at, [point]);
  }

  const buckets = [...byBucket.keys()].sort((a, b) => a - b);
  const start = props.since ? floorToBucket(new Date(props.since).getTime()) : buckets[0];
  const end = props.until
    ? floorToBucket(new Date(props.until).getTime())
    : props.since
      ? floorToBucket(Date.now())
      : buckets[buckets.length - 1];
  const count = Math.floor((end - start) / bucketSpanMs.value) + 1;

  if (count < 2 || count > MAX_FILLED_BUCKETS) {
    return buckets.map((at) => buildPoint(at, byBucket.get(at) ?? [], defs));
  }

  const filled: TrendPoint[] = [];
  for (let at = start; at <= end; at += bucketSpanMs.value) {
    filled.push(buildPoint(at, byBucket.get(at) ?? [], defs));
  }
  return filled;
});

function pointLabel(at: number): string {
  const date = new Date(at);
  return props.bucket === 'hour'
    ? new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date)
    : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(date);
}

const yAccessors = computed(() =>
  seriesDefs.value.map((def) => (point: TrendPoint) => point.values[def.label] ?? 0),
);

const colorFor = (_point: TrendPoint, index?: number): string =>
  seriesDefs.value[index ?? 0]?.color ?? OTHER_COLOR;

const tooltipItems = computed(() => [
  ...seriesDefs.value.map((def) => ({ name: def.label, color: def.color })),
  { name: 'Total', color: 'var(--muted-foreground)' },
]);

/** A lone bucket needs a padded domain, or the bar spans the whole plot. */
const xDomain = computed<[number, number] | undefined>(() => {
  if (series.value.length !== 1) return undefined;
  const center = series.value[0].x;
  return [center - bucketSpanMs.value / 2, center + bucketSpanMs.value / 2];
});

function formatTick(tick: number | Date): string {
  const date = new Date(tick);
  return props.bucket === 'hour'
    ? new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(date)
    : new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' }).format(date);
}

function formatValueTick(tick: number | Date): string {
  const value = Number(tick);
  return metric.value === 'cost' ? formatCost(value) : formatCompact(value);
}
</script>

<template>
  <Card>
    <CardHeader>
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="space-y-1.5">
          <CardTitle class="text-base">Usage over time</CardTitle>
          <CardDescription>
            {{ bucket === 'hour' ? 'Hourly' : 'Daily' }} buckets, stacked by model.
          </CardDescription>
        </div>
        <div class="flex gap-1 rounded-lg border p-1">
          <button
            v-for="option in METRICS"
            :key="option.value"
            type="button"
            class="rounded-md px-2.5 py-1 text-xs font-medium transition-colors"
            :class="
              metric === option.value
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:text-foreground'
            "
            :aria-pressed="metric === option.value"
            @click="metric = option.value"
          >
            {{ option.label }}
          </button>
        </div>
      </div>
    </CardHeader>
    <CardContent>
      <VisXYContainer
        v-if="series.length"
        :data="series"
        :height="240"
        :margin="{ top: 12, right: 12, bottom: 32, left: 52 }"
        :x-domain="xDomain"
        :y-domain="[0, undefined]"
      >
        <VisStackedBar
          :x="(point: TrendPoint) => point.x"
          :y="yAccessors"
          :color="colorFor"
          :data-step="bucketSpanMs"
          :bar-padding="0.2"
          :bar-max-width="48"
          :rounded-corners="2"
        />
        <VisAxis
          type="x"
          :tick-format="formatTick"
          :grid-line="false"
          :tick-line="false"
          :domain-line="false"
        />
        <VisAxis
          type="y"
          :tick-format="formatValueTick"
          :grid-line="false"
          :tick-line="false"
          :domain-line="false"
          :num-ticks="4"
        />
        <ChartCrosshair
          :colors="seriesDefs.map((def) => def.color)"
          index="label"
          :items="tooltipItems"
          :custom-tooltip="UsageTrendTooltip"
        />
      </VisXYContainer>
      <p v-else class="py-12 text-center text-sm text-muted-foreground">
        No usage in this window.
      </p>

      <div v-if="seriesDefs.length" class="mt-4 flex flex-wrap items-center gap-x-4 gap-y-1.5">
        <span
          v-for="def in seriesDefs"
          :key="def.label"
          class="flex max-w-[14rem] min-w-0 items-center gap-1.5 text-xs text-muted-foreground"
        >
          <span class="size-2.5 shrink-0 rounded-sm" :style="{ backgroundColor: def.color }" />
          <span class="truncate" :title="def.label">{{ def.label }}</span>
        </span>
      </div>
    </CardContent>
  </Card>
</template>
