<script setup lang="ts">
import { CircleAlert, CircleCheck, KeyRound, Plug, RefreshCw, Waypoints } from '@lucide/vue';
import { computed, onMounted, ref } from 'vue';
import { RouterLink } from 'vue-router';

import PageHeader from '@/components/PageHeader.vue';
import UsageSummaryCards from '@/components/UsageSummaryCards.vue';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { ApiError, api } from '@/lib/api';
import { USAGE_RANGES, rangeToSince } from '@/lib/ranges';
import type { HealthResponse, InitState, UsageSummary, VersionResponse } from '@/types/api';

const health = ref<HealthResponse | null>(null);
const version = ref<VersionResponse | null>(null);
const initState = ref<InitState | null>(null);
const summary = ref<UsageSummary | null>(null);
const range = ref('all');
const loading = ref(true);
const error = ref<string | null>(null);

const healthy = computed(() => health.value?.status === 'ok');
const since = computed(() => rangeToSince(range.value));

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [healthResponse, versionResponse, initResponse, summaryResponse] = await Promise.all([
      api.health(),
      api.version(),
      api.initState(),
      api.usageSummary(since.value),
    ]);
    health.value = healthResponse;
    version.value = versionResponse;
    initState.value = initResponse;
    summary.value = summaryResponse;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load router status';
  } finally {
    loading.value = false;
  }
}

async function reloadSummary(): Promise<void> {
  try {
    summary.value = await api.usageSummary(since.value);
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
    </template>
  </div>
</template>
