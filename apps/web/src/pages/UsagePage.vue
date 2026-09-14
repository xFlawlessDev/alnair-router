<script setup lang="ts">
import { ChevronLeft, ChevronRight, RefreshCw, ScrollText } from '@lucide/vue';
import { onMounted, ref } from 'vue';

import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import UsageSummaryCards from '@/components/UsageSummaryCards.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
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
import { formatCost, formatDateTime, formatLatency, formatNumber } from '@/lib/format';
import { USAGE_RANGES, rangeToSince } from '@/lib/ranges';
import type { UsageRecord, UsageSummary } from '@/types/api';

const limits = [50, 100, 200, 500];

const records = ref<UsageRecord[]>([]);
const summary = ref<UsageSummary | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const range = ref('all');
const limit = ref(100);
const offset = ref(0);

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [recordsResponse, summaryResponse] = await Promise.all([
      api.listUsage(limit.value, offset.value),
      api.usageSummary(rangeToSince(range.value)),
    ]);
    records.value = recordsResponse;
    summary.value = summaryResponse;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load usage';
  } finally {
    loading.value = false;
  }
}

function applyFilters(): void {
  offset.value = 0;
  load();
}

function previousPage(): void {
  offset.value = Math.max(0, offset.value - limit.value);
  load();
}

function nextPage(): void {
  offset.value += limit.value;
  load();
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="Usage"
      description="One row per upstream attempt, failures included."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
      </template>
    </PageHeader>

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
