<script setup lang="ts">
import { Eye, EyeOff, KeyRound, LogOut, RefreshCw } from '@lucide/vue';
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref } from 'vue';
import { toast } from 'vue-sonner';

import UsageSummaryCards from '@/components/UsageSummaryCards.vue';
import UsageBreakdownPopover, {
  type BreakdownRow,
} from '@/components/usage/UsageBreakdownPopover.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Spinner } from '@/components/ui/spinner';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { ApiError, api } from '@/lib/api';
import { getClientKey, setClientKey, useClientKey } from '@/lib/clientKey';
import { formatCompact, formatCost, formatNumber } from '@/lib/format';
import { USAGE_RANGES, rangeToSince } from '@/lib/ranges';
import type { MyUsageResponse, PublicModelUsage } from '@/types/api';

const { clear } = useClientKey();

/** Unovis is heavy; keep it out of the main bundle until the chart renders. */
const UsageTrendChart = defineAsyncComponent(
  () => import('@/components/usage/UsageTrendChart.vue'),
);

const connected = ref(getClientKey() !== '');
const draft = ref('');
const connecting = ref(false);
const connectError = ref<string | null>(null);
const reveal = ref(false);
const usage = ref<MyUsageResponse | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);
const range = ref('all');
const month = ref('any');

/** Last 12 months for the month selector, newest first. */
const MONTHS = Array.from({ length: 12 }, (_, index) => {
  const date = new Date(
    Date.UTC(new Date().getUTCFullYear(), new Date().getUTCMonth() - index, 1),
  );
  return {
    value: `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, '0')}`,
    label: new Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric' }).format(date),
  };
});

/** Short ranges read better with hour buckets; longer ones with days. */
function bucketFor(value: string): 'hour' | 'day' {
  return value === '1h' || value === '24h' ? 'hour' : 'day';
}

/** A selected month wins over the quick ranges; both feed the same window. */
function queryWindow(): { since: string | null; until: string | null } {
  if (month.value === 'any') {
    return { since: rangeToSince(range.value), until: null };
  }

  const [year, monthIndex] = month.value.split('-').map(Number);
  const start = Date.UTC(year, monthIndex - 1, 1);
  const nextMonth = Date.UTC(year, monthIndex, 1);
  const now = Date.now();
  const end = nextMonth > now ? now : nextMonth - 1;
  return { since: new Date(start).toISOString(), until: new Date(end).toISOString() };
}

const since = computed(() => queryWindow().since);
const until = computed(() => queryWindow().until);
const bucket = computed<'hour' | 'day'>(() =>
  month.value !== 'any' ? 'day' : bucketFor(range.value),
);

const totalTokens = computed(() => {
  const summary = usage.value?.summary;
  return summary ? summary.prompt_tokens + summary.completion_tokens : 0;
});

/** One model's token share, with the prompt/completion split in the hint. */
function modelTokenRows(row: PublicModelUsage): BreakdownRow[] {
  return [
    {
      label: row.model,
      value: row.prompt_tokens + row.completion_tokens,
      format: 'tokens',
      hint: `${formatNumber(row.prompt_tokens)} prompt · ${formatNumber(row.completion_tokens)} completion`,
    },
  ];
}

/** One model's share of the recorded cost. */
function modelCostRows(row: PublicModelUsage): BreakdownRow[] {
  return [
    {
      label: row.model,
      value: row.cost_usd,
      format: 'cost',
      hint: `${formatNumber(row.requests)} requests · ${formatNumber(row.error_requests)} failed`,
    },
  ];
}

function onRangeChange(): void {
  month.value = 'any';
  void load();
}

function onMonthChange(): void {
  if (month.value !== 'any') range.value = 'all';
  void load();
}

/** Validates the key before switching to the dashboard, so a bad key is
 * retyped in place instead of locking the page into an error state. */
async function connect(): Promise<void> {
  const key = draft.value.trim();
  if (!key) return;

  connecting.value = true;
  connectError.value = null;
  setClientKey(key);
  try {
    usage.value = await api.myUsage(since.value, bucket.value, until.value);
    connected.value = true;
    draft.value = '';
    reveal.value = false;
  } catch (caught) {
    clear();
    connectError.value =
      caught instanceof ApiError ? caught.message : 'Could not connect with that key';
  } finally {
    connecting.value = false;
  }
}

async function load(silent = false): Promise<void> {
  if (!connected.value) return;
  if (!silent) loading.value = true;
  error.value = null;
  try {
    usage.value = await api.myUsage(since.value, bucket.value, until.value);
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load your usage';
  } finally {
    if (!silent) loading.value = false;
  }
}

/** Keeps the page current without a manual refresh; paused while hidden. */
const LIVE_INTERVAL_MS = 30_000;
let pollTimer: number | undefined;

function startPolling(): void {
  stopPolling();
  pollTimer = window.setInterval(() => {
    if (document.visibilityState === 'visible') void load(true);
  }, LIVE_INTERVAL_MS);
}

function stopPolling(): void {
  if (pollTimer !== undefined) {
    window.clearInterval(pollTimer);
    pollTimer = undefined;
  }
}

function onVisibilityChange(): void {
  if (document.visibilityState === 'visible') void load(true);
}

function disconnect(): void {
  clear();
  connected.value = false;
  usage.value = null;
  error.value = null;
  toast.success('Disconnected');
}

onMounted(() => {
  if (connected.value) void load();
  startPolling();
  document.addEventListener('visibilitychange', onVisibilityChange);
});

onUnmounted(() => {
  stopPolling();
  document.removeEventListener('visibilitychange', onVisibilityChange);
});
</script>

<template>
  <div class="flex flex-col gap-6">
    <div v-if="!connected" class="mx-auto flex w-full max-w-md flex-col gap-4 py-4 sm:py-12">
      <Card>
        <CardHeader class="items-center gap-3 text-center">
          <div
            class="flex size-12 items-center justify-center rounded-full bg-primary/10 text-primary"
          >
            <KeyRound class="size-6" />
          </div>
          <CardTitle class="text-lg">Connect your API key</CardTitle>
          <CardDescription>
            Paste a router-issued key (starts with <code>sk-router-</code>) to see your own usage.
            Ask the router operator if you do not have one.
          </CardDescription>
        </CardHeader>
        <CardContent class="grid gap-4">
          <div class="grid gap-2">
            <Label for="client-key">API key</Label>
            <div class="relative">
              <KeyRound
                class="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
              />
              <Input
                id="client-key"
                v-model="draft"
                :type="reveal ? 'text' : 'password'"
                :class="[
                  'h-10 pl-9 pr-10 font-mono',
                  connectError ? 'border-destructive focus-visible:ring-destructive' : '',
                ]"
                autocomplete="off"
                spellcheck="false"
                placeholder="sk-router-…"
                :aria-invalid="connectError !== null"
                @keyup.enter="connect"
              />
              <button
                type="button"
                class="absolute right-2 top-1/2 -translate-y-1/2 rounded-md p-1.5 text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                :aria-label="reveal ? 'Hide API key' : 'Show API key'"
                @click="reveal = !reveal"
              >
                <EyeOff v-if="reveal" class="size-4" />
                <Eye v-else class="size-4" />
              </button>
            </div>
            <p v-if="connectError" class="text-xs text-destructive">{{ connectError }}</p>
            <p v-else class="text-xs text-muted-foreground">
              Kept in this browser tab only; sent to this router and nowhere else.
            </p>
          </div>
          <Button class="w-full" :disabled="!draft.trim() || connecting" @click="connect">
            <Spinner v-if="connecting" />
            {{ connecting ? 'Connecting…' : 'Connect' }}
          </Button>
        </CardContent>
      </Card>
    </div>

    <template v-else>
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="flex items-center gap-2">
          <Badge variant="secondary">{{ usage?.key.name ?? 'Connected key' }}</Badge>
          <code v-if="usage" class="text-xs text-muted-foreground">{{ usage.key.prefix }}…</code>
          <span class="flex items-center gap-1.5 text-xs text-muted-foreground">
            <span class="size-2 animate-pulse rounded-full bg-emerald-500" />
            Live · 30s
          </span>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <Select v-model="month" @update:model-value="onMonthChange">
            <SelectTrigger class="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="any">Any month</SelectItem>
              <SelectItem v-for="option in MONTHS" :key="option.value" :value="option.value">
                {{ option.label }}
              </SelectItem>
            </SelectContent>
          </Select>
          <Select v-model="range" @update:model-value="onRangeChange">
            <SelectTrigger class="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="item in USAGE_RANGES" :key="item.value" :value="item.value">
                {{ item.label }}
              </SelectItem>
            </SelectContent>
          </Select>
          <Button variant="outline" :disabled="loading" @click="load()">
            <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
          </Button>
          <Button variant="outline" @click="disconnect"><LogOut /> Disconnect</Button>
        </div>
      </div>

      <Card v-if="error" class="border-destructive/40">
        <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
      </Card>

      <template v-else>
        <UsageSummaryCards :summary="usage?.summary ?? null" :models="usage?.models ?? []" />

        <UsageTrendChart
          v-if="usage"
          :points="usage.timeseries"
          :bucket="usage.bucket"
          :since="since"
          :until="until"
        />

        <Card>
          <CardHeader>
            <CardTitle class="text-base">By model</CardTitle>
            <CardDescription>
              Every upstream attempt billed to this key in the selected window.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table v-if="usage?.models.length" class="table-fixed">
              <TableHeader>
                <TableRow>
                  <TableHead class="w-[38%]">Model</TableHead>
                  <TableHead class="w-[14%]">Requests</TableHead>
                  <TableHead class="w-[12%]">Failed</TableHead>
                  <TableHead class="w-[16%]">Tokens</TableHead>
                  <TableHead class="w-[20%]">Cost</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow v-for="row in usage.models" :key="row.model">
                  <TableCell class="min-w-0">
                    <code class="block truncate text-xs" :title="row.model">{{ row.model }}</code>
                  </TableCell>
                  <TableCell>{{ formatNumber(row.requests) }}</TableCell>
                  <TableCell>{{ formatNumber(row.error_requests) }}</TableCell>
                  <TableCell>
                    <UsageBreakdownPopover
                      :title="`Tokens · ${row.model}`"
                      :total="totalTokens"
                      :rows="modelTokenRows(row)"
                      show-percent
                    >
                      {{ formatCompact(row.prompt_tokens + row.completion_tokens) }}
                    </UsageBreakdownPopover>
                  </TableCell>
                  <TableCell>
                    <UsageBreakdownPopover
                      :title="`Cost · ${row.model}`"
                      :total="usage.summary.cost_usd"
                      total-format="cost"
                      :rows="modelCostRows(row)"
                      show-percent
                    >
                      {{ formatCost(row.cost_usd) }}
                    </UsageBreakdownPopover>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
            <p v-else class="text-sm text-muted-foreground">No usage in this window.</p>
          </CardContent>
        </Card>
      </template>
    </template>
  </div>
</template>
