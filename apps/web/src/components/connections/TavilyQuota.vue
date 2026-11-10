<script setup lang="ts">
import { RefreshCw } from "@lucide/vue";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Button } from "@/components/ui/button";
import { api, ApiError } from "@/lib/api";
import { formatNumber } from "@/lib/format";
import type { TavilyQuotaSnapshot } from "@/types/api";

const props = defineProps<{ connectionId: string }>();
const quota = ref<TavilyQuotaSnapshot | null>(null);
const error = ref<string | null>(null);
const loading = ref(false);
const updated = ref<Date | null>(null);
let timer: ReturnType<typeof setInterval> | undefined;
let stopped = false;
const windows = computed(() => {
  if (!quota.value) return [];
  const { key, account } = quota.value;
  const rows = [
    {
      label: "API key",
      used: key.usage,
      limit: key.limit,
      unlimited: key.limit === null,
    },
    {
      label: "Plan",
      used: account.plan_usage,
      limit: account.plan_limit,
      unlimited: false,
    },
  ];
  if (
    account.paygo_usage != null &&
    (account.paygo_usage > 0 || (account.paygo_limit ?? 0) > 0)
  )
    rows.push({
      label: "Pay as you go",
      used: account.paygo_usage,
      limit: account.paygo_limit,
      unlimited: false,
    });
  return rows;
});

async function refresh(): Promise<void> {
  if (loading.value) return;
  loading.value = true;
  try {
    const result = await api.connectionQuota(props.connectionId);
    if (!stopped) {
      quota.value = result;
      error.value = null;
      updated.value = new Date();
    }
  } catch (caught) {
    if (!stopped)
      error.value =
        caught instanceof ApiError
          ? caught.message
          : "Could not load Tavily quota";
  } finally {
    if (!stopped) loading.value = false;
  }
}
onMounted(() => {
  void refresh();
  timer = setInterval(() => void refresh(), 60_000);
});
onBeforeUnmount(() => {
  stopped = true;
  if (timer) clearInterval(timer);
});
</script>

<template>
  <section aria-label="Tavily quota" class="min-w-0 space-y-2 text-xs">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="font-medium">
        Tavily quota<span
          v-if="quota?.account.current_plan"
          class="text-muted-foreground"
        >
          · {{ quota.account.current_plan }}</span
        >
      </p>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        class="size-7 shrink-0"
        aria-label="Refresh Tavily quota"
        title="Refresh quota (updates every minute)"
        :disabled="loading"
        @click="refresh"
      >
        <RefreshCw class="size-3.5" />
      </Button>
    </div>
    <p v-if="loading && !quota" class="text-muted-foreground">Loading quota…</p>
    <p v-if="error" role="alert" class="break-words text-destructive">
      {{ error }}<span v-if="quota"> · Showing last successful reading</span>
    </p>
    <div v-for="window in windows" :key="window.label" class="grid gap-1">
      <div class="flex flex-wrap justify-between gap-x-2">
        <span>{{ window.label }}</span>
        <span class="tabular-nums"
          >{{ formatNumber(window.used) }} /
          {{
            window.limit == null
              ? window.unlimited
                ? "Unlimited"
                : "Limit unavailable"
              : formatNumber(window.limit)
          }}
          credits</span
        >
      </div>
      <progress
        v-if="window.limit != null && window.limit > 0"
        class="h-1.5 w-full accent-primary"
        :max="window.limit"
        :value="Math.min(window.used, window.limit)"
        :aria-label="`${window.label}: ${window.used} of ${window.limit} credits used`"
      />
    </div>
    <p v-if="updated" class="text-muted-foreground">
      Updated
      {{
        updated.toLocaleTimeString(undefined, {
          hour: "2-digit",
          minute: "2-digit",
        })
      }}
    </p>
  </section>
</template>
