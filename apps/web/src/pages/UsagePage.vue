<script setup lang="ts">
import {
  ChevronLeft,
  ChevronRight,
  Pause,
  Play,
  RefreshCw,
  ScrollText,
  TriangleAlert,
} from "@lucide/vue";
import {
  computed,
  defineAsyncComponent,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from "vue";
import { RouterLink } from "vue-router";

import EmptyState from "@/components/EmptyState.vue";
import PageHeader from "@/components/PageHeader.vue";
import UsageSummaryCards from "@/components/UsageSummaryCards.vue";
import ProviderTopology from "@/components/usage/ProviderTopology.vue";
import UsageFilterBar from "@/components/usage/UsageFilterBar.vue";
import UsageRecordSheet from "@/components/usage/UsageRecordSheet.vue";
import UsageTable from "@/components/usage/UsageTable.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ApiError, api } from "@/lib/api";
import { USAGE_RANGES, rangeToSince } from "@/lib/ranges";
import type {
  ActivitySnapshot,
  ApiKey,
  UsageBucket,
  UsageFacets,
  UsageFilter,
  UsageRecord,
  UsageSort,
  UsageSortField,
  UsageSummary,
} from "@/types/api";

/** Sentinel because Select values cannot be empty strings. */
const ALL = "__all__";

/** Unovis is heavy; keep it out of the main bundle until the chart renders. */
const UsageTrendChart = defineAsyncComponent(
  () => import("@/components/usage/UsageTrendChart.vue"),
);

const limits = [50, 100, 200, 500];

const records = ref<UsageRecord[]>([]);
const summary = ref<UsageSummary | null>(null);
const trend = ref<UsageBucket[]>([]);
const keys = ref<ApiKey[]>([]);
const facets = ref<UsageFacets | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
/** A background refresh failed; the table on screen is stale, not live. */
const staleError = ref<string | null>(null);
const range = ref("all");
const limit = ref(100);
const offset = ref(0);
const apiKeyId = ref(ALL);
const model = ref("");
const provider = ref(ALL);
const connection = ref(ALL);
const sort = ref<UsageSort>({ field: "time", descending: true });
const selected = ref<UsageRecord | null>(null);
const detailOpen = ref(false);
let modelTimer: number | undefined;

// Live activity polling
const live = ref(true);
const activity = ref<ActivitySnapshot | null>(null);
const updatedAt = ref<Date | null>(null);
let activityTimer: number | undefined;
let tableTimer: number | undefined;

const nodes = computed(() => activity.value?.connections ?? []);
const activeNodes = computed(() =>
  nodes.value.filter((node) => node.in_flight > 0),
);

/** Total matching rows, from the same `COUNT(*)` the summary cards use. */
const totalRows = computed(() => summary.value?.requests ?? 0);

/** A full page is only the last one when it reaches the filtered total. */
const hasMore = computed(
  () => offset.value + records.value.length < totalRows.value,
);

const rangeLabel = computed(() =>
  records.value.length
    ? `Showing rows ${offset.value + 1}–${offset.value + records.value.length} of ${totalRows.value}`
    : "No rows to show",
);

const filtersActive = computed(
  () =>
    apiKeyId.value !== ALL ||
    model.value.trim() !== "" ||
    provider.value !== ALL ||
    connection.value !== ALL,
);

/** Short ranges read better with hour buckets; longer ones with days. */
const bucket = computed<"hour" | "day">(() =>
  range.value === "1h" || range.value === "24h" ? "hour" : "day",
);

const updatedLabel = computed(() =>
  updatedAt.value
    ? updatedAt.value.toLocaleTimeString(undefined, { hour12: false })
    : "—",
);

async function refreshActivity(): Promise<void> {
  try {
    activity.value = await api.activity(30);
  } catch {
    // Keep the last snapshot; the usage load surfaces connectivity problems.
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
  pollNow();
  void load({ silent: true }); // Initial sync before starting interval-based polling
  activityTimer = window.setInterval(pollNow, 2000);
  tableTimer = window.setInterval(() => void load({ silent: true }), 5000);
}

/** Hidden tabs skip the tick; the visibility handler catches up on return. */
function pollNow(): void {
  if (document.visibilityState !== "visible") return;
  void refreshActivity();
}

function onVisibilityChange(): void {
  if (document.visibilityState !== "visible") return;
  void refreshActivity();
  void load({ silent: true });
}

watch(live, (enabled) => {
  if (enabled) startPolling();
  else stopPolling();
});

// Stop toggles resume the last successful rows; don't lose them during a pause.

/** Current filter set, shared by the table, summary cards and trend chart. */
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
  const filter = currentFilter();
  try {
    const [recordsResponse, summaryResponse, trendResponse] = await Promise.all(
      [
        api.listUsage(limit.value, offset.value, filter, sort.value),
        api.usageSummary(filter),
        api.usageTimeseries(filter, bucket.value),
      ],
    );
    records.value = recordsResponse;
    summary.value = summaryResponse;
    trend.value = trendResponse;
    updatedAt.value = new Date();
    staleError.value = null;
  } catch (caught) {
    const message =
      caught instanceof ApiError ? caught.message : "Failed to load usage";
    // A silent refresh has no error card, so flag the failure in the header
    // rather than leaving stale rows under a healthy "Live" pill.
    if (silent) staleError.value = message;
    else error.value = message;
  } finally {
    // Always clear the initial spinner: the first load is silent (polling).
    loading.value = false;
  }
}

/** Fetches the filter pickers once, then starts the live refresh loop. */
async function init(): Promise<void> {
  try {
    const [keyList, facetList] = await Promise.all([
      api.listKeys(),
      api.usageFacets(),
    ]);
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

function onRangeFilter(value: string): void {
  range.value = value;
  applyFilters();
}

/** Typing in the model filter is debounced; picking a suggestion is instant. */
function onModelFilter(value: string): void {
  model.value = value;
  if (modelTimer !== undefined) window.clearTimeout(modelTimer);
  modelTimer = window.setTimeout(() => applyFilters(), 300);
}

/** Re-clicking the active column flips direction; a new column starts descending. */
function onSort(field: UsageSortField): void {
  sort.value =
    sort.value.field === field
      ? { field, descending: !sort.value.descending }
      : { field, descending: true };
  offset.value = 0;
  void load();
}

function openDetail(record: UsageRecord): void {
  selected.value = record;
  detailOpen.value = true;
}

function clearFilters(): void {
  apiKeyId.value = ALL;
  model.value = "";
  provider.value = ALL;
  connection.value = ALL;
  range.value = "all";
  applyFilters();
}

/** Clears the filter that is hiding rows, keeping the rest of the view. */
function clearToAll(): void {
  apiKeyId.value = ALL;
  model.value = "";
  provider.value = ALL;
  connection.value = ALL;
  range.value = "all";
  applyFilters();
}

function previousPage(): void {
  offset.value = Math.max(0, offset.value - limit.value);
  void load();
}

function nextPage(): void {
  if (!hasMore.value) return;
  offset.value += limit.value;
  void load();
}

onMounted(() => {
  void init();
  document.addEventListener("visibilitychange", onVisibilityChange);
});
onUnmounted(() => {
  if (modelTimer !== undefined) window.clearTimeout(modelTimer);
  document.removeEventListener("visibilitychange", onVisibilityChange);
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
              v-if="live && !staleError"
              class="absolute inline-flex size-full animate-ping rounded-full bg-emerald-500 opacity-75"
            />
            <span
              class="relative inline-flex size-2 rounded-full"
              :class="
                staleError
                  ? 'bg-destructive'
                  : live
                    ? 'bg-emerald-500'
                    : 'bg-muted-foreground/40'
              "
            />
          </span>
          <template v-if="staleError">Refresh failed</template>
          <template v-else>{{ live ? `Live · ${updatedLabel}` : "Paused" }}</template>
        </span>
        <Button variant="outline" size="sm" @click="live = !live">
          <Play v-if="!live" />
          <Pause v-else />
          {{ live ? "Pause" : "Resume" }}
        </Button>
        <Button variant="outline" :disabled="loading" @click="load()">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
      </template>
    </PageHeader>

    <Card v-if="staleError" class="border-destructive/40">
      <CardContent
        class="flex flex-wrap items-center gap-2 p-4 text-sm text-destructive"
      >
        <TriangleAlert class="size-4 shrink-0" />
        <span
          >Showing the last successful refresh ({{ updatedLabel }}). {{
            staleError
          }}</span
        >
        <Button
          variant="outline"
          size="sm"
          class="ml-auto"
          @click="load()"
          >Retry</Button
        >
      </CardContent>
    </Card>

    <Card>
      <CardHeader
        class="flex-row flex-wrap items-center justify-between gap-3 pb-3"
      >
        <div>
          <CardTitle class="text-base">Live activity</CardTitle>
          <CardDescription>
            Providers around the router; the animated edge marks the route
            currently in use.
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
          <RouterLink to="/logs" class="underline underline-offset-4"
            >Open console</RouterLink
          >
        </p>
      </CardContent>
    </Card>

    <UsageSummaryCards :summary="summary" />

    <UsageTrendChart
      v-if="trend.length"
      :points="trend"
      :bucket="bucket"
      :since="rangeToSince(range)"
      :until="null"
    />

    <UsageFilterBar
      :keys="keys"
      :facets="facets"
      :api-key-id="apiKeyId"
      :model="model"
      :provider="provider"
      :connection="connection"
      :range="range"
      @update:api-key-id="onApiKeyFilter"
      @update:model="onModelFilter"
      @update:provider="onProviderFilter"
      @update:connection="onConnectionFilter"
      @update:range="onRangeFilter"
      @clear="clearFilters"
    />

    <div class="flex flex-wrap items-center gap-3">
      <Select v-model.number="limit" @update:model-value="applyFilters">
        <SelectTrigger class="w-32" aria-label="Rows per page">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem v-for="value in limits" :key="value" :value="value">
            {{ value }} rows
          </SelectItem>
        </SelectContent>
      </Select>
      <span class="text-xs text-muted-foreground">{{ rangeLabel }}</span>
      <div class="ml-auto flex items-center gap-1">
        <Button
          variant="outline"
          size="sm"
          :disabled="loading || offset === 0"
          @click="previousPage"
        >
          <ChevronLeft /> Previous
        </Button>
        <Button
          variant="outline"
          size="sm"
          :disabled="loading || !hasMore"
          @click="nextPage"
        >
          Next <ChevronRight />
        </Button>
      </div>
    </div>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{
        error
      }}</CardContent>
    </Card>

    <Card v-else-if="loading" aria-busy="true">
      <CardContent class="grid gap-3 p-4">
        <span class="sr-only">Loading usage…</span>
        <Skeleton v-for="row in 8" :key="row" class="h-9 w-full" />
      </CardContent>
    </Card>

    <EmptyState
      v-else-if="!records.length && filtersActive"
      title="No rows match these filters"
      description="The router has recorded usage, but nothing in the selected window matches. Try a wider time range."
    >
      <template #icon><ScrollText class="size-5" /></template>
      <template #action>
        <Button variant="outline" size="sm" @click="clearToAll">
          Clear filters
        </Button>
      </template>
    </EmptyState>

    <EmptyState
      v-else-if="!records.length"
      title="No usage recorded"
      description="Usage rows appear here once requests flow through /v1/*."
    >
      <template #icon><ScrollText class="size-5" /></template>
      <template #action>
        <Button variant="outline" size="sm" as-child>
          <RouterLink to="/guide">Open the API guide</RouterLink>
        </Button>
      </template>
    </EmptyState>

    <Card v-else>
      <UsageTable
        :records="records"
        :keys="keys"
        :sort="sort"
        :average-latency-ms="summary?.avg_latency_ms ?? 0"
        @sort="onSort"
        @select="openDetail"
      />
    </Card>

    <UsageRecordSheet v-model:open="detailOpen" :record="selected" :keys="keys" />
  </div>
</template>
