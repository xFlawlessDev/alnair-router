<script setup lang="ts">
import { computed } from "vue";

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { formatCost, formatNumber } from "@/lib/format";

/** One line of a breakdown: token count or USD amount. */
export interface BreakdownRow {
  label: string;
  value: number;
  format?: "tokens" | "cost";
  hint?: string;
}

const props = withDefaults(
  defineProps<{
    title: string;
    total: number;
    totalFormat?: "tokens" | "cost";
    rows: BreakdownRow[];
    /** Optional second section comparing one metric per model. */
    byModel?: BreakdownRow[];
    /** Show each main row's share of the total. */
    showPercent?: boolean;
  }>(),
  { totalFormat: "tokens", byModel: () => [], showPercent: false },
);

interface Section {
  title: string;
  rows: BreakdownRow[];
  percent: boolean;
}

const sections = computed<Section[]>(() => {
  const sections: Section[] = [
    { title: "", rows: props.rows, percent: props.showPercent },
  ];
  if (props.byModel.length) {
    sections.push({
      title: "Share by model",
      rows: props.byModel,
      percent: true,
    });
  }
  return sections;
});

function formatted(row: BreakdownRow): string {
  return (row.format ?? "tokens") === "cost"
    ? formatCost(row.value)
    : formatNumber(row.value);
}

function total(): string {
  return props.totalFormat === "cost"
    ? formatCost(props.total)
    : formatNumber(props.total);
}

function share(value: number): number {
  if (props.total <= 0) return 0;
  return Math.min(100, (value / props.total) * 100);
}

function percentage(value: number): string {
  if (props.total <= 0) return "0%";
  const percent = (value / props.total) * 100;
  return `${percent >= 10 ? percent.toFixed(0) : percent.toFixed(1)}%`;
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
    <PopoverContent
      class="w-72 max-w-[calc(100vw-2rem)] overflow-hidden"
      align="start"
    >
      <p class="truncate text-xs font-medium" :title="title">{{ title }}</p>

      <template v-for="section in sections" :key="section.title">
        <p
          v-if="section.title"
          class="mt-3 border-t pt-2 text-[10px] font-medium tracking-wide text-muted-foreground uppercase"
        >
          {{ section.title }}
        </p>
        <dl class="mt-2 grid w-full grid-cols-1 gap-2.5">
          <div
            v-for="row in section.rows"
            :key="row.label"
            class="grid w-full min-w-0 grid-cols-1 gap-1"
          >
            <div
              class="flex w-full min-w-0 items-baseline justify-between gap-2 text-xs"
            >
              <dt
                class="min-w-0 flex-1 truncate text-muted-foreground"
                :title="row.label"
              >
                {{ row.label }}
              </dt>
              <dd
                class="flex shrink-0 items-baseline gap-2 font-medium tabular-nums"
              >
                <span class="whitespace-nowrap">{{ formatted(row) }}</span>
                <span
                  v-if="section.percent"
                  class="w-9 text-right text-[10px] font-normal text-muted-foreground"
                >
                  {{ percentage(row.value) }}
                </span>
              </dd>
            </div>
            <div class="h-1 w-full overflow-hidden rounded-full bg-muted">
              <div
                class="h-full rounded-full bg-primary"
                :style="{ width: `${share(row.value)}%` }"
              />
            </div>
            <p
              v-if="row.hint"
              class="w-full truncate text-[10px] text-muted-foreground"
              :title="row.hint"
            >
              {{ row.hint }}
            </p>
          </div>
        </dl>
      </template>

      <div
        class="mt-3 flex items-baseline justify-between border-t pt-2 text-xs"
      >
        <span class="text-muted-foreground">Total</span>
        <span class="font-medium">{{ total() }}</span>
      </div>
    </PopoverContent>
  </Popover>
</template>
