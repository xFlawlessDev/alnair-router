<script setup lang="ts">
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { formatCost, formatNumber } from '@/lib/format';

/** One line of a breakdown: token count or USD amount. */
export interface BreakdownRow {
  label: string;
  value: number;
  format?: 'tokens' | 'cost';
  hint?: string;
}

const props = withDefaults(
  defineProps<{
    title: string;
    total: number;
    totalFormat?: 'tokens' | 'cost';
    rows: BreakdownRow[];
  }>(),
  { totalFormat: 'tokens' },
);

function formatted(row: BreakdownRow): string {
  return (row.format ?? 'tokens') === 'cost' ? formatCost(row.value) : formatNumber(row.value);
}

function total(): string {
  return props.totalFormat === 'cost' ? formatCost(props.total) : formatNumber(props.total);
}

function share(value: number): number {
  if (props.total <= 0) return 0;
  return Math.min(100, (value / props.total) * 100);
}
</script>

<template>
  <Popover>
    <PopoverTrigger as-child>
      <button
        type="button"
        class="cursor-help text-left underline decoration-muted-foreground/40 decoration-dotted underline-offset-4"
      >
        <slot />
      </button>
    </PopoverTrigger>
    <PopoverContent class="w-72" align="start">
      <p class="text-xs font-medium">{{ title }}</p>
      <dl class="mt-2 grid gap-2.5">
        <div v-for="row in rows" :key="row.label" class="grid gap-1">
          <div class="flex items-baseline justify-between gap-2 text-xs">
            <dt class="text-muted-foreground">{{ row.label }}</dt>
            <dd class="font-medium">{{ formatted(row) }}</dd>
          </div>
          <div class="h-1 w-full overflow-hidden rounded-full bg-muted">
            <div class="h-full rounded-full bg-primary" :style="{ width: `${share(row.value)}%` }" />
          </div>
          <p v-if="row.hint" class="text-[10px] text-muted-foreground">{{ row.hint }}</p>
        </div>
      </dl>
      <div class="mt-3 flex items-baseline justify-between border-t pt-2 text-xs">
        <span class="text-muted-foreground">Total</span>
        <span class="font-medium">{{ total() }}</span>
      </div>
    </PopoverContent>
  </Popover>
</template>
