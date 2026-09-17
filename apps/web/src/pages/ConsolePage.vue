<script setup lang="ts">
import {
  Activity,
  ArrowDownToLine,
  Copy,
  Loader2,
  Pause,
  Play,
  RefreshCw,
  Search,
  SearchX,
  Terminal,
  TriangleAlert,
  X,
} from "@lucide/vue";
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { toast } from "vue-sonner";

import PageHeader from "@/components/PageHeader.vue";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ApiError, api } from "@/lib/api";
import { formatLatency } from "@/lib/format";
import type { ActivityEvent, ActivitySnapshot } from "@/types/api";

type LevelFilter = "all" | ActivityEvent["level"];

const LEVELS: { value: LevelFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "info", label: "Info" },
  { value: "warn", label: "Warn" },
  { value: "error", label: "Error" },
];

const EVENT_LIMIT = 300;
const POLL_MS = 1500;

const snapshot = ref<ActivitySnapshot | null>(null);
const loaded = ref(false);
const paused = ref(false);
const follow = ref(true);
const level = ref<LevelFilter>("all");
const search = ref("");
const error = ref<string | null>(null);
/** A background poll failed; the feed on screen is the last good snapshot. */
const staleError = ref<string | null>(null);
const updatedAt = ref<Date | null>(null);
const expanded = ref<Set<number>>(new Set());
const feed = ref<HTMLElement | null>(null);
let timer: number | undefined;

const allEvents = computed<ActivityEvent[]>(() => snapshot.value?.events ?? []);

const counts = computed<Record<LevelFilter, number>>(() => {
  const totals: Record<LevelFilter, number> = {
    all: allEvents.value.length,
    info: 0,
    warn: 0,
    error: 0,
  };
  for (const event of allEvents.value) totals[event.level] += 1;
  return totals;
});

const events = computed<ActivityEvent[]>(() => {
  const term = search.value.trim().toLowerCase();
  return allEvents.value.filter((event) => {
    if (level.value !== "all" && event.level !== level.value) return false;
    if (!term) return true;
    return [event.kind, event.connection, event.model, event.message].some(
      (value) => value?.toLowerCase().includes(term),
    );
  });
});

const filtering = computed(
  () => level.value !== "all" || search.value.trim() !== "",
);

const eventSummary = computed(() =>
  filtering.value
    ? `${events.value.length} of ${counts.value.all} events`
    : `${counts.value.all} events`,
);

const inFlight = computed(() => snapshot.value?.active ?? []);

const updatedLabel = computed(() =>
  updatedAt.value
    ? updatedAt.value.toLocaleTimeString(undefined, { hour12: false })
    : "—",
);

const statusLabel = computed(() => {
  if (staleError.value) return "Refresh failed";
  if (paused.value) return "Paused";
  return `Live · ${updatedLabel.value}`;
});

async function refresh(): Promise<void> {
  try {
    snapshot.value = await api.activity(EVENT_LIMIT);
    updatedAt.value = new Date();
    staleError.value = null;
    error.value = null;
  } catch (caught) {
    const message =
      caught instanceof ApiError ? caught.message : "Failed to load activity";
    // A failed poll keeps the last snapshot on screen and flags the failure in
    // the status pill, rather than blanking the feed it could not refresh.
    if (snapshot.value) staleError.value = message;
    else error.value = message;
  } finally {
    loaded.value = true;
  }

  // Polling never runs while paused, so this only follows on a manual refresh.
  if (follow.value) await scrollToLatest();
}

async function scrollToLatest(): Promise<void> {
  await nextTick();
  const element = feed.value;
  if (element) element.scrollTop = element.scrollHeight;
}

function stop(): void {
  if (timer !== undefined) {
    window.clearInterval(timer);
    timer = undefined;
  }
}

/** Polls on a timer, but never while the tab is hidden. */
function tick(): void {
  if (document.visibilityState !== "visible") return;
  void refresh();
}

function start(): void {
  stop();
  void refresh();
  timer = window.setInterval(tick, POLL_MS);
}

function togglePause(): void {
  paused.value = !paused.value;
  if (paused.value) stop();
  else start();
}

function toggleFollow(): void {
  follow.value = !follow.value;
  if (follow.value) void scrollToLatest();
}

function onVisibilityChange(): void {
  if (document.visibilityState !== "visible") return;
  void refresh();
}

/** Following is implied by scrolling back to the live edge. */
function onScroll(): void {
  const element = feed.value;
  if (!element) return;
  const atBottom =
    element.scrollHeight - element.scrollTop - element.clientHeight < 24;
  if (atBottom !== follow.value) follow.value = atBottom;
}

/** Clicking the active level would deselect every toggle; keep it instead. */
function pickLevel(value: unknown): void {
  if (LEVELS.some((entry) => entry.value === value))
    level.value = value as LevelFilter;
}

function countFor(value: LevelFilter): number {
  return counts.value[value];
}

function isExpanded(seq: number): boolean {
  return expanded.value.has(seq);
}

function toggleExpanded(seq: number): void {
  const next = new Set(expanded.value);
  if (next.has(seq)) next.delete(seq);
  else next.add(seq);
  expanded.value = next;
}

function clearFilters(): void {
  level.value = "all";
  search.value = "";
}

/** In-flight elapsed time; sub-second attempts would otherwise read as "—". */
function elapsedLabel(ms: number): string {
  return ms < 1000 ? `${Math.round(ms)} ms` : formatLatency(ms);
}

/** Time to the millisecond: consecutive events can land in the same second. */
function timeLabel(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const base = date.toLocaleTimeString(undefined, { hour12: false });
  return `${base}.${String(date.getMilliseconds()).padStart(3, "0")}`;
}

function levelTone(level: string): string {
  if (level === "error") return "bg-red-500/15 text-red-400";
  if (level === "warn") return "bg-amber-500/15 text-amber-400";
  return "bg-sky-500/15 text-sky-400";
}

function gutterClass(level: string): string {
  if (level === "error") return "border-l-2 border-l-red-500/70";
  if (level === "warn") return "border-l-2 border-l-amber-500/70";
  return "border-l-2 border-l-transparent";
}

function statusClass(status: number): string {
  if (status >= 500) return "text-red-400";
  if (status >= 400) return "text-amber-400";
  return "text-emerald-400";
}

/** One event as a single line, for pasting into a bug report. */
async function copyEvent(event: ActivityEvent): Promise<void> {
  const line = [
    timeLabel(event.at),
    event.level.toUpperCase(),
    event.kind,
    event.connection,
    event.model,
    event.message,
    event.latency_ms != null ? `${event.latency_ms}ms` : null,
    event.status,
  ]
    .filter((part) => part !== null && part !== undefined && part !== "")
    .join(" ");
  try {
    await navigator.clipboard.writeText(line);
    toast.success(`Copied event #${event.seq}`);
  } catch {
    toast.error("Clipboard is not available");
  }
}

onMounted(() => {
  start();
  document.addEventListener("visibilitychange", onVisibilityChange);
});
onUnmounted(() => {
  document.removeEventListener("visibilitychange", onVisibilityChange);
  stop();
});
</script>

<template>
  <div class="flex min-h-0 flex-col gap-6 md:h-[calc(100svh-9rem)]">
    <PageHeader
      title="Console"
      description="Live API transfer log: requests, upstream attempts, keys, and limits."
    >
      <template #actions>
        <span class="flex items-center gap-2 text-xs text-muted-foreground">
          <span class="relative flex size-2">
            <span
              v-if="!paused && !staleError"
              class="absolute inline-flex size-full animate-ping rounded-full bg-emerald-500 opacity-75"
            />
            <span
              class="relative inline-flex size-2 rounded-full"
              :class="
                staleError
                  ? 'bg-destructive'
                  : paused
                    ? 'bg-muted-foreground/40'
                    : 'bg-emerald-500'
              "
            />
          </span>
          {{ statusLabel }}
        </span>
        <Button variant="outline" size="sm" @click="togglePause">
          <Play v-if="paused" />
          <Pause v-else />
          {{ paused ? "Resume" : "Pause" }}
        </Button>
        <Button
          variant="outline"
          size="sm"
          :aria-pressed="follow"
          title="Keep the newest event in view"
          @click="toggleFollow"
        >
          <ArrowDownToLine :class="follow ? 'text-primary' : ''" /> Follow
        </Button>
        <Button variant="outline" size="sm" @click="refresh()">
          <RefreshCw /> Refresh
        </Button>
      </template>
    </PageHeader>

    <Card v-if="staleError" class="border-destructive/40">
      <CardContent
        class="flex flex-wrap items-center gap-2 p-4 text-sm text-destructive"
      >
        <TriangleAlert class="size-4 shrink-0" />
        <span
          >Showing the last successful refresh ({{ updatedLabel }}).
          {{ staleError }}</span
        >
        <Button variant="outline" size="sm" class="ml-auto" @click="refresh()"
          >Retry</Button
        >
      </CardContent>
    </Card>

    <Card v-if="error && !snapshot">
      <CardContent
        class="flex flex-wrap items-center gap-2 p-6 text-sm text-destructive"
      >
        <TriangleAlert class="size-4 shrink-0" />
        <span>{{ error }}</span>
        <Button variant="outline" size="sm" class="ml-auto" @click="start()"
          >Retry</Button
        >
      </CardContent>
    </Card>

    <Card v-else class="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div class="flex flex-wrap items-center gap-2 border-b px-3 py-2">
        <Terminal class="size-3.5 shrink-0 text-muted-foreground" />
        <span class="text-xs text-muted-foreground">{{ eventSummary }}</span>

        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          :model-value="level"
          aria-label="Filter by level"
          class="ml-1"
          @update:model-value="pickLevel"
        >
          <ToggleGroupItem
            v-for="item in LEVELS"
            :key="item.value"
            :value="item.value"
            :aria-label="`Show ${item.label.toLowerCase()} events`"
          >
            {{ item.label }}
            <span
              v-if="countFor(item.value)"
              class="text-muted-foreground tabular-nums"
            >
              {{ countFor(item.value) }}
            </span>
          </ToggleGroupItem>
        </ToggleGroup>

        <div class="relative ml-auto w-full sm:w-56">
          <Search
            class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
          />
          <Input
            v-model="search"
            class="h-8 pl-8 text-xs"
            placeholder="Search kind, connection, model, message…"
            aria-label="Search events"
          />
          <button
            v-if="search"
            type="button"
            class="absolute top-1/2 right-2 -translate-y-1/2 text-muted-foreground transition-colors hover:text-foreground"
            aria-label="Clear search"
            @click="search = ''"
          >
            <X class="size-3.5" />
          </button>
        </div>
      </div>

      <!-- The feed keeps a dark surface in both themes: it reads as a terminal
           log rather than a table, which is what this page is for. -->
      <div class="flex min-h-0 flex-1 flex-col bg-zinc-950">
        <div
          v-if="inFlight.length"
          class="flex flex-wrap items-center gap-2 border-b border-zinc-800 px-3 py-2"
        >
          <span
            class="flex items-center gap-1.5 text-xs font-medium text-emerald-400"
          >
            <Loader2 class="size-3 animate-spin" />
            {{ inFlight.length }} in flight
          </span>
          <span
            v-for="(attempt, index) in inFlight"
            :key="`${attempt.connection_id}-${attempt.model}-${attempt.tier}-${index}`"
            class="flex items-center gap-1.5 rounded-md bg-zinc-900 px-2 py-0.5 font-mono text-[11px] text-zinc-300"
            :title="`tier ${attempt.tier} · ${attempt.source}`"
          >
            <span class="truncate">{{ attempt.connection }}</span>
            <span class="text-zinc-500">/</span>
            <span class="truncate text-zinc-400">{{ attempt.model }}</span>
            <span class="text-zinc-500 tabular-nums">{{
              elapsedLabel(attempt.elapsed_ms)
            }}</span>
          </span>
        </div>

        <div
          ref="feed"
          class="min-h-0 flex-1 overflow-y-auto font-mono text-xs"
          role="log"
          aria-label="Activity events"
          :aria-busy="!loaded"
          @scroll.passive="onScroll"
        >
          <template v-if="!loaded">
            <div
              v-for="row in 10"
              :key="row"
              class="flex items-center gap-2 border-b border-zinc-900 px-3 py-1.5"
            >
              <span class="h-3 w-20 animate-pulse rounded bg-zinc-800" />
              <span class="h-3 w-10 animate-pulse rounded bg-zinc-800" />
              <span class="h-3 flex-1 animate-pulse rounded bg-zinc-800" />
            </div>
          </template>

          <div
            v-else-if="!events.length"
            class="flex h-full flex-col items-center justify-center gap-2 px-6 py-10 text-center"
          >
            <span
              class="flex size-10 items-center justify-center rounded-full bg-zinc-900 text-zinc-500"
            >
              <SearchX v-if="filtering" class="size-5" />
              <Activity v-else class="size-5" />
            </span>
            <p class="font-sans text-sm text-zinc-300">
              {{
                filtering ? "No events match this filter" : "No activity yet"
              }}
            </p>
            <p class="max-w-sm font-sans text-xs text-zinc-500">
              <template v-if="filtering">
                {{ counts.all }} events are buffered, none of them match the
                current level or search.
              </template>
              <template v-else>
                Events appear here as requests flow through /v1/*. The buffer is
                cleared when the router restarts.
              </template>
            </p>
            <button
              v-if="filtering"
              type="button"
              class="underline underline-offset-4 transition-colors hover:text-zinc-100"
              @click="clearFilters"
            >
              Clear filters
            </button>
          </div>

          <template v-else>
            <div
              v-for="event in events"
              :key="event.seq"
              class="border-b border-zinc-900 last:border-b-0"
              :class="gutterClass(event.level)"
            >
              <button
                type="button"
                class="flex w-full items-start gap-2 px-2 py-1.5 text-left transition-colors hover:bg-zinc-900/60 focus-visible:bg-zinc-900/60 focus-visible:outline-none"
                :aria-expanded="isExpanded(event.seq)"
                @click="toggleExpanded(event.seq)"
              >
                <span class="shrink-0 text-zinc-500 tabular-nums">{{
                  timeLabel(event.at)
                }}</span>
                <span
                  class="w-12 shrink-0 rounded px-1 text-center text-[10px] leading-5 font-semibold uppercase"
                  :class="levelTone(event.level)"
                >
                  {{ event.level }}
                </span>
                <span
                  class="hidden w-32 shrink-0 truncate text-zinc-400 sm:block"
                  :title="event.kind"
                  >{{ event.kind }}</span
                >
                <span
                  class="hidden w-24 shrink-0 truncate text-zinc-400 lg:block"
                  :title="event.connection ?? ''"
                  >{{ event.connection ?? "—" }}</span
                >
                <span
                  class="hidden w-28 shrink-0 truncate text-zinc-500 xl:block"
                  :title="event.model ?? ''"
                  >{{ event.model ?? "—" }}</span
                >
                <span class="min-w-0 flex-1 break-words text-zinc-100">{{
                  event.message
                }}</span>
                <span
                  v-if="event.latency_ms != null"
                  class="shrink-0 text-zinc-500 tabular-nums"
                  >{{ event.latency_ms }}ms</span
                >
                <span
                  v-if="event.status"
                  class="w-9 shrink-0 text-right tabular-nums"
                  :class="statusClass(event.status)"
                  >{{ event.status }}</span
                >
              </button>

              <div
                v-if="isExpanded(event.seq)"
                class="grid gap-2 border-t border-zinc-900 bg-zinc-900/40 px-3 py-2"
              >
                <p class="break-words whitespace-pre-wrap text-zinc-200">
                  {{ event.message }}
                </p>
                <div
                  class="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-zinc-500"
                >
                  <span>seq {{ event.seq }}</span>
                  <span>{{ event.kind }}</span>
                  <span v-if="event.connection"
                    >connection {{ event.connection }}</span
                  >
                  <span v-if="event.model">model {{ event.model }}</span>
                  <span v-if="event.latency_ms != null"
                    >{{ event.latency_ms }}ms</span
                  >
                  <span v-if="event.status">status {{ event.status }}</span>
                  <Button
                    variant="ghost"
                    size="xs"
                    class="ml-auto text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100"
                    @click="copyEvent(event)"
                  >
                    <Copy /> Copy line
                  </Button>
                </div>
              </div>
            </div>
          </template>
        </div>
      </div>
    </Card>
  </div>
</template>
