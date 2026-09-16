<script setup lang="ts">
import { ArrowDownToLine, Loader2, Pause, Play, Terminal } from "@lucide/vue";
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";

import PageHeader from "@/components/PageHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ApiError, api } from "@/lib/api";
import type { ActivityEvent, ActivitySnapshot } from "@/types/api";

const LEVELS = [
  { value: "all", label: "All levels" },
  { value: "info", label: "Info" },
  { value: "warn", label: "Warnings" },
  { value: "error", label: "Errors" },
];

const snapshot = ref<ActivitySnapshot | null>(null);
const paused = ref(false);
const follow = ref(true);
const level = ref("all");
const error = ref<string | null>(null);
const feed = ref<HTMLElement | null>(null);
let timer: number | undefined;

const events = computed<ActivityEvent[]>(() => {
  const all = snapshot.value?.events ?? [];
  return level.value === "all"
    ? all
    : all.filter((event) => event.level === level.value);
});

const inFlight = computed(() => snapshot.value?.active.length ?? 0);

async function refresh(): Promise<void> {
  try {
    snapshot.value = await api.activity(300);
    error.value = null;
  } catch (caught) {
    error.value =
      caught instanceof ApiError ? caught.message : "Failed to load activity";
  }

  if (follow.value && !paused.value) {
    await nextTick();
    feed.value?.scrollTo({ top: feed.value.scrollHeight });
  }
}

function stop(): void {
  if (timer !== undefined) {
    window.clearInterval(timer);
    timer = undefined;
  }
}

function start(): void {
  stop();
  void refresh();
  timer = window.setInterval(refresh, 1500);
}

function togglePause(): void {
  paused.value = !paused.value;
  if (paused.value) stop();
  else start();
}

function timeOnly(value: string): string {
  return new Date(value).toLocaleTimeString(undefined, { hour12: false });
}

function levelClass(level: string): string {
  if (level === "error") return "text-red-400";
  if (level === "warn") return "text-amber-400";
  return "text-sky-400";
}

onMounted(start);
onUnmounted(stop);
</script>

<template>
  <div class="flex min-h-0 flex-col gap-6 md:h-[calc(100svh-9rem)]">
    <PageHeader
      title="Console"
      description="Live API transfer log: requests, upstream attempts, keys, and limits."
    >
      <template #actions>
        <Badge v-if="inFlight" variant="secondary" class="gap-1">
          <Loader2 class="size-3 animate-spin" /> {{ inFlight }} in flight
        </Badge>
        <Select v-model="level">
          <SelectTrigger class="w-36">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="item in LEVELS"
              :key="item.value"
              :value="item.value"
            >
              {{ item.label }}
            </SelectItem>
          </SelectContent>
        </Select>
        <Button variant="outline" size="sm" @click="follow = !follow">
          <ArrowDownToLine :class="follow ? 'text-primary' : ''" /> Follow
        </Button>
        <Button variant="outline" size="sm" @click="togglePause">
          <Play v-if="paused" />
          <Pause v-else />
          {{ paused ? "Resume" : "Pause" }}
        </Button>
      </template>
    </PageHeader>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{
        error
      }}</CardContent>
    </Card>

    <Card class="flex min-h-0 flex-1 flex-col overflow-hidden">
      <CardContent class="flex min-h-0 flex-1 flex-col gap-0 p-0">
        <div
          class="flex items-center gap-2 border-b px-3 py-2 text-xs text-muted-foreground"
        >
          <Terminal class="size-3.5" />
          <span>{{ events.length }} events</span>
          <span v-if="paused" class="text-amber-500">paused</span>
        </div>

        <div
          ref="feed"
          class="min-h-0 flex-1 overflow-y-auto bg-zinc-950 p-3 font-mono text-xs text-zinc-200"
        >
          <p v-if="!events.length" class="text-zinc-500">
            No events match this filter.
          </p>
          <div
            v-for="event in events"
            :key="event.seq"
            class="flex items-start gap-2 border-b border-zinc-900 py-1 last:border-b-0"
          >
            <span class="shrink-0 text-zinc-500">{{ timeOnly(event.at) }}</span>
            <span
              class="w-10 shrink-0 uppercase"
              :class="levelClass(event.level)"
            >
              {{ event.level }}
            </span>
            <span
              class="w-36 shrink-0 truncate text-zinc-400"
              :title="event.kind"
            >
              {{ event.kind }}
            </span>
            <span
              class="w-28 shrink-0 truncate text-zinc-500"
              :title="event.connection ?? ''"
            >
              {{ event.connection ?? "—" }}
            </span>
            <span class="min-w-0 flex-1 break-words">{{ event.message }}</span>
            <span
              v-if="event.latency_ms != null"
              class="shrink-0 text-zinc-500"
            >
              {{ event.latency_ms }}ms
            </span>
            <span
              v-if="event.status"
              class="w-8 shrink-0 text-right"
              :class="
                event.status >= 400 ? 'text-amber-400' : 'text-emerald-400'
              "
            >
              {{ event.status }}
            </span>
          </div>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
