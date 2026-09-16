<script setup lang="ts">
import { Boxes, Copy, Eye, EyeOff, KeyRound, LogOut, RefreshCw } from '@lucide/vue';
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref } from 'vue';
import { toast } from 'vue-sonner';
import UsageSummaryCards from '@/components/UsageSummaryCards.vue';
import BudgetCard from '@/components/usage/BudgetCard.vue';
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
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
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
import { formatCompact, formatCost, formatNumber, formatRate } from '@/lib/format';
import { USAGE_RANGES, rangeToSince } from '@/lib/ranges';
import type {
  MyUsageResponse,
  PublicCatalogEntry,
  PublicCatalogResponse,
  PublicModelUsage,
} from '@/types/api';

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
const catalog = ref<PublicCatalogResponse | null>(null);
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

/** OpenAI-compatible base URL customers point their SDK at. */
const baseUrl = computed(() => `${window.location.origin}/v1`);

const totalTokens = computed(() => {
  const summary = usage.value?.summary;
  return summary ? summary.prompt_tokens + summary.completion_tokens : 0;
});

/** Combos arrive one row per tier; the catalog table shows one row per id. */
const catalogRows = computed(() => {
  const rows: PublicCatalogEntry[] = [];
  const seen = new Set<string>();
  for (const entry of catalog.value?.data ?? []) {
    const key = `${entry.kind}:${entry.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    rows.push(entry);
  }
  return rows;
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

async function copyText(value: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value);
    toast.success(`Copied “${value}”`);
  } catch {
    toast.error('Clipboard is not available');
  }
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
    const [usageResponse, catalogResponse] = await Promise.all([
      api.myUsage(since.value, bucket.value, until.value),
      api.myModels(),
    ]);
    usage.value = usageResponse;
    catalog.value = catalogResponse;
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
    // The catalog rarely changes, so polls reuse it instead of refetching.
    const catalogRequest =
      silent && catalog.value ? Promise.resolve(catalog.value) : api.myModels();
    const [usageResponse, catalogResponse] = await Promise.all([
      api.myUsage(since.value, bucket.value, until.value),
      catalogRequest,
    ]);
    usage.value = usageResponse;
    catalog.value = catalogResponse;
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
  catalog.value = null;
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
      <div
        class="sticky top-0 z-10 -mx-4 flex flex-wrap items-center justify-between gap-3 border-b bg-background/95 px-4 py-3 backdrop-blur supports-[backdrop-filter]:bg-background/60 sm:-mx-6 sm:px-6"
      >
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

        <BudgetCard
          v-if="usage"
          :spend="usage.spend"
          :budget="usage.budget"
        />

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

        <Card>
          <CardHeader>
            <CardTitle class="flex items-center gap-2 text-base">
              <Boxes class="size-4" /> Models you can use
              <Badge variant="outline">{{ catalogRows.length }}</Badge>
            </CardTitle>
            <CardDescription>
              {{
                catalog?.allowed_models.length
                  ? 'Your key is limited to the patterns below.'
                  : 'Your key can call every model on this router.'
              }}
            </CardDescription>
            <div
              v-if="catalog?.allowed_models.length"
              class="flex flex-wrap items-center gap-1.5 pt-1"
            >
              <code
                v-for="pattern in catalog.allowed_models"
                :key="pattern"
                class="rounded bg-muted px-1.5 py-0.5 text-xs"
              >
                {{ pattern }}
              </code>
            </div>
          </CardHeader>
          <CardContent>
            <div class="mb-3 flex flex-wrap items-center gap-2">
              <span class="text-xs text-muted-foreground">Base URL</span>
              <Badge variant="outline" class="font-mono text-[10px] font-normal">
                {{ baseUrl }}
              </Badge>
              <Button
                variant="ghost"
                size="icon"
                class="size-6"
                :aria-label="`Copy ${baseUrl}`"
                :title="`Copy ${baseUrl}`"
                @click="copyText(baseUrl)"
              >
                <Copy class="size-3.5" />
              </Button>
            </div>

            <Table v-if="catalogRows.length" class="table-fixed">
              <TableHeader>
                <TableRow>
                  <TableHead class="w-[36%]">Model</TableHead>
                  <TableHead class="w-[16%]">Input ($/1M)</TableHead>
                  <TableHead class="w-[16%]">Output ($/1M)</TableHead>
                  <TableHead class="w-[16%]">Cache read ($/1M)</TableHead>
                  <TableHead class="w-[16%]">Cache write ($/1M)</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow v-for="entry in catalogRows" :key="`${entry.kind}:${entry.id}`">
                  <TableCell class="min-w-0">
                    <div class="flex min-w-0 items-center gap-2">
                      <code class="block truncate text-xs" :title="entry.id">{{ entry.id }}</code>
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6 shrink-0"
                        :aria-label="`Copy ${entry.id}`"
                        :title="`Copy ${entry.id}`"
                        @click="copyText(entry.id)"
                      >
                        <Copy class="size-3.5" />
                      </Button>
                    </div>
                  </TableCell>
                  <TableCell>
                    <TooltipProvider v-if="entry.kind === 'combo' && entry.price?.input_per_million_usd">
                      <Tooltip>
                        <TooltipTrigger as-child>
                          <span class="cursor-help border-b border-dashed border-muted-foreground/50">{{ formatRate(entry.price?.input_per_million_usd) }}</span>
                        </TooltipTrigger>
                        <TooltipContent>Pricing varies by tier</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                    <template v-else>{{ formatRate(entry.price?.input_per_million_usd) }}</template>
                  </TableCell>
                  <TableCell>
                    <TooltipProvider v-if="entry.kind === 'combo' && entry.price?.output_per_million_usd">
                      <Tooltip>
                        <TooltipTrigger as-child>
                          <span class="cursor-help border-b border-dashed border-muted-foreground/50">{{ formatRate(entry.price?.output_per_million_usd) }}</span>
                        </TooltipTrigger>
                        <TooltipContent>Pricing varies by tier</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                    <template v-else>{{ formatRate(entry.price?.output_per_million_usd) }}</template>
                  </TableCell>
                  <TableCell>
                    <TooltipProvider v-if="entry.kind === 'combo' && entry.price?.cache_read_per_million_usd">
                      <Tooltip>
                        <TooltipTrigger as-child>
                          <span class="cursor-help border-b border-dashed border-muted-foreground/50">{{ formatRate(entry.price?.cache_read_per_million_usd) }}</span>
                        </TooltipTrigger>
                        <TooltipContent>Pricing varies by tier</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                    <template v-else>{{ formatRate(entry.price?.cache_read_per_million_usd) }}</template>
                  </TableCell>
                  <TableCell>
                    <TooltipProvider v-if="entry.kind === 'combo' && entry.price?.cache_write_per_million_usd">
                      <Tooltip>
                        <TooltipTrigger as-child>
                          <span class="cursor-help border-b border-dashed border-muted-foreground/50">{{ formatRate(entry.price?.cache_write_per_million_usd) }}</span>
                        </TooltipTrigger>
                        <TooltipContent>Pricing varies by tier</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                    <template v-else>{{ formatRate(entry.price?.cache_write_per_million_usd) }}</template>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
            <p v-else class="text-sm text-muted-foreground">
              No catalog entries match this key's allowlist.
            </p>
          </CardContent>
        </Card>
      </template>
    </template>
  </div>
</template>
