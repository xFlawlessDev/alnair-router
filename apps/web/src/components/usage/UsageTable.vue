<script setup lang="ts">
import { ArrowDown, ArrowUp, ChevronRight, ChevronsUpDown } from "@lucide/vue";
import { computed } from "vue";

import UsageBreakdownPopover, {
  type BreakdownRow,
} from "@/components/usage/UsageBreakdownPopover.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  formatCost,
  formatDateTime,
  formatLatency,
  formatNumber,
} from "@/lib/format";
import type { ApiKey, UsageRecord, UsageSortField } from "@/types/api";

const props = defineProps<{
  records: UsageRecord[];
  keys: ApiKey[];
  sort: { field: UsageSortField; descending: boolean };
  /** Average latency across the filtered window, for flagging outliers. */
  averageLatencyMs: number;
}>();

const emit = defineEmits<{
  sort: [UsageSortField];
  select: [UsageRecord];
}>();

/** Numeric columns right-align their cells and their sort toggles. */
const COLUMNS: {
  field: UsageSortField;
  label: string;
  numeric?: boolean;
}[] = [
  { field: "time", label: "Time" },
  { field: "model", label: "Model" },
  { field: "connection", label: "Resolved" },
  { field: "status", label: "Status" },
  { field: "tokens", label: "Tokens", numeric: true },
  { field: "cost", label: "Cost", numeric: true },
  { field: "latency", label: "Latency", numeric: true },
];

const keyNames = computed(
  () => new Map(props.keys.map((key) => [key.id, key.name])),
);

/**
 * Threshold past which a row's latency is called out. Attempts vary naturally,
 * so the flag only appears once a row is well above the window's average.
 */
const slowLatencyMs = computed(() =>
  props.averageLatencyMs > 0 ? props.averageLatencyMs * 2 : 0,
);

function isSlow(record: UsageRecord): boolean {
  return slowLatencyMs.value > 0 && record.latency_ms >= slowLatencyMs.value;
}

/** Rows written without a key (`/v1` with auth open) have none to show. */
function keyName(record: UsageRecord): string {
  if (!record.api_key_id) return "—";
  return keyNames.value.get(record.api_key_id) ?? "Deleted key";
}

/** Token rows: cached and reasoning are subsets, the hints say so. */
function tokenRows(record: UsageRecord): BreakdownRow[] {
  return [
    { label: "Prompt", value: record.prompt_tokens },
    {
      label: "Cached read",
      value: record.cached_tokens,
      hint: "Included in prompt tokens; billed at the cache-read rate.",
    },
    { label: "Completion", value: record.completion_tokens },
    {
      label: "Reasoning",
      value: record.reasoning_tokens,
      hint: "Included in completion tokens; billed at the reasoning rate.",
    },
  ].filter(
    (row) =>
      row.value > 0 || row.label === "Prompt" || row.label === "Completion",
  );
}

/** Cost rows: input, output and the reasoning premium that make up the total. */
function costRows(record: UsageRecord): BreakdownRow[] {
  const rows: BreakdownRow[] = [
    { label: "Input", value: record.cost_input_usd, format: "cost" },
    {
      label: "Output",
      value: record.cost_output_usd,
      format: "cost",
      hint: "Completion tokens at the output rate.",
    },
    {
      label: "Reasoning premium",
      value: record.cost_reasoning_usd,
      format: "cost",
      hint: "Extra rate charged for reasoning tokens.",
    },
  ];
  const known = rows.reduce((sum, row) => sum + row.value, 0);
  if (known <= 0 && record.cost_usd > 0) {
    return [
      {
        label: "Recorded total",
        value: record.cost_usd,
        format: "cost",
        hint: "This row predates the cost breakdown.",
      },
    ];
  }
  return rows;
}

/** Clicking the active column flips direction; a new column starts descending. */
function toggleSort(field: UsageSortField): void {
  emit("sort", field);
}
</script>

<template>
  <Table>
    <TableHeader>
      <TableRow>
        <TableHead v-for="column in COLUMNS" :key="column.field">
          <button
            type="button"
            class="inline-flex w-full items-center gap-1 transition-colors hover:text-foreground"
            :class="[
              sort.field === column.field ? 'text-foreground' : '',
              column.numeric ? 'justify-end' : 'justify-start',
            ]"
            :aria-label="`Sort by ${column.label}`"
            :aria-pressed="sort.field === column.field"
            @click="toggleSort(column.field)"
          >
            {{ column.label }}
            <ArrowDown
              v-if="sort.field === column.field && sort.descending"
              class="size-3.5"
            />
            <ArrowUp v-else-if="sort.field === column.field" class="size-3.5" />
            <ChevronsUpDown v-else class="size-3.5 opacity-40" />
          </button>
        </TableHead>
        <TableHead>API key</TableHead>
        <TableHead>Attempt</TableHead>
        <TableHead class="w-10" />
      </TableRow>
    </TableHeader>
    <TableBody>
      <TableRow
        v-for="record in records"
        :key="record.id"
        class="cursor-pointer"
        @click="emit('select', record)"
      >
        <TableCell class="text-xs whitespace-nowrap text-muted-foreground">
          {{ formatDateTime(record.created_at) }}
        </TableCell>
        <TableCell>
          <code class="text-xs">{{ record.requested_model }}</code>
        </TableCell>
        <TableCell>
          <div
            v-if="
              record.connection_name ||
              record.resolved_provider ||
              record.resolved_model
            "
            class="flex flex-col gap-1"
          >
            <code v-if="record.connection_name" class="text-xs">{{
              record.connection_name
            }}</code>
            <Badge
              v-if="record.resolved_provider"
              variant="outline"
              class="w-fit text-xs"
            >
              {{ record.resolved_provider }}
            </Badge>
            <code
              v-if="record.resolved_model"
              class="text-xs text-muted-foreground"
            >
              {{ record.resolved_model }}
            </code>
          </div>
          <span v-else class="text-muted-foreground">—</span>
        </TableCell>
        <TableCell>
          <Badge :variant="record.status === 'ok' ? 'default' : 'destructive'">
            {{ record.status }}
          </Badge>
        </TableCell>
        <!-- The breakdown popover opens in place, so it must not also open the
             row's detail sheet. -->
        <TableCell class="text-right text-xs text-muted-foreground" @click.stop>
          <UsageBreakdownPopover
            title="Token breakdown"
            :total="record.prompt_tokens + record.completion_tokens"
            :rows="tokenRows(record)"
          >
            {{ formatNumber(record.prompt_tokens) }} /
            {{ formatNumber(record.completion_tokens) }}
            <span v-if="record.cached_tokens">
              · {{ formatNumber(record.cached_tokens) }} cached</span
            >
            <span v-if="record.reasoning_tokens">
              · {{ formatNumber(record.reasoning_tokens) }} reasoning</span
            >
          </UsageBreakdownPopover>
        </TableCell>
        <TableCell class="text-right text-xs" @click.stop>
          <UsageBreakdownPopover
            title="Cost breakdown"
            total-format="cost"
            :total="record.cost_usd"
            :rows="costRows(record)"
          >
            {{ formatCost(record.cost_usd) }}
          </UsageBreakdownPopover>
        </TableCell>
        <TableCell
          class="text-right text-xs whitespace-nowrap tabular-nums"
          :class="isSlow(record) ? 'font-medium text-amber-500' : ''"
          :title="
            isSlow(record)
              ? `At least twice the ${formatLatency(averageLatencyMs)} average`
              : undefined
          "
        >
          {{ formatLatency(record.latency_ms) }}
        </TableCell>
        <TableCell class="text-xs text-muted-foreground">
          <span :title="record.api_key_id ?? undefined">{{
            keyName(record)
          }}</span>
        </TableCell>
        <TableCell>
          <Badge
            v-if="record.attempt > 1"
            variant="secondary"
            class="text-xs"
            title="Earlier attempts in this request failed before this one"
          >
            #{{ record.attempt }}
          </Badge>
          <span v-else class="text-xs text-muted-foreground">1</span>
        </TableCell>
        <TableCell>
          <Button
            variant="ghost"
            size="icon-sm"
            :aria-label="`View details for the ${record.requested_model} attempt`"
            @click.stop="emit('select', record)"
          >
            <ChevronRight />
          </Button>
        </TableCell>
      </TableRow>
    </TableBody>
  </Table>
</template>
