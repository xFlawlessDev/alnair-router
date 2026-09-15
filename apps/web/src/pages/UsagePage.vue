<script setup lang="ts">
import { ChevronLeft, ChevronRight, Pause, Play, RefreshCw, ScrollText } from '@lucide/vue';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { RouterLink } from 'vue-router';

import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import UsageSummaryCards from '@/components/UsageSummaryCards.vue';
import ProviderTopology from '@/components/usage/ProviderTopology.vue';
import UsageBreakdownPopover, {
  type BreakdownRow,
} from '@/components/usage/UsageBreakdownPopover.vue';
import UsageFilterBar from '@/components/usage/UsageFilterBar.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { ApiError, api } from '@/lib/api';
import {
  formatCost,
  formatDateTime,
  formatLatency,
  formatNumber,
} from '@/lib/format';
import { USAGE_RANGES, rangeToSince } from '@/lib/ranges';
import type {
  ActivitySnapshot,
  ApiKey,
  UsageFacets,
  UsageFilter,
  UsageRecord,
  UsageSummary,
} from '@/types/api';

/** Sentinel because Select values cannot be empty strings. */
const ALL = '__all__';

const limits = [50, 100, 200, 500];

const records = ref<UsageRecord[]>([]);
const summary = ref<UsageSummary | null>(null);
const keys = ref<ApiKey[]>([]);
const facets = ref<UsageFacets | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const range = ref('all');
const limit = ref(100);
const offset = ref(0);
const apiKeyId = ref(ALL);
const model = ref('');
const provider = ref(ALL);
const connection = ref(ALL);
let modelTimer: number | undefined;

// Live activity polling
const live = ref(true);
const activity = ref<ActivitySnapshot | null>(null);
const updatedAt = ref<Date | null>(null);
let activityTimer: number | undefined;
let tableTimer: number | undefined;

const nodes = computed(() => activity.value?.connections ?? []);
const activeNodes = computed(() => nodes.value.filter((node) => node.in_flight > 0));

/** Token rows: cached and reasoning are subsets, the hints say so. */
function tokenRows(record: UsageRecord): BreakdownRow[] {
  return [
    { label: 'Prompt', value: record.prompt_tokens },
    {
      label: 'Cached read',
      value: record.cached_tokens,
      hint: 'Included in prompt tokens; billed at the cache-read rate.',
    },
    { label: 'Completion', value: record.completion_tokens },
    {
      label: 'Reasoning',
      value: record.reasoning_tokens,
      hint: 'Included in completion tokens; billed at the reasoning rate.',
    },
  ].filter((row) => row.value > 0 || row.label === 'Prompt' || row.label === 'Completion');
}

/** Cost rows: input, output and the reasoning premium that make up the total. */
function costRows(record: UsageRecord): BreakdownRow[] {
  const rows: BreakdownRow[] = [
    { label: 'Input', value: record.cost_input_usd, format: 'cost' },
    {
      label: 'Output',
      value: record.cost_output_usd,
      format: 'cost',
      hint: 'Completion tokens at the output rate.',
    },
    {
      label: 'Reasoning premium',
      value: record.cost_reasoning_usd,
      format: 'cost',
      hint: 'Extra rate charged for reasoning tokens.',
    },
  ];
  const known = rows.reduce((sum, row) => sum + row.value, 0);
  if (known <= 0 && record.cost_usd > 0) {
    return [
      {
        label: 'Recorded total',
        value: record.cost_usd,
        format: 'cost',
        hint: 'This row predates the cost breakdown.',
      },
    ];
  }
  return rows;
}
const updatedLabel = computed(() =>
  updatedAt.value
    ? updatedAt.value.toLocaleTimeString(undefined, { hour12: false })
    : '—',
);

async function refreshActivity(): Promise<void> {
  try {
    activity.value = await api.activity(30);
  } catch {
    // Keep the last snapshot; the summary cards already surface API errors.
  }
}

function stopPolling(): void {
  if (activityTimer !== undefined) {
    window.clearInterval(activityTimer);
    activityTimer = undefined;
  }
  if (tableTimer !== undefined) {
    window.clearInterval(tableTimer);
    tableTimer = undefined;
  }
}

function startPolling(): void {
  stopPolling();
  void refreshActivity();
  void load({ silent: true });
  activityTimer = window.setInterval(refreshActivity, 2000);
  tableTimer = window.setInterval(() => void load({ silent: true }), 5000);
}

watch(live, (enabled) => (enabled ? startPolling() : stopPolling()));

/** Current filter set, shared by the table and the summary cards. */
function currentFilter(): UsageFilter {
  return {
    since: rangeToSince(range.value),
    api_key_id: apiKeyId.value === ALL ? null : apiKeyId.value,
    model: model.value.trim() || null,
    provider: provider.value === ALL ? null : provider.value,
    connection: connection.value === ALL ? null : connection.value,
  };
}

/** `silent` refresh keeps the current table on screen without a loading flash. */
async function load(options: { silent?: boolean } = {}): Promise<void> {
  const silent = options.silent === true;
  if (!silent) loading.value = true;
  error.value = null;
  try {
    const [recordsResponse, summaryResponse] = await Promise.all([
      api.listUsage(limit.value, offset.value, currentFilter()),
      api.usageSummary(currentFilter()),
    ]);
    records.value = recordsResponse;
    summary.value = summaryResponse;
    updatedAt.value = new Date();

    if (!silent) facets.value = await api.usageFacets();
  } catch (caught) {
    if (!silent) {
      error.value = caught instanceof ApiError ? caught.message : 'Failed to load usage';
    }
  } finally {
    // Always clear the initial spinner: the first load is silent (polling).
    loading.value = false;
  }
}

/** Fetches the filter pickers once, then starts the live refresh loop. */
async function init(): Promise<void> {
  try {
    const [keyList, facetList] = await Promise.all([api.listKeys(), api.usageFacets()]);
    keys.value = keyList;
    facets.value = facetList;
  } catch {
    // The usage load below surfaces connectivity problems.
  }

  if (live.value) startPolling();
  else await load();
}

function applyFilters(): void {
  offset.value = 0;
  void load();
}

function onApiKeyFilter(value: string): void {
  apiKeyId.value = value;
  applyFilters();
}

function onProviderFilter(value: string): void {
  provider.value = value;
  applyFilters();
}

function onConnectionFilter(value: string): void {
  connection.value = value;
  applyFilters();
}

/** Typing in the model filter is debounced; picking a suggestion is instant. */
function onModelFilter(value: string): void {
  model.value = value;
  if (modelTimer !== undefined) window.clearTimeout(modelTimer);
  modelTimer = window.setTimeout(() => applyFilters(), 300);
}

function clearFilters(): void {
  apiKeyId.value = ALL;
  model.value = '';
  provider.value = ALL;
  connection.value = ALL;
  applyFilters();
}

function previousPage(): void {
  offset.value = Math.max(0, offset.value - limit.value);
  void load();
}

function nextPage(): void {
  offset.value += limit.value;
  void load();
}

onMounted(init);
onUnmounted(() => {
  if (modelTimer !== undefined) window.clearTimeout(modelTimer);
  stopPolling();
});
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="Usage"
      description="One row per upstream attempt, failures included."
    >
      <template #actions>
        <span class="flex items-center gap-2 text-xs text-muted-foreground">
          <span class="relative flex size-2">
            <span
              v-if="live"
              class="absolute inline-flex size-full animate-ping rounded-full bg-emerald-500 opacity-75"
            />
            <span
              class="relative inline-flex size-2 rounded-full"
              :class="live ? 'bg-emerald-500' : 'bg-muted-foreground/40'"
            />
          </span>
          {{ live ? `Live · ${updatedLabel}` : 'Paused' }}
        </span>
        <Button variant="outline" size="sm" @click="live = !live">
          <Play v-if="!live" />
          <Pause v-else />
          {{ live ? 'Pause' : 'Resume' }}
        </Button>
        <Button variant="outline" :disabled="loading" @click="load()">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
      </template>
    </PageHeader>

    <Card>
      <CardHeader class="flex-row flex-wrap items-center justify-between gap-3 pb-3">
        <div>
          <CardTitle class="text-base">Live activity</CardTitle>
          <CardDescription>
            Providers around the router; the animated edge marks the route currently in use.
          </CardDescription>
        </div>
        <Badge v-if="activeNodes.length" variant="secondary">
          {{ activeNodes.length }} in flight
        </Badge>
      </CardHeader>
      <CardContent class="grid gap-3">
        <div
          v-if="!activity"
          class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground"
        >
          Waiting for activity…
        </div>

        <div
          v-else-if="!nodes.length"
          class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground"
        >
          No enabled connections yet.
        </div>

        <ProviderTopology v-else :connections="nodes" />

        <p class="text-xs text-muted-foreground">
          Edges animate only on providers handling a live request.
          <RouterLink to="/logs" class="underline underline-offset-4">Open console</RouterLink>
        </p>
      </CardContent>
    </Card>

    <UsageSummaryCards :summary="summary" />

    <UsageFilterBar
      :keys="keys"
      :facets="facets"
      :api-key-id="apiKeyId"
      :model="model"
      :provider="provider"
      :connection="connection"
      @update:api-key-id="onApiKeyFilter"
      @update:model="onModelFilter"
      @update:provider="onProviderFilter"
      @update:connection="onConnectionFilter"
      @clear="clearFilters"
    />

    <div class="flex flex-wrap items-center gap-3">
      <Select v-model="range" @update:model-value="applyFilters">
        <SelectTrigger class="w-44">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem v-for="item in USAGE_RANGES" :key="item.value" :value="item.value">
            {{ item.label }}
          </SelectItem>
        </SelectContent>
      </Select>
      <Select v-model.number="limit" @update:model-value="applyFilters">
        <SelectTrigger class="w-32">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem v-for="value in limits" :key="value" :value="value">
            {{ value }} rows
          </SelectItem>
        </SelectContent>
      </Select>
      <span class="text-xs text-muted-foreground">
        Showing rows {{ records.length ? offset + 1 : 0 }}–{{ offset + records.length }}
      </span>
      <div class="ml-auto flex items-center gap-1">
        <Button variant="outline" size="sm" :disabled="loading || offset === 0" @click="previousPage">
          <ChevronLeft /> Previous
        </Button>
        <Button
          variant="outline"
          size="sm"
          :disabled="loading || records.length < limit"
          @click="nextPage"
        >
          Next <ChevronRight />
        </Button>
      </div>
    </div>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">Loading usage…</p>

    <EmptyState
      v-else-if="!records.length"
      title="No usage recorded"
      description="Usage rows appear here once requests flow through /v1/*."
    >
      <template #icon><ScrollText class="size-5" /></template>
    </EmptyState>

    <Card v-else>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Time</TableHead>
            <TableHead>Model</TableHead>
            <TableHead>Resolved</TableHead>
            <TableHead>Attempt</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Tokens</TableHead>
            <TableHead>Cost</TableHead>
            <TableHead>Latency</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="record in records" :key="record.id">
            <TableCell class="text-xs whitespace-nowrap text-muted-foreground">
              {{ formatDateTime(record.created_at) }}
            </TableCell>
            <TableCell>
              <code class="text-xs">{{ record.requested_model }}</code>
            </TableCell>
            <TableCell>
              <div v-if="record.connection_name || record.resolved_provider || record.resolved_model" class="flex flex-col gap-1">
                <code v-if="record.connection_name" class="text-xs">{{ record.connection_name }}</code>
                <Badge v-if="record.resolved_provider" variant="outline" class="w-fit text-xs">
                  {{ record.resolved_provider }}
                </Badge>
                <code v-if="record.resolved_model" class="text-xs text-muted-foreground">
                  {{ record.resolved_model }}
                </code>
              </div>
              <span v-else class="text-muted-foreground">—</span>
            </TableCell>
            <TableCell class="text-muted-foreground">{{ record.attempt }}</TableCell>
            <TableCell>
              <Badge :variant="record.status === 'ok' ? 'default' : 'destructive'">
                {{ record.status }}
              </Badge>
            </TableCell>
            <TableCell class="text-xs text-muted-foreground">
              <UsageBreakdownPopover
                title="Token breakdown"
                :total="record.prompt_tokens + record.completion_tokens"
                :rows="tokenRows(record)"
              >
                {{ formatNumber(record.prompt_tokens) }} / {{ formatNumber(record.completion_tokens) }}
                <span v-if="record.cached_tokens"> · {{ formatNumber(record.cached_tokens) }} cached</span>
                <span v-if="record.reasoning_tokens"> · {{ formatNumber(record.reasoning_tokens) }} reasoning</span>
              </UsageBreakdownPopover>
            </TableCell>
            <TableCell class="text-xs">
              <UsageBreakdownPopover
                title="Cost breakdown"
                total-format="cost"
                :total="record.cost_usd"
                :rows="costRows(record)"
              >
                {{ formatCost(record.cost_usd) }}
              </UsageBreakdownPopover>
            </TableCell>
            <TableCell class="text-xs">{{ formatLatency(record.latency_ms) }}</TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Card>
  </div>
</template>
