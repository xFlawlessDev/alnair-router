<script setup lang="ts">
import { CloudDownload, Pencil, Plus, RefreshCw, Search, Trash2 } from '@lucide/vue';
import { computed, onMounted, ref } from 'vue';
import { toast } from 'vue-sonner';

import ConfirmDialog from '@/components/ConfirmDialog.vue';
import PageHeader from '@/components/PageHeader.vue';
import PriceFormDialog from '@/components/pricing/PriceFormDialog.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { ApiError, api } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import type { ModelPrice, PriceMatch, PricingSyncStatus } from '@/types/api';

/** Long catalogs are searchable; render at most this many rows at once. */
const VISIBLE_LIMIT = 200;

const prices = ref<ModelPrice[]>([]);
const status = ref<PricingSyncStatus | null>(null);
const search = ref('');
const loading = ref(true);
const syncing = ref(false);
const error = ref<string | null>(null);
const formOpen = ref(false);
const editing = ref<ModelPrice | null>(null);
const deleting = ref<ModelPrice | null>(null);
const deletingBusy = ref(false);
const matchModel = ref('');
const matchResult = ref<PriceMatch | null>(null);
const matchBusy = ref(false);

const overrides = computed(() => prices.value.filter((price) => price.source === 'override').length);

const filtered = computed(() => {
  const term = search.value.trim().toLowerCase();
  if (!term) return prices.value;
  return prices.value.filter((price) => price.model.toLowerCase().includes(term));
});

const visible = computed(() => filtered.value.slice(0, VISIBLE_LIMIT));

function rate(value: number | null): string {
  if (value === null) return '—';
  return `$${value.toFixed(4).replace(/0+$/, '').replace(/\.$/, '')}`;
}

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [priceList, syncStatus] = await Promise.all([api.listPricing(), api.pricingSyncStatus()]);
    prices.value = priceList;
    status.value = syncStatus;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load pricing';
  } finally {
    loading.value = false;
  }
}

async function syncNow(): Promise<void> {
  syncing.value = true;
  try {
    const result = await api.syncPricing();
    toast.success(`Crawled ${result.model_count} model prices`);
    await load();
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Sync failed');
  } finally {
    syncing.value = false;
  }
}

function openCreate(): void {
  editing.value = null;
  formOpen.value = true;
}

/** Shows which catalog key prices a given model id (relay paths included). */
async function testMatch(): Promise<void> {
  const model = matchModel.value.trim();
  if (!model) return;
  matchBusy.value = true;
  try {
    matchResult.value = await api.matchPricing(model);
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Match lookup failed');
  } finally {
    matchBusy.value = false;
  }
}

function openEdit(price: ModelPrice): void {
  editing.value = price;
  formOpen.value = true;
}

async function confirmDelete(): Promise<void> {
  if (!deleting.value) return;
  deletingBusy.value = true;
  const model = deleting.value.model;
  try {
    await api.deletePricing(model);
    toast.success(`Override for “${model}” removed`);
    deleting.value = null;
    await load();
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to delete the override');
  } finally {
    deletingBusy.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="Pricing"
      description="Model rates that turn tokens into recorded cost. Overrides win over the crawled catalog."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button variant="outline" :disabled="syncing" @click="syncNow">
          <CloudDownload :class="syncing ? 'animate-pulse' : ''" />
          {{ syncing ? 'Syncing…' : 'Sync now' }}
        </Button>
        <Button @click="openCreate"><Plus /> Add override</Button>
      </template>
    </PageHeader>

    <Card>
      <CardHeader class="pb-3">
        <CardTitle class="text-base">Test a model id</CardTitle>
        <CardDescription>
          See which catalog key prices a request — handy for relay paths like
          <code>ocg/openai/gpt-5.6-luna</code>.
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-3">
        <div class="flex flex-wrap items-center gap-2">
          <Input
            v-model="matchModel"
            placeholder="ocg/openai/gpt-5.6-luna"
            class="sm:max-w-sm"
            aria-label="Model id to match"
            @keyup.enter="testMatch"
          />
          <Button variant="outline" :disabled="matchBusy || !matchModel.trim()" @click="testMatch">
            <Search /> Test match
          </Button>
        </div>
        <p v-if="matchResult" class="flex flex-wrap items-center gap-2 text-xs">
          <template v-if="matchResult.matched">
            <code>{{ matchResult.model }}</code>
            <span>→</span>
            <code>{{ matchResult.matched }}</code>
            <Badge variant="outline">{{ matchResult.source }}</Badge>
            <span v-if="matchResult.price" class="text-muted-foreground">
              in {{ rate(matchResult.price.input_per_million_usd) }} · out
              {{ rate(matchResult.price.output_per_million_usd) }}
            </span>
          </template>
          <span v-else class="text-destructive">
            No catalog entry matches — add an override or set the connection's pricing model.
          </span>
        </p>
      </CardContent>
    </Card>

    <Card>
      <CardHeader class="pb-3">
        <CardTitle class="text-base">Catalog status</CardTitle>
        <CardDescription>
          {{
            status
              ? `Last crawl ${formatDateTime(status.synced_at)} · ${status.model_count} models`
              : 'No crawl yet — add overrides manually or enable pricing.sync_enabled.'
          }}
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-3">
        <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <Badge variant="outline">{{ prices.length }} rows</Badge>
          <Badge variant="secondary">{{ overrides }} overrides</Badge>
          <span v-if="status" class="truncate" :title="status.source">{{ status.source }}</span>
        </div>
      </CardContent>
    </Card>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
    </Card>

    <template v-else>
      <Input
        v-model="search"
        placeholder="Filter by model id…"
        class="sm:max-w-sm"
        aria-label="Filter model prices"
      />

      <p v-if="loading" class="text-sm text-muted-foreground">Loading pricing…</p>

      <p v-else-if="!prices.length" class="text-sm text-muted-foreground">
        No prices stored. Use <strong>Sync now</strong> to crawl the configured catalog, or add an
        override.
      </p>

      <Card v-else>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Model</TableHead>
              <TableHead>Input</TableHead>
              <TableHead>Output</TableHead>
              <TableHead>Cache read</TableHead>
              <TableHead>Cache write</TableHead>
              <TableHead>Reasoning</TableHead>
              <TableHead>Source</TableHead>
              <TableHead>Updated</TableHead>
              <TableHead class="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow v-for="price in visible" :key="`${price.model}:${price.source}`">
              <TableCell><code class="text-xs">{{ price.model }}</code></TableCell>
              <TableCell class="text-xs">{{ rate(price.input_per_million_usd) }}</TableCell>
              <TableCell class="text-xs">{{ rate(price.output_per_million_usd) }}</TableCell>
              <TableCell class="text-xs">{{ rate(price.cache_read_per_million_usd) }}</TableCell>
              <TableCell class="text-xs">{{ rate(price.cache_write_per_million_usd) }}</TableCell>
              <TableCell class="text-xs">{{ rate(price.reasoning_per_million_usd) }}</TableCell>
              <TableCell>
                <Badge :variant="price.source === 'override' ? 'default' : 'outline'">
                  {{ price.source }}
                </Badge>
              </TableCell>
              <TableCell class="text-xs whitespace-nowrap text-muted-foreground">
                {{ formatDateTime(price.updated_at) }}
              </TableCell>
              <TableCell class="text-right">
                <div v-if="price.source === 'override'" class="flex justify-end gap-1">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    :aria-label="`Edit price for ${price.model}`"
                    @click="openEdit(price)"
                  >
                    <Pencil />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    class="text-destructive hover:text-destructive"
                    :aria-label="`Delete price for ${price.model}`"
                    @click="deleting = price"
                  >
                    <Trash2 />
                  </Button>
                </div>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
        <p v-if="filtered.length > VISIBLE_LIMIT" class="border-t p-3 text-xs text-muted-foreground">
          Showing the first {{ VISIBLE_LIMIT }} of {{ filtered.length }} matches — refine the filter.
        </p>
      </Card>
    </template>

    <PriceFormDialog v-model:open="formOpen" :price="editing" @saved="load" />

    <ConfirmDialog
      :open="deleting !== null"
      title="Remove override?"
      :description="`Usage falls back to the crawled catalog or the built-in rates for “${deleting?.model}”.`"
      :pending="deletingBusy"
      @update:open="deleting = $event ? deleting : null"
      @confirm="confirmDelete"
    />
  </div>
</template>
