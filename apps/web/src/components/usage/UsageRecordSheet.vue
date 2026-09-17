<script setup lang="ts">
import { computed } from "vue";

import UsageBreakdownPopover, {
  type BreakdownRow,
} from "@/components/usage/UsageBreakdownPopover.vue";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  formatCost,
  formatDateTime,
  formatLatency,
  formatNumber,
} from "@/lib/format";
import type { ApiKey, UsageRecord } from "@/types/api";

const props = defineProps<{
  record: UsageRecord | null;
  keys: ApiKey[];
}>();
const open = defineModel<boolean>("open", { required: true });

/** Every field of the row, so nothing needs a second request to inspect. */
const fields = computed(() => {
  const record = props.record;
  if (!record) return [];
  return [
    { label: "Time", value: formatDateTime(record.created_at) },
    { label: "Requested model", value: record.requested_model, code: true },
    {
      label: "Resolved model",
      value: record.resolved_model ?? "—",
      code: true,
    },
    { label: "Provider", value: record.resolved_provider ?? "—" },
    { label: "Connection", value: record.connection_name ?? "—", code: true },
    { label: "API key", value: keyLabel(record) },
    { label: "Attempt", value: attemptLabel(record) },
    { label: "Latency", value: formatLatency(record.latency_ms) },
    { label: "Recorded cost", value: formatCost(record.cost_usd) },
  ];
});

function keyLabel(record: UsageRecord): string {
  if (!record.api_key_id) return "Unauthenticated request";
  const key = props.keys.find((item) => item.id === record.api_key_id);
  return key ? key.name : "Deleted key";
}

function attemptLabel(record: UsageRecord): string {
  return record.attempt > 1
    ? `${record.attempt} (earlier attempts failed)`
    : "1";
}

const tokenRows = computed<BreakdownRow[]>(() => {
  const record = props.record;
  if (!record) return [];
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
});

const costRows = computed<BreakdownRow[]>(() => {
  const record = props.record;
  if (!record) return [];
  return [
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
});

const tokenTotal = computed(() =>
  props.record
    ? props.record.prompt_tokens + props.record.completion_tokens
    : 0,
);
</script>

<template>
  <Sheet v-model:open="open">
    <SheetContent class="w-full overflow-y-auto sm:max-w-md">
      <SheetHeader>
        <SheetTitle>Attempt detail</SheetTitle>
        <SheetDescription>
          One upstream attempt, exactly as it was recorded.
        </SheetDescription>
      </SheetHeader>

      <div v-if="record" class="mt-6 grid gap-6">
        <div class="grid gap-2">
          <div
            v-for="field in fields"
            :key="field.label"
            class="flex items-baseline justify-between gap-3 text-sm"
          >
            <span class="shrink-0 text-muted-foreground">{{
              field.label
            }}</span>
            <code v-if="field.code" class="min-w-0 truncate text-xs">{{
              field.value
            }}</code>
            <span v-else class="min-w-0 truncate text-right">{{
              field.value
            }}</span>
          </div>
          <div class="flex items-baseline justify-between gap-3 text-sm">
            <span class="text-muted-foreground">Status</span>
            <Badge
              :variant="record.status === 'ok' ? 'default' : 'destructive'"
            >
              {{ record.status }}
            </Badge>
          </div>
        </div>

        <Separator />

        <div class="grid gap-2">
          <p class="text-sm font-medium">Tokens</p>
          <UsageBreakdownPopover
            title="Token breakdown"
            :total="tokenTotal"
            :rows="tokenRows"
            show-percent
          >
            <span class="text-sm">
              {{ formatNumber(record.prompt_tokens) }} prompt ·
              {{ formatNumber(record.completion_tokens) }} completion
            </span>
          </UsageBreakdownPopover>
          <p class="text-xs text-muted-foreground">
            Click the figures above for the full split, including cached and
            reasoning tokens.
          </p>
        </div>

        <div class="grid gap-2">
          <p class="text-sm font-medium">Cost</p>
          <UsageBreakdownPopover
            title="Cost breakdown"
            total-format="cost"
            :total="record.cost_usd"
            :rows="costRows"
            show-percent
          >
            <span class="text-sm">{{ formatCost(record.cost_usd) }}</span>
          </UsageBreakdownPopover>
        </div>
      </div>
    </SheetContent>
  </Sheet>
</template>
