<script setup lang="ts">
import {
  Boxes,
  CircleAlert,
  CircleCheck,
  Copy,
  KeyRound,
  Plug,
  RefreshCw,
  Waypoints,
} from '@lucide/vue';
import { computed, onMounted, ref } from 'vue';
import { RouterLink } from 'vue-router';
import { toast } from 'vue-sonner';

import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import UsageSummaryCards from '@/components/UsageSummaryCards.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
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
import { formatRate } from '@/lib/format';
import { USAGE_RANGES, rangeToSince } from '@/lib/ranges';
import type {
  HealthResponse,
  InitState,
  ModelCatalogEntry,
  UsageSummary,
  VersionResponse,
} from '@/types/api';

const baseUrl = `${window.location.origin}/v1`;

const health = ref<HealthResponse | null>(null);
const version = ref<VersionResponse | null>(null);
const initState = ref<InitState | null>(null);
const summary = ref<UsageSummary | null>(null);
const range = ref('all');
const loading = ref(true);
const error = ref<string | null>(null);

const modelEntries = ref<ModelCatalogEntry[]>([]);
const modelSearch = ref('');

const filteredModels = computed(() => {
  const term = modelSearch.value.trim().toLowerCase();
  if (!term) return modelEntries.value;
  return modelEntries.value.filter((entry) =>
    [entry.id, entry.provider, entry.provider_type, entry.upstream_model ?? ''].some((value) =>
      value.toLowerCase().includes(term),
    ),
  );
});

function isOpenAlias(entry: ModelCatalogEntry): boolean {
  return entry.kind === 'alias' && entry.upstream_model === null;
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
    toast.error('Clipboard is not available');
  }
}

function priceTitle(entry: ModelCatalogEntry): string | undefined {
  if (!entry.price_matched || entry.price_matched === entry.upstream_model) return undefined;
  return `Matched catalog key: ${entry.price_matched}`;
}

async function copyBaseUrl(): Promise<void> {
  try {
    await navigator.clipboard.writeText(baseUrl);
    toast.success(`Copied "${baseUrl}"`);
  } catch {
    toast.error('Clipboard is not available');
  }
}

const healthy = computed(() => health.value?.status === 'ok');
const since = computed(() => rangeToSince(range.value));

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [healthResponse, versionResponse, initResponse, summaryResponse, modelsResponse] =
      await Promise.all([
        api.health(),
        api.version(),
        api.initState(),
        api.usageSummary({ since: since.value }),
        api.modelCatalog(),
      ]);
    health.value = healthResponse;
    version.value = versionResponse;
    initState.value = initResponse;
    summary.value = summaryResponse;
    modelEntries.value = modelsResponse.data;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load router status';
  } finally {
    loading.value = false;
  }
}

async function reloadSummary(): Promise<void> {
  try {
    summary.value = await api.usageSummary({ since: since.value });
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load usage';
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

    <button
      class="inline-flex w-fit items-center gap-1.5 rounded-md border bg-muted px-2.5 py-1 font-mono text-xs transition-colors hover:bg-accent"
      title="Copy base URL"
      @click="copyBaseUrl"
    >
      {{ baseUrl }}
      <Copy class="size-3 text-muted-foreground" />
    </button>

    <Card v-if="error" class="border-destructive/40">
      <CardHeader>
        <CardTitle class="flex items-center gap-2 text-destructive">
          <CircleAlert class="size-4" /> Cannot reach the router
        </CardTitle>
        <CardDescription>{{ error }}</CardDescription>
      </CardHeader>
      <CardContent>
        <p class="text-sm text-muted-foreground">
          Start it with <code class="rounded bg-muted px-1.5 py-0.5">cargo run -p alnair-router</code>
          and make sure the dev proxy target matches
          <code class="rounded bg-muted px-1.5 py-0.5">ALNAIR_ROUTER_URL</code>.
        </p>
      </CardContent>
    </Card>

    <template v-else>
      <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardHeader class="pb-2">
            <CardTitle class="flex items-center gap-2 text-sm font-medium text-muted-foreground">
              <CircleCheck class="size-4" /> Status
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p class="text-2xl font-semibold tracking-tight">
              {{ loading ? '…' : healthy ? 'Healthy' : 'Degraded' }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ health ? `${health.service} v${health.version}` : 'Waiting for /api/health' }}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardTitle class="flex items-center gap-2 text-sm font-medium text-muted-foreground">
              <Plug class="size-4" /> Connections
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p class="text-2xl font-semibold tracking-tight">
              {{ initState ? `${initState.enabled_connections} / ${initState.connections}` : '…' }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">Enabled / total upstreams</p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardTitle class="flex items-center gap-2 text-sm font-medium text-muted-foreground">
              <KeyRound class="size-4" /> Client auth
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p class="text-2xl font-semibold tracking-tight">
              {{ initState ? (initState.require_api_key ? 'Required' : 'Open') : '…' }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ initState?.require_api_key ? 'Bearer key needed on /v1' : 'No key needed on /v1' }}
              ·
              <RouterLink to="/settings" class="underline underline-offset-4">Configure</RouterLink>
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardTitle class="flex items-center gap-2 text-sm font-medium text-muted-foreground">
              <Waypoints class="size-4" /> Version
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p class="text-2xl font-semibold tracking-tight">{{ version?.version ?? '…' }}</p>
            <p class="mt-1 text-xs text-muted-foreground">{{ version?.name ?? 'alnair-router' }}</p>
          </CardContent>
        </Card>
      </div>

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
            <p class="mt-1 text-xs text-muted-foreground">An upstream endpoint and its key.</p>
          </RouterLink>
          <RouterLink
            to="/aliases"
            class="rounded-md border p-3 text-sm transition-colors hover:bg-accent"
          >
            <span class="font-medium">2. Create an alias</span>
            <p class="mt-1 text-xs text-muted-foreground">Map a prefix like <code>oa</code> to it.</p>
          </RouterLink>
          <RouterLink
            to="/combos"
            class="rounded-md border p-3 text-sm transition-colors hover:bg-accent"
          >
            <span class="font-medium">3. Build a combo</span>
            <p class="mt-1 text-xs text-muted-foreground">Chain tiers for automatic failover.</p>
          </RouterLink>
        </CardContent>
      </Card>

      <section class="flex flex-col gap-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <h2 class="text-lg font-semibold tracking-tight">Usage</h2>
          <Select v-model="range" @update:model-value="reloadSummary">
            <SelectTrigger class="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="item in USAGE_RANGES" :key="item.value" :value="item.value">
                {{ item.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <UsageSummaryCards :summary="summary" />
        <p class="text-xs text-muted-foreground">
          One row is recorded per upstream attempt, failures included. Full detail lives in
          <RouterLink to="/usage" class="underline underline-offset-4">Usage</RouterLink>.
        </p>
      </section>

      <section class="flex flex-col gap-4">
        <h2 class="text-lg font-semibold tracking-tight">Models</h2>

        <EmptyState
          v-if="!modelEntries.length"
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
            <Badge variant="outline">{{ filteredModels.length }} of {{ modelEntries.length }}</Badge>
          </div>

          <p v-if="!filteredModels.length" class="text-sm text-muted-foreground">
            Nothing matches "{{ modelSearch.trim() }}".
          </p>

          <Card v-else>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Model</TableHead>
                  <TableHead>Provider</TableHead>
                  <TableHead>Upstream model</TableHead>
                  <TableHead>Input ($/1M)</TableHead>
                  <TableHead>Output ($/1M)</TableHead>
                  <TableHead>Cache read ($/1M)</TableHead>
                  <TableHead>Cache write ($/1M)</TableHead>
                  <TableHead>Source</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow
                  v-for="entry in filteredModels"
                  :key="`${entry.kind}:${entry.id}:${entry.tier ?? 0}:${entry.upstream_model ?? ''}`"
                >
                  <TableCell>
                    <div class="flex items-center gap-2">
                      <code
                        class="inline-block max-w-[14rem] truncate rounded bg-muted px-1.5 py-0.5 align-middle text-xs"
                        :title="copyValue(entry)"
                      >
                        {{ isOpenAlias(entry) ? `${entry.id}/…` : entry.id }}
                      </code>
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6"
                        :aria-label="`Copy ${copyValue(entry)}`"
                        :title="`Copy ${copyValue(entry)}`"
                        @click="copyId(entry)"
                      >
                        <Copy class="size-3.5" />
                      </Button>
                      <Badge variant="outline">{{ entry.kind }}</Badge>
                      <Badge v-if="entry.tier" variant="secondary">#{{ entry.tier }}</Badge>
                    </div>
                  </TableCell>
                  <TableCell>
                    <p class="font-medium">{{ entry.provider }}</p>
                    <p class="text-xs text-muted-foreground">{{ entry.provider_type }}</p>
                  </TableCell>
                  <TableCell>
                    <code
                      v-if="entry.upstream_model"
                      class="inline-block max-w-[14rem] truncate align-middle text-xs"
                      :title="entry.upstream_model"
                    >
                      {{ entry.upstream_model }}
                    </code>
                    <span v-else class="text-xs text-muted-foreground">any model</span>
                  </TableCell>
                  <TableCell :title="priceTitle(entry)">
                    {{ formatRate(entry.price?.input_per_million_usd) }}
                  </TableCell>
                  <TableCell :title="priceTitle(entry)">
                    {{ formatRate(entry.price?.output_per_million_usd) }}
                  </TableCell>
                  <TableCell :title="priceTitle(entry)">
                    {{ formatRate(entry.price?.cache_read_per_million_usd) }}
                  </TableCell>
                  <TableCell :title="priceTitle(entry)">
                    {{ formatRate(entry.price?.cache_write_per_million_usd) }}
                  </TableCell>
                  <TableCell>
                    <Badge v-if="entry.price_source" variant="outline">{{ entry.price_source }}</Badge>
                    <span v-else class="text-xs text-muted-foreground">—</span>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </Card>

          <p class="text-xs text-muted-foreground">
            Prices are USD per million tokens from the pricing catalog — set overrides on the
            <RouterLink to="/pricing" class="underline underline-offset-4">Pricing</RouterLink> page.
            Aliases without a pinned model copy as a prefix; append <code>/model</code> to call one.
          </p>
        </template>
      </section>
    </template>
  </div>
</template>
