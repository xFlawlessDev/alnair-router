<script setup lang="ts">
import { ChevronLeft, ChevronRight, Pause, Play, RefreshCw, ScrollText } from '@lucide/vue';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { RouterLink } from 'vue-router';

import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import UsageSummaryCards from '@/components/UsageSummaryCards.vue';
import ProviderTopology from '@/components/usage/ProviderTopology.vue';
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
import type { ActivitySnapshot, UsageRecord, UsageSummary } from '@/types/api';

const limits = [50, 100, 200, 500];

const records = ref<UsageRecord[]>([]);
const summary = ref<UsageSummary | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const range = ref('all');
const limit = ref(100);
const offset = ref(0);

// Live activity polling
const live = ref(true);
const activity = ref<ActivitySnapshot | null>(null);
const updatedAt = ref<Date | null>(null);
let activityTimer: number | undefined;
let tableTimer: number | undefined;

const nodes = computed(() => activity.value?.connections ?? []);
const activeNodes = computed(() => nodes.value.filter((node) => node.in_flight > 0));
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

/** `silent` refresh keeps the current table on screen without a loading flash. */
async function load(options: { silent?: boolean } = {}): Promise<void> {
  const silent = options.silent === true;
  if (!silent) loading.value = true;
  error.value = null;
  try {
    const [recordsResponse, summaryResponse] = await Promise.all([
      api.listUsage(limit.value, offset.value),
      api.usageSummary(rangeToSince(range.value)),
    ]);
    records.value = recordsResponse;
    summary.value = summaryResponse;
    updatedAt.value = new Date();
  } catch (caught) {
    if (!silent) {
      error.value = caught instanceof ApiError ? caught.message : 'Failed to load usage';
    }
  } finally {
    // Always clear the initial spinner: the first load is silent (polling).
    loading.value = false;
  }
}

function applyFilters(): void {
  offset.value = 0;
  void load();
}

function previousPage(): void {
  offset.value = Math.max(0, offset.value - limit.value);
  void load();
}

function nextPage(): void {
  offset.value += limit.value;
  void load();
}

onMounted(() => {
  if (live.value) startPolling();
  else void load();
});
onUnmounted(stopPolling);
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
              <div v-if="record.resolved_provider || record.resolved_model" class="flex flex-col gap-1">
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
              {{ formatNumber(record.prompt_tokens) }} / {{ formatNumber(record.completion_tokens) }}
              <span v-if="record.cached_tokens"> · {{ formatNumber(record.cached_tokens) }} cached</span>
            </TableCell>
            <TableCell class="text-xs">{{ formatCost(record.cost_usd) }}</TableCell>
            <TableCell class="text-xs">{{ formatLatency(record.latency_ms) }}</TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Card>
  </div>
</template>
