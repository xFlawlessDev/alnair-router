<script setup lang="ts">
import {
  ArrowUpCircle,
  Boxes,
  CircleAlert,
  CircleCheck,
  Copy,
  ExternalLink,
  Gauge,
  KeyRound,
  Plug,
  RefreshCw,
  Timer,
} from "@lucide/vue";
import { computed, onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { toast } from "vue-sonner";

import EmptyState from "@/components/EmptyState.vue";
import PageHeader from "@/components/PageHeader.vue";
import ProviderIcon from "@/components/ProviderIcon.vue";
import UsageSummaryCards from "@/components/UsageSummaryCards.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ApiError, api } from "@/lib/api";
import { formatDuration, formatNumber, formatRate } from "@/lib/format";
import { USAGE_RANGES, rangeToSince } from "@/lib/ranges";
import type {
  ActivitySnapshot,
  HealthResponse,
  InitState,
  ModelCatalogEntry,
  ModelUsage,
  UpdateStatus,
  UsageSummary,
  VersionResponse,
} from "@/types/api";

const baseUrl = `${window.location.origin}/v1`;
/** Self-service usage page: a client key reads its own rollup there. */
const meUrl = `${window.location.origin}/me`;

const health = ref<HealthResponse | null>(null);
const version = ref<VersionResponse | null>(null);
const update = ref<UpdateStatus | null>(null);
const initState = ref<InitState | null>(null);
const summary = ref<UsageSummary | null>(null);
const usageModels = ref<ModelUsage[]>([]);
const activity = ref<ActivitySnapshot | null>(null);
const range = ref("all");
const loading = ref(true);
const error = ref<string | null>(null);
/** Non-blocking notice: a side panel failed but the page still rendered. */
const partialError = ref<string | null>(null);

const modelEntries = ref<ModelCatalogEntry[]>([]);
const modelSearch = ref("");

const filteredModels = computed(() => {
  const term = modelSearch.value.trim().toLowerCase();
  if (!term) return modelEntries.value;
  return modelEntries.value.filter((entry) =>
    [
      entry.id,
      entry.provider,
      entry.provider_type,
      entry.upstream_model ?? "",
    ].some((value) => value.toLowerCase().includes(term)),
  );
});

function isOpenAlias(entry: ModelCatalogEntry): boolean {
  return entry.kind === "alias" && entry.upstream_model === null;
}

function copyValue(entry: ModelCatalogEntry): string {
  return isOpenAlias(entry) ? `${entry.id}/` : entry.id;
}

async function copyId(entry: ModelCatalogEntry): Promise<void> {
  const value = copyValue(entry);
  try {
    await navigator.clipboard.writeText(value);
    toast.success(`Copied "${value}"`);
  } catch {
    toast.error("Clipboard is not available");
  }
}

function priceTitle(entry: ModelCatalogEntry): string | undefined {
  if (!entry.price_matched || entry.price_matched === entry.upstream_model)
    return undefined;
  return `Matched catalog key: ${entry.price_matched}`;
}

async function copyValueToClipboard(value: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value);
    toast.success(`Copied "${value}"`);
  } catch {
    toast.error("Clipboard is not available");
  }
}

async function copyBaseUrl(): Promise<void> {
  await copyValueToClipboard(baseUrl);
}

async function copyMeUrl(): Promise<void> {
  await copyValueToClipboard(meUrl);
}

const healthy = computed(() => health.value?.status === "ok");
const since = computed(() => rangeToSince(range.value));

/** In-flight requests and the connections currently serving them. */
const inFlight = computed(() => activity.value?.active.length ?? 0);
const busyConnections = computed(() => {
  const connections = activity.value?.connections ?? [];
  return connections.filter((connection) => connection.in_flight > 0).length;
});

/** Uptime plus the tally the activity tracker keeps since boot. */
const activitySummary = computed(() => {
  const snapshot = activity.value;
  if (!snapshot) return null;
  const requests = snapshot.connections.reduce(
    (sum, connection) => sum + connection.requests,
    0,
  );
  const failures = snapshot.connections.reduce(
    (sum, connection) => sum + connection.failures,
    0,
  );
  return { requests, failures };
});

/**
 * Warnings and errors, newest first. `attempt.failed` is the useful one — it
 * carries the upstream reason — while the trailing `request` event only repeats
 * the HTTP status the client already saw.
 */
const recentErrors = computed(() =>
  (activity.value?.events ?? [])
    .filter((event) => event.level !== "info")
    .slice(0, 5),
);

/** The newest recorded request, used as a "traffic is flowing" hint. */
const lastTraffic = computed(() => {
  const events = activity.value?.events ?? [];
  const request = events.find((event) => event.kind === "request");
  return request?.at ?? null;
});

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  partialError.value = null;

  const filter = { since: since.value };
  const [core, models, usage, live, release] = await Promise.allSettled([
    Promise.all([api.health(), api.version(), api.initState()]),
    api.modelCatalog(),
    Promise.all([api.usageSummary(filter), api.usageModels(filter)]),
    api.activity(30),
    api.update(),
  ]);

  if (core.status === "fulfilled") {
    const [healthResponse, versionResponse, initResponse] = core.value;
    health.value = healthResponse;
    version.value = versionResponse;
    initState.value = initResponse;
  } else {
    error.value =
      core.reason instanceof ApiError
        ? core.reason.message
        : "Failed to load router status";
  }

  const failures: string[] = [];
  if (models.status === "fulfilled") {
    modelEntries.value = models.value.data;
  } else {
    failures.push("model catalog");
  }
  if (usage.status === "fulfilled") {
    const [summaryResponse, modelRows] = usage.value;
    summary.value = summaryResponse;
    usageModels.value = modelRows;
  } else {
    failures.push("usage");
  }
  if (live.status === "fulfilled") {
    activity.value = live.value;
  } else {
    failures.push("live activity");
  }
  // A failed update check is not worth a page-level warning: the banner falls
  // back to naming the running version.
  if (release.status === "fulfilled") {
    update.value = release.value;
  }
  if (failures.length) {
    partialError.value = `Could not load: ${failures.join(", ")}.`;
  }

  loading.value = false;
}

/** Re-asks GitHub, bypassing the router's cache. */
async function checkForUpdates(): Promise<void> {
  try {
    update.value = await api.update(true);
    if (update.value.update_available) {
      toast.info(`v${update.value.latest_version} is available`);
    } else if (update.value.error) {
      toast.error(update.value.error);
    } else {
      toast.success("You are on the latest version");
    }
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Update check failed",
    );
  }
}

async function reloadUsage(): Promise<void> {
  const filter = { since: since.value };
  try {
    const [summaryResponse, modelRows] = await Promise.all([
      api.usageSummary(filter),
      api.usageModels(filter),
    ]);
    summary.value = summaryResponse;
    usageModels.value = modelRows;
  } catch (caught) {
    error.value =
      caught instanceof ApiError ? caught.message : "Failed to load usage";
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-8">
    <PageHeader
      title="Overview"
      description="Router status, configuration, and usage at a glance."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
      </template>
    </PageHeader>

    <div class="flex flex-wrap items-center gap-2">
      <button
        class="inline-flex w-fit items-center gap-1.5 rounded-md border bg-muted px-2.5 py-1 font-mono text-xs transition-colors hover:bg-accent"
        title="Copy base URL"
        @click="copyBaseUrl"
      >
        {{ baseUrl }}
        <Copy class="size-3 text-muted-foreground" />
      </button>
      <Badge
        v-if="initState"
        :variant="initState.require_api_key ? 'secondary' : 'outline'"
        :title="
          initState.require_api_key
            ? 'A router-issued bearer key is required on /v1'
            : 'Every request to /v1 is accepted without a key'
        "
      >
        <KeyRound class="size-3" />
        {{ initState.require_api_key ? "Key required" : "Auth open" }}
      </Badge>
      <button
        class="inline-flex w-fit items-center gap-1.5 rounded-md border bg-muted px-2.5 py-1 font-mono text-xs transition-colors hover:bg-accent"
        title="Copy self-service usage URL"
        data-testid="my-usage-copy"
        @click="copyMeUrl"
      >
        {{ meUrl }}
        <Copy class="size-3 text-muted-foreground" />
      </button>
    </div>

    <Card v-if="error" class="border-destructive/40">
      <CardHeader>
        <CardTitle class="flex items-center gap-2 text-destructive">
          <CircleAlert class="size-4" /> Cannot reach the router
        </CardTitle>
        <CardDescription>{{ error }}</CardDescription>
      </CardHeader>
      <CardContent>
        <p class="text-sm text-muted-foreground">
          Start it with
          <code class="rounded bg-muted px-1.5 py-0.5"
            >cargo run -p alnair-router</code
          >
          and make sure the dev proxy target matches
          <code class="rounded bg-muted px-1.5 py-0.5">ALNAIR_ROUTER_URL</code>.
        </p>
      </CardContent>
    </Card>

    <template v-else>
      <Card v-if="partialError" class="border-amber-500/40">
        <CardContent class="flex items-center gap-2 p-4 text-sm">
          <CircleAlert class="size-4 text-amber-500" />
          <span class="text-muted-foreground">{{ partialError }}</span>
          <Button variant="ghost" size="sm" class="ml-auto" @click="load">
            Retry
          </Button>
        </CardContent>
      </Card>

      <Card
        v-if="update?.update_available"
        class="border-emerald-500/40"
        data-testid="update-banner"
      >
        <CardContent class="flex flex-wrap items-center gap-2 p-4 text-sm">
          <ArrowUpCircle class="size-4 text-emerald-500" />
          <span>
            <span class="font-medium"
              >v{{ update.latest_version }} is available</span
            >
            <span class="text-muted-foreground">
              — this router runs v{{ update.version }}.</span
            >
          </span>
          <a
            v-if="update.release_url"
            :href="update.release_url"
            target="_blank"
            rel="noopener noreferrer"
            class="inline-flex items-center gap-1 underline underline-offset-4"
          >
            Release notes <ExternalLink class="size-3" />
          </a>
          <Button
            variant="ghost"
            size="sm"
            class="ml-auto"
            @click="checkForUpdates"
          >
            Check again
          </Button>
        </CardContent>
      </Card>

      <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardHeader class="pb-2">
            <CardTitle
              class="flex items-center gap-2 text-sm font-medium text-muted-foreground"
            >
              <CircleCheck class="size-4" /> Status
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Skeleton v-if="loading" class="h-8 w-24" />
            <p v-else class="text-2xl font-semibold tracking-tight">
              {{ healthy ? "Healthy" : "Degraded" }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{
                health
                  ? `${health.service} v${health.version}`
                  : "Waiting for /api/health"
              }}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardTitle
              class="flex items-center gap-2 text-sm font-medium text-muted-foreground"
            >
              <Plug class="size-4" /> Connections
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Skeleton v-if="loading" class="h-8 w-20" />
            <p v-else class="text-2xl font-semibold tracking-tight">
              {{
                initState
                  ? `${initState.enabled_connections} / ${initState.connections}`
                  : "…"
              }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              Enabled / total upstreams
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardTitle
              class="flex items-center gap-2 text-sm font-medium text-muted-foreground"
            >
              <Gauge class="size-4" /> Live traffic
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Skeleton v-if="loading" class="h-8 w-24" />
            <p v-else class="text-2xl font-semibold tracking-tight">
              {{ activity ? formatNumber(inFlight) : "…" }}
              <span v-if="inFlight" class="text-base font-normal text-amber-500"
                >in flight</span
              >
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              <template v-if="activitySummary">
                {{ formatNumber(activitySummary.requests) }} requests ·
                {{ formatNumber(activitySummary.failures) }} failed since boot
              </template>
              <template v-else>Waiting for /api/activity</template>
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardTitle
              class="flex items-center gap-2 text-sm font-medium text-muted-foreground"
            >
              <Timer class="size-4" /> Uptime
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Skeleton v-if="loading" class="h-8 w-20" />
            <p v-else class="text-2xl font-semibold tracking-tight">
              {{ activity ? formatDuration(activity.uptime_ms) : "…" }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              <template v-if="busyConnections">
                {{ busyConnections }} connection{{
                  busyConnections === 1 ? "" : "s"
                }}
                busy
              </template>
              <template v-else-if="version">
                {{ version.name }} v{{ version.version }}
              </template>
              <template v-else>No in-flight requests</template>
            </p>
          </CardContent>
        </Card>
      </div>

      <Card v-if="recentErrors.length">
        <CardHeader class="pb-3">
          <CardTitle class="flex items-center gap-2 text-base">
            <CircleAlert class="size-4 text-amber-500" /> Recent problems
            <Badge variant="outline">{{ recentErrors.length }}</Badge>
          </CardTitle>
          <CardDescription
            >Tier failures and rejected requests since the router
            started.</CardDescription
          >
        </CardHeader>
        <CardContent class="grid gap-2">
          <div
            v-for="event in recentErrors"
            :key="event.seq"
            class="flex flex-wrap items-baseline gap-x-2 gap-y-1 rounded-md border p-2 text-xs"
          >
            <Badge
              :variant="event.level === 'error' ? 'destructive' : 'secondary'"
            >
              {{ event.status ?? event.level }}
            </Badge>
            <span class="font-medium">{{ event.kind }}</span>
            <code v-if="event.model" class="text-muted-foreground">{{
              event.model
            }}</code>
            <span v-if="event.connection" class="text-muted-foreground"
              >via {{ event.connection }}</span
            >
            <span class="min-w-0 flex-1 truncate" :title="event.message">{{
              event.message
            }}</span>
          </div>
        </CardContent>
      </Card>

      <Card v-if="initState && !initState.initialized">
        <CardHeader>
          <CardTitle>Getting started</CardTitle>
          <CardDescription>
            No enabled connection yet — the router has nothing to route to.
          </CardDescription>
        </CardHeader>
        <CardContent class="grid gap-3 sm:grid-cols-3">
          <RouterLink
            to="/connections"
            class="rounded-md border p-3 text-sm transition-colors hover:bg-accent"
          >
            <span class="font-medium">1. Add a connection</span>
            <p class="mt-1 text-xs text-muted-foreground">
              An upstream endpoint and its key.
            </p>
          </RouterLink>
          <RouterLink
            to="/aliases"
            class="rounded-md border p-3 text-sm transition-colors hover:bg-accent"
          >
            <span class="font-medium">2. Create an alias</span>
            <p class="mt-1 text-xs text-muted-foreground">
              Map a prefix like <code>oa</code> to it.
            </p>
          </RouterLink>
          <RouterLink
            to="/combos"
            class="rounded-md border p-3 text-sm transition-colors hover:bg-accent"
          >
            <span class="font-medium">3. Build a combo</span>
            <p class="mt-1 text-xs text-muted-foreground">
              Chain tiers for automatic failover.
            </p>
          </RouterLink>
        </CardContent>
      </Card>

      <Card v-if="initState?.require_api_key === false">
        <CardContent class="flex items-center gap-2 p-4 text-sm">
          <KeyRound class="size-4 text-amber-500" />
          <span class="text-muted-foreground"
            >Client auth is open — every request to <code>/v1</code> is accepted
            without a key.</span
          >
          <RouterLink
            to="/settings"
            class="ml-auto shrink-0 underline underline-offset-4"
            >Configure</RouterLink
          >
        </CardContent>
      </Card>

      <section class="flex flex-col gap-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <h2 class="text-lg font-semibold tracking-tight">Usage</h2>
          <Select v-model="range" @update:model-value="reloadUsage">
            <SelectTrigger class="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="item in USAGE_RANGES"
                :key="item.value"
                :value="item.value"
              >
                {{ item.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <UsageSummaryCards :summary="summary" :models="usageModels" />
        <p class="text-xs text-muted-foreground">
          One row is recorded per upstream attempt, failures included.
          <template v-if="lastTraffic">
            Last request
            {{ new Date(lastTraffic).toLocaleTimeString() }}.
          </template>
          Full detail lives in
          <RouterLink to="/usage" class="underline underline-offset-4"
            >Usage</RouterLink
          >.
        </p>
      </section>

      <section class="flex flex-col gap-4">
        <h2 class="text-lg font-semibold tracking-tight">Models</h2>

        <p
          v-if="loading"
          class="flex items-center gap-2 text-sm text-muted-foreground"
        >
          <RefreshCw class="size-4 animate-spin" /> Loading the model catalog…
        </p>

        <EmptyState
          v-else-if="!modelEntries.length"
          title="No models yet"
          description="The catalog lists enabled aliases and combos. Create a connection, then map aliases or chain combos to see them here."
        >
          <template #icon><Boxes class="size-5" /></template>
          <template #action>
            <Button as-child>
              <RouterLink to="/aliases">Set up aliases</RouterLink>
            </Button>
          </template>
        </EmptyState>

        <template v-else>
          <div class="flex flex-wrap items-center gap-3">
            <Input
              v-model="modelSearch"
              placeholder="Filter by model, provider or upstream…"
              class="sm:max-w-sm"
              aria-label="Filter the model catalog"
            />
            <Badge variant="outline"
              >{{ filteredModels.length }} of {{ modelEntries.length }}</Badge
            >
          </div>

          <p
            v-if="!filteredModels.length"
            class="text-sm text-muted-foreground"
          >
            Nothing matches "{{ modelSearch.trim() }}".
          </p>

          <Card v-else>
            <Table class="table-fixed">
              <TableHeader>
                <TableRow>
                  <TableHead class="w-[20%]">Model</TableHead>
                  <TableHead class="w-[17%]">Provider</TableHead>
                  <TableHead class="w-[17%]">Upstream model</TableHead>
                  <TableHead class="w-[9%] text-right">Input</TableHead>
                  <TableHead class="w-[9%] text-right">Output</TableHead>
                  <TableHead class="w-[10%] text-right">Cache read</TableHead>
                  <TableHead class="w-[10%] text-right">Cache write</TableHead>
                  <TableHead class="w-[8%]">Source</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow
                  v-for="entry in filteredModels"
                  :key="`${entry.kind}:${entry.id}:${entry.tier ?? 0}:${entry.upstream_model ?? ''}`"
                >
                  <TableCell class="min-w-0">
                    <div class="flex min-w-0 items-center gap-2">
                      <code
                        class="min-w-0 flex-1 truncate rounded bg-muted px-1.5 py-0.5 text-xs"
                        :title="copyValue(entry)"
                      >
                        {{ isOpenAlias(entry) ? `${entry.id}/…` : entry.id }}
                      </code>
                      <Badge variant="outline" class="shrink-0">{{
                        entry.kind
                      }}</Badge>
                      <Badge
                        v-if="entry.tier"
                        variant="secondary"
                        class="shrink-0"
                        >#{{ entry.tier }}</Badge
                      >
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6 shrink-0"
                        :aria-label="`Copy ${copyValue(entry)}`"
                        :title="`Copy ${copyValue(entry)}`"
                        @click="copyId(entry)"
                      >
                        <Copy class="size-3.5" />
                      </Button>
                    </div>
                  </TableCell>
                  <TableCell class="min-w-0">
                    <div class="flex min-w-0 items-center gap-2">
                      <ProviderIcon
                        :id="entry.provider_id"
                        :type="entry.provider_type"
                        :label="entry.provider"
                        :title="entry.provider"
                        class="text-muted-foreground"
                      />
                      <div class="min-w-0">
                        <p class="truncate font-medium" :title="entry.provider">
                          {{ entry.provider }}
                        </p>
                        <p class="truncate text-xs text-muted-foreground">
                          {{ entry.provider_type }}
                        </p>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell class="min-w-0">
                    <code
                      v-if="entry.upstream_model"
                      class="block truncate text-xs"
                      :title="entry.upstream_model"
                    >
                      {{ entry.upstream_model }}
                    </code>
                    <span v-else class="text-xs text-muted-foreground"
                      >any model</span
                    >
                  </TableCell>
                  <TableCell
                    class="text-right tabular-nums"
                    :title="priceTitle(entry)"
                  >
                    {{ formatRate(entry.price?.input_per_million_usd) }}
                  </TableCell>
                  <TableCell
                    class="text-right tabular-nums"
                    :title="priceTitle(entry)"
                  >
                    {{ formatRate(entry.price?.output_per_million_usd) }}
                  </TableCell>
                  <TableCell
                    class="text-right tabular-nums"
                    :title="priceTitle(entry)"
                  >
                    {{ formatRate(entry.price?.cache_read_per_million_usd) }}
                  </TableCell>
                  <TableCell
                    class="text-right tabular-nums"
                    :title="priceTitle(entry)"
                  >
                    {{ formatRate(entry.price?.cache_write_per_million_usd) }}
                  </TableCell>
                  <TableCell>
                    <Badge v-if="entry.price_source" variant="outline">{{
                      entry.price_source
                    }}</Badge>
                    <span v-else class="text-xs text-muted-foreground">—</span>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </Card>

          <p class="text-xs text-muted-foreground">
            Prices are USD per million tokens from the pricing catalog — set
            overrides on the
            <RouterLink to="/pricing" class="underline underline-offset-4"
              >Pricing</RouterLink
            >
            page. Aliases without a pinned model copy as a prefix; append
            <code>/model</code> to call one.
          </p>
        </template>
      </section>
    </template>
  </div>
</template>
