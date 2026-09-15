<script setup lang="ts">
import { Boxes, Copy, RefreshCw } from '@lucide/vue';
import { computed, onMounted, ref } from 'vue';
import { RouterLink } from 'vue-router';
import { toast } from 'vue-sonner';

import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
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
import { formatRate } from '@/lib/format';
import type { ModelCatalogEntry } from '@/types/api';

const entries = ref<ModelCatalogEntry[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const search = ref('');

const filtered = computed(() => {
  const term = search.value.trim().toLowerCase();
  if (!term) return entries.value;
  return entries.value.filter((entry) =>
    [entry.id, entry.provider, entry.provider_type, entry.upstream_model ?? ''].some((value) =>
      value.toLowerCase().includes(term),
    ),
  );
});

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    entries.value = (await api.modelCatalog()).data;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load the model catalog';
  } finally {
    loading.value = false;
  }
}

/** Aliases without a pinned model accept any suffix, so copy the prefix. */
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
    toast.success(`Copied “${value}”`);
  } catch {
    toast.error('Clipboard is not available');
  }
}

function priceTitle(entry: ModelCatalogEntry): string | undefined {
  if (!entry.price_matched || entry.price_matched === entry.upstream_model) return undefined;
  return `Matched catalog key: ${entry.price_matched}`;
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="Models"
      description="Every model reference you can call, the provider that serves it and its catalog price."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
      </template>
    </PageHeader>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">Loading models…</p>

    <EmptyState
      v-else-if="!entries.length"
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
          v-model="search"
          placeholder="Filter by model, provider or upstream…"
          class="sm:max-w-sm"
          aria-label="Filter the model catalog"
        />
        <Badge variant="outline">{{ filtered.length }} of {{ entries.length }}</Badge>
      </div>

      <p v-if="!filtered.length" class="text-sm text-muted-foreground">
        Nothing matches “{{ search.trim() }}”.
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
              v-for="entry in filtered"
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
  </div>
</template>
