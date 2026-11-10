<script setup lang="ts">
import { RefreshCw } from "@lucide/vue";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { Button } from "@/components/ui/button";
import { ApiError, api } from "@/lib/api";
import type {
  OAuthAccount,
  OAuthQuotaSnapshot,
  OAuthQuotaWindow,
} from "@/types/api";

const props = defineProps<{ connectionId: string }>();
type Entry = {
  account: OAuthAccount;
  quota: OAuthQuotaSnapshot | null;
  error: string | null;
  updated: Date | null;
};
const entries = ref<Entry[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
let timer: ReturnType<typeof setInterval> | undefined;
let stopped = false;

async function refresh(): Promise<void> {
  if (loading.value) return;
  loading.value = true;
  try {
    const accounts = (await api.listOAuthAccounts(props.connectionId)).filter(
      (account) =>
        account.provider_key === "openai-codex" && account.enabled !== 0,
    );
    const results = await Promise.all(
      accounts.map(async (account): Promise<Entry> => {
        const previous = entries.value.find(
          (entry) => entry.account.id === account.id,
        );
        try {
          const quota = await api.oauthAccountQuota(
            props.connectionId,
            account.id,
          );
          return { account, quota, error: null, updated: new Date() };
        } catch (caught) {
          return {
            account,
            quota: previous?.quota ?? null,
            updated: previous?.updated ?? null,
            error:
              caught instanceof ApiError
                ? caught.message
                : "Could not load quota",
          };
        }
      }),
    );
    if (!stopped) {
      entries.value = results;
      error.value = null;
    }
  } catch (caught) {
    if (!stopped)
      error.value =
        caught instanceof ApiError ? caught.message : "Could not load accounts";
  } finally {
    if (!stopped) loading.value = false;
  }
}

function duration(seconds: number | null): string {
  if (!seconds) return "Usage window";
  if (seconds % 86400 === 0) return `${seconds / 86400}-day window`;
  if (seconds % 3600 === 0) return `${seconds / 3600}-hour window`;
  return `${Math.round(seconds / 60)}-minute window`;
}
function windows(limits: OAuthQuotaSnapshot["rate_limit"]): OAuthQuotaWindow[] {
  return [limits?.primary_window, limits?.secondary_window].filter(
    (window): window is OAuthQuotaWindow => window != null,
  );
}
function resetTime(timestamp: number | null): string {
  return timestamp == null
    ? "Reset time unavailable"
    : `Resets ${new Date(timestamp * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}`;
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
  <section aria-label="Codex quota" class="min-w-0 space-y-2">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="text-xs font-medium">Codex quota</p>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        class="size-7 shrink-0"
        aria-label="Refresh quota"
        :title="
          loading ? 'Refreshing quota…' : 'Refresh quota (updates every minute)'
        "
        :disabled="loading"
        @click="refresh"
      >
        <RefreshCw class="size-3.5" />
      </Button>
    </div>
    <p v-if="loading && !entries.length" class="text-xs text-muted-foreground">
      Loading quota…
    </p>
    <p v-else-if="error" role="alert" class="text-xs text-destructive">
      {{ error }}
    </p>
    <p v-else-if="!entries.length" class="text-xs text-muted-foreground">
      No active Codex accounts.
    </p>
    <div
      v-for="entry in entries"
      :key="entry.account.id"
      class="min-w-0 space-y-2 text-xs"
    >
      <p class="truncate font-medium">
        {{ entry.account.label
        }}<span v-if="entry.quota?.plan_type">
          · {{ entry.quota.plan_type }}</span
        >
      </p>
      <p v-if="entry.error" role="alert" class="break-words text-destructive">
        {{ entry.error
        }}<span v-if="entry.updated"> · Showing last successful reading</span>
      </p>
      <template v-if="entry.quota">
        <div class="grid grid-cols-2 gap-3">
          <div
            v-for="(window, index) in windows(entry.quota.rate_limit)"
            :key="`main-${index}`"
            class="grid gap-1"
          >
            <div class="flex flex-wrap justify-between gap-x-2">
              <span>{{ duration(window.limit_window_seconds) }}</span
              ><span>{{ window.used_percent }}% used</span>
            </div>
            <progress
              class="h-1.5 w-full accent-primary"
              max="100"
              :value="window.used_percent"
              :aria-label="`${duration(window.limit_window_seconds)}: ${window.used_percent}% used`"
            />
            <p
              class="text-[11px] leading-snug text-muted-foreground"
              :title="
                window.reset_at
                  ? new Date(window.reset_at * 1000).toLocaleString()
                  : undefined
              "
            >
              {{ resetTime(window.reset_at) }}
            </p>
          </div>
        </div>
        <div
          v-for="(limit, index) in entry.quota.additional_rate_limits"
          :key="index"
          class="grid gap-1"
        >
          <p class="font-medium">
            {{
              limit.limit_name ?? limit.metered_feature ?? "Additional limit"
            }}
          </p>
          <div
            v-for="(window, windowIndex) in windows(limit.rate_limit)"
            :key="windowIndex"
            class="grid gap-1"
          >
            <div class="flex flex-wrap justify-between gap-x-2">
              <span>{{ duration(window.limit_window_seconds) }}</span
              ><span>{{ window.used_percent }}% used</span>
            </div>
            <progress
              class="h-1.5 w-full accent-primary"
              max="100"
              :value="window.used_percent"
              :aria-label="`${limit.limit_name ?? 'Additional limit'}: ${window.used_percent}% used`"
            />
            <p class="text-muted-foreground">
              {{ resetTime(window.reset_at) }}
            </p>
          </div>
        </div>
        <p
          v-if="
            !windows(entry.quota.rate_limit).length &&
            !entry.quota.additional_rate_limits.length
          "
          class="text-muted-foreground"
        >
          No usage windows returned.
        </p>
        <p v-if="entry.updated" class="text-muted-foreground">
          Updated
          {{
            entry.updated.toLocaleTimeString(undefined, {
              hour: "2-digit",
              minute: "2-digit",
            })
          }}
        </p>
      </template>
    </div>
  </section>
</template>
