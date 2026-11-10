<script setup lang="ts">
import { RefreshCw } from "@lucide/vue";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import EmptyState from "@/components/EmptyState.vue";
import PageHeader from "@/components/PageHeader.vue";
import HealthHistoryTable from "@/components/provider-health/HealthHistoryTable.vue";
import ProviderHealthTable from "@/components/provider-health/ProviderHealthTable.vue";
import type {
  HealthDisplayRow,
  ProbeResult,
} from "@/components/provider-health/types";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { ApiError, api } from "@/lib/api";
import type {
  ComboWithEntries,
  Connection,
  ConnectionActivity,
  ProviderHealthHistory,
  ProviderHealthRow,
} from "@/types/api";

const ranges = ["live", "1h", "6h", "24h", "7d", "30d"] as const;
const range = ref<(typeof ranges)[number]>("7d");
const tab = ref<"providers" | "models" | "chains" | "probes">("providers");
const filter = ref<"all" | "healthy" | "degraded" | "unhealthy">("all");
const connections = ref<Connection[]>([]);
const combos = ref<ComboWithEntries[]>([]);
const history = ref<ProviderHealthHistory | null>(null);
const activities = ref<Record<string, ConnectionActivity>>({});
const probes = ref<Record<string, ProbeResult>>({});
const testing = ref<string[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const activityError = ref(false);
let requestNumber = 0;
let timer: ReturnType<typeof setInterval> | undefined;

function status(
  row: ProviderHealthRow | undefined,
): HealthDisplayRow["status"] {
  if (!row?.attempts) return "unknown";
  if (row.failures === row.attempts) return "unhealthy";
  return row.failures ? "degraded" : "healthy";
}
function display(row: ProviderHealthRow): HealthDisplayRow {
  return {
    name: row.name,
    status: status(row),
    attempts: row.attempts,
    failures: row.failures,
    laterAttempts: row.later_attempts,
    p95: row.p95_ms,
    issue: row.failures ? `${row.failures} failed attempts` : undefined,
  };
}
const providerRows = computed(() =>
  connections.value.map((connection): HealthDisplayRow => {
    const historic = history.value?.providers.find(
      (row) => row.name === connection.name,
    );
    const probe = probes.value[connection.id];
    const currentStatus = !connection.enabled
      ? "disabled"
      : probe?.error || probe?.result?.ok === false
        ? "unhealthy"
        : probe?.result?.ok
          ? "healthy"
          : status(historic);
    return {
      name: connection.name,
      detail: `${connection.provider_type} · ${connection.account_count} extra keys`,
      connectionId: connection.id,
      status: currentStatus,
      attempts: historic?.attempts ?? 0,
      failures: historic?.failures ?? 0,
      laterAttempts: historic?.later_attempts ?? 0,
      p95: historic?.p95_ms ?? 0,
      issue:
        probe?.error ??
        (probe?.result?.ok === false ? probe.result.message : undefined) ??
        (historic?.failures
          ? `${historic.failures} failed attempts`
          : undefined),
    };
  }),
);
const modelRows = computed(() => (history.value?.models ?? []).map(display));
const chainRows = computed(() => {
  const known = new Set(
    combos.value.map((item) => item.combo.name.toLowerCase()),
  );
  return (history.value?.references ?? [])
    .filter((row) => known.has(row.name.toLowerCase()))
    .map(display);
});
const selectedRows = computed(() =>
  tab.value === "models"
    ? modelRows.value
    : tab.value === "chains"
      ? chainRows.value
      : providerRows.value,
);
const filteredRows = computed(() =>
  filter.value === "all"
    ? selectedRows.value
    : selectedRows.value.filter((row) => row.status === filter.value),
);
const unhealthy = computed(
  () => providerRows.value.filter((row) => row.status === "unhealthy").length,
);
const degraded = computed(
  () => providerRows.value.filter((row) => row.status === "degraded").length,
);
const healthy = computed(
  () => providerRows.value.filter((row) => row.status === "healthy").length,
);
const total = computed(() => history.value?.overall[0]);

async function load(): Promise<void> {
  const current = ++requestNumber;
  loading.value = true;
  error.value = null;
  const [connectionResult, historyResult, activityResult, comboResult] =
    await Promise.allSettled([
      api.listConnections(),
      api.providerHealth(range.value),
      api.activity(1),
      api.listCombos(),
    ]);
  if (current !== requestNumber) return;
  if (
    connectionResult.status === "rejected" ||
    historyResult.status === "rejected" ||
    comboResult.status === "rejected"
  ) {
    const reason =
      connectionResult.status === "rejected"
        ? connectionResult.reason
        : historyResult.status === "rejected"
          ? historyResult.reason
          : comboResult.status === "rejected"
            ? comboResult.reason
            : null;
    error.value =
      reason instanceof ApiError
        ? reason.message
        : "Could not load provider health.";
  } else {
    const previous = new Map(
      connections.value.map((connection) => [
        connection.id,
        connection.updated_at,
      ]),
    );
    probes.value = Object.fromEntries(
      Object.entries(probes.value).filter(([id]) => {
        const connection = connectionResult.value.find(
          (item) => item.id === id,
        );
        return (
          connection &&
          previous.get(id) === connection.updated_at &&
          connection.enabled
        );
      }),
    );
    connections.value = connectionResult.value;
    history.value = historyResult.value;
    combos.value = comboResult.value;
  }
  activityError.value = activityResult.status === "rejected";
  activities.value =
    activityResult.status === "fulfilled"
      ? Object.fromEntries(
          activityResult.value.connections.map((activity) => [
            activity.id,
            activity,
          ]),
        )
      : {};
  loading.value = false;
}
function selectRange(value: (typeof ranges)[number]): void {
  range.value = value;
  void load();
}
async function test(connection: Connection): Promise<void> {
  if (testing.value.includes(connection.id)) return;
  testing.value = [...testing.value, connection.id];
  try {
    const result = await api.testConnection(connection.id);
    probes.value = {
      ...probes.value,
      [connection.id]: {
        result,
        error: null,
        checkedAt: new Date().toISOString(),
      },
    };
  } catch (caught) {
    probes.value = {
      ...probes.value,
      [connection.id]: {
        result: null,
        error:
          caught instanceof ApiError
            ? caught.message
            : "Connection check failed.",
        checkedAt: new Date().toISOString(),
      },
    };
  } finally {
    testing.value = testing.value.filter((id) => id !== connection.id);
  }
}
function testId(id: string): void {
  const connection = connections.value.find((item) => item.id === id);
  if (connection) void test(connection);
}
onMounted(() => {
  void load();
  timer = setInterval(() => {
    if (range.value === "live" && !loading.value) void load();
  }, 10000);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
  requestNumber++;
});
</script>

<template>
  <div class="editorial-page">
    <PageHeader
      title="Provider Health"
      description="Find upstreams with failed attempts or slow responses."
    >
      <template #actions>
        <div class="flex flex-wrap gap-1" role="group" aria-label="Time range">
          <Button
            v-for="value in ranges"
            :key="value"
            size="sm"
            :variant="range === value ? 'default' : 'outline'"
            :aria-pressed="range === value"
            @click="selectRange(value)"
            >{{ value === "live" ? "Live" : value }}</Button
          >
          <Button
            variant="outline"
            size="sm"
            :disabled="loading"
            aria-label="Refresh provider health"
            @click="load"
            ><RefreshCw :class="loading ? 'animate-spin' : ''"
          /></Button>
        </div>
      </template>
    </PageHeader>
    <Card v-if="error"
      ><CardContent class="p-6 text-sm text-destructive">{{
        error
      }}</CardContent></Card
    >
    <p v-else-if="loading && !history" class="text-sm text-muted-foreground">
      Loading provider health…
    </p>
    <EmptyState
      v-else-if="connections.length === 0"
      title="No connections yet"
      description="Add a provider connection before checking its health."
      ><template #action
        ><Button as-child
          ><RouterLink to="/connections">Add connection</RouterLink></Button
        ></template
      ></EmptyState
    >
    <template v-else>
      <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <Card
          ><CardContent class="p-4"
            ><p class="text-sm text-muted-foreground">Unhealthy</p>
            <p class="mt-1 text-2xl font-semibold">{{ unhealthy }}</p>
            <p class="text-xs text-muted-foreground">
              of {{ connections.length }} connections
            </p></CardContent
          ></Card
        >
        <Card
          ><CardContent class="p-4"
            ><p class="text-sm text-muted-foreground">Degraded</p>
            <p class="mt-1 text-2xl font-semibold">{{ degraded }}</p>
            <p class="text-xs text-muted-foreground">
              {{ healthy }} healthy
            </p></CardContent
          ></Card
        >
        <Card
          ><CardContent class="p-4"
            ><p class="text-sm text-muted-foreground">Later attempts</p>
            <p class="mt-1 text-2xl font-semibold">
              {{ total?.later_attempts ?? 0 }}
            </p>
            <p class="text-xs text-muted-foreground">
              of {{ total?.attempts ?? 0 }} attempts
            </p></CardContent
          ></Card
        >
        <Card
          ><CardContent class="p-4"
            ><p class="text-sm text-muted-foreground">p95 attempt time</p>
            <p class="mt-1 text-2xl font-semibold">
              {{ total ? `${total.p95_ms} ms` : "—" }}
            </p></CardContent
          ></Card
        >
      </div>
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div role="group" aria-label="Health view" class="flex flex-wrap gap-1">
          <Button
            v-for="item in [
              ['providers', 'Providers'],
              ['models', 'Models'],
              ['chains', 'Chains'],
              ['probes', 'Probes'],
            ] as const"
            :key="item[0]"
            size="sm"
            :variant="tab === item[0] ? 'default' : 'outline'"
            :aria-pressed="tab === item[0]"
            @click="tab = item[0]"
            >{{ item[1] }}</Button
          >
        </div>
        <div
          v-if="tab !== 'probes'"
          role="group"
          aria-label="Filter health status"
          class="flex flex-wrap gap-1"
        >
          <Button
            v-for="item in [
              ['all', 'All'],
              ['healthy', 'Healthy'],
              ['degraded', 'Degraded'],
              ['unhealthy', 'Unhealthy'],
            ] as const"
            :key="item[0]"
            size="sm"
            :variant="filter === item[0] ? 'default' : 'outline'"
            :aria-pressed="filter === item[0]"
            @click="filter = item[0]"
            >{{ item[1] }}</Button
          >
        </div>
      </div>
      <p class="text-xs text-muted-foreground">
        Status reflects attempts in the selected period or the latest manual
        probe. Degraded means at least one failed attempt; unhealthy means all
        attempts failed or a probe failed. Unknown means no attempts or probe.
        Later attempts include retries and are not necessarily fallbacks. Uptime
        history is not recorded. Live covers the last 5 minutes.
      </p>
      <p
        v-if="tab === 'probes' && activityError"
        class="text-sm text-destructive"
      >
        Activity is unavailable. Connection checks are still available.
      </p>
      <Card class="editorial-panel"
        ><CardContent class="p-0">
          <ProviderHealthTable
            v-if="tab === 'probes'"
            :connections="connections"
            :activities="activities"
            :probes="probes"
            :testing="testing"
            :activity-available="!activityError"
            @test="test"
          />
          <HealthHistoryTable
            v-else-if="filteredRows.length"
            :rows="filteredRows"
            :label="
              tab === 'models'
                ? 'Model'
                : tab === 'chains'
                  ? 'Chain'
                  : 'Provider'
            "
            :testing="testing"
            @test="testId"
          />
          <p v-else class="p-6 text-sm text-muted-foreground">
            No data matches this view.
          </p>
        </CardContent></Card
      >
    </template>
  </div>
</template>
