<script setup lang="ts">
import {
  Coins,
  Gauge,
  Hash,
  Layers,
  Loader2,
  MessageSquare,
  Play,
  Route,
  Server,
  Timer,
  TriangleAlert,
} from "@lucide/vue";
import type { Component } from "vue";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { ApiError, api } from "@/lib/api";
import { formatCost, formatLatency, formatNumber } from "@/lib/format";
import type { Alias, AliasChatTestResult } from "@/types/api";

const props = defineProps<{ open: boolean; alias: Alias | null }>();
const emit = defineEmits<{ "update:open": [boolean] }>();

const DEFAULT_PROMPT = "Reply with the single word: pong";

const model = ref("");
const prompt = ref("");
const running = ref(false);
const result = ref<AliasChatTestResult | null>(null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    model.value = props.alias?.model_override ?? "";
    prompt.value = DEFAULT_PROMPT;
    result.value = null;
  },
);

/** The request reference the resolver will see, e.g. `glm/glm-4.6`. */
const reference = computed(
  () => `${props.alias?.prefix ?? ""}/${model.value.trim() || "…"}`,
);

/** The prompt actually sent, mirroring the backend's default. */
const sentPrompt = computed(() => prompt.value.trim() || DEFAULT_PROMPT);

interface Stat {
  icon: Component;
  label: string;
  value: string;
  mono?: boolean;
}

const stats = computed<Stat[]>(() => {
  const response = result.value;
  if (!response?.ok) return [];
  const attemptCount = response.attempts ?? 1;
  return [
    { icon: Route, label: "Model", value: response.model ?? "—", mono: true },
    {
      icon: Server,
      label: "Source",
      value: response.source ?? "—",
      mono: true,
    },
    { icon: Layers, label: "Provider", value: response.provider_type ?? "—" },
    { icon: Gauge, label: "Tiers", value: `${attemptCount}` },
    {
      icon: Hash,
      label: "Tokens",
      value: `${formatNumber(response.prompt_tokens ?? 0)} in · ${formatNumber(
        response.completion_tokens ?? 0,
      )} out`,
    },
    { icon: Coins, label: "Cost", value: formatCost(response.cost_usd ?? 0) },
    {
      icon: Timer,
      label: "Latency",
      value: formatLatency(response.latency_ms ?? 0),
    },
  ];
});

async function run(): Promise<void> {
  if (!props.alias) return;
  running.value = true;
  result.value = null;
  try {
    result.value = await api.testAliasChat(props.alias.id, {
      prompt: prompt.value.trim() || undefined,
      model: model.value.trim() || undefined,
    });
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Chat test failed",
    );
  } finally {
    running.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent class="sm:max-w-2xl">
      <DialogHeader>
        <DialogTitle class="flex items-center gap-2">
          <MessageSquare class="size-4 text-muted-foreground" />
          Chat test
          <code class="rounded bg-muted px-1.5 py-0.5 text-xs font-normal"
            >{{ alias?.prefix }}/</code
          >
        </DialogTitle>
        <DialogDescription>
          Runs one real non-streaming completion through the resolver and
          executor, so it exercises the same path as a client request.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-4 rounded-lg border bg-muted/30 p-4">
          <div class="grid gap-2">
            <Label for="chat-test-model">Model segment</Label>
            <Input
              id="chat-test-model"
              v-model="model"
              placeholder="Uses the alias model override"
              autocapitalize="off"
              class="font-mono"
            />
            <p class="text-xs text-muted-foreground">
              Resolves as <code>{{ reference }}</code
              >.
            </p>
          </div>

          <div class="grid gap-2">
            <Label for="chat-test-prompt">Prompt</Label>
            <Textarea id="chat-test-prompt" v-model="prompt" rows="3" />
          </div>
        </div>

        <div
          v-if="running || result"
          class="grid gap-3 rounded-lg border p-4"
          aria-live="polite"
        >
          <div class="flex min-w-0 items-center gap-2">
            <Badge v-if="running" variant="secondary" class="shrink-0 gap-1">
              <Loader2 class="size-3 animate-spin" />
              Running
            </Badge>
            <Badge
              v-else-if="result"
              :variant="result.ok ? 'default' : 'destructive'"
              class="shrink-0 gap-1"
            >
              <TriangleAlert v-if="!result.ok" class="size-3" />
              {{ result.ok ? "Completed" : "Failed" }}
            </Badge>
            <span class="min-w-0 truncate text-sm text-muted-foreground">
              {{ running ? "Waiting for the provider…" : result?.message }}
            </span>
          </div>

          <div class="grid gap-2">
            <div class="flex justify-end">
              <div
                class="max-w-[85%] rounded-lg rounded-br-sm bg-primary px-3 py-2 text-sm whitespace-pre-wrap text-primary-foreground"
              >
                {{ sentPrompt }}
              </div>
            </div>

            <div v-if="running" class="flex justify-start">
              <div
                class="flex items-center gap-2 rounded-lg rounded-bl-sm bg-muted px-3 py-2 text-sm text-muted-foreground"
              >
                <Loader2 class="size-3.5 animate-spin" />
                Thinking…
              </div>
            </div>

            <div v-else-if="result" class="flex justify-start">
              <div
                class="max-w-[85%] rounded-lg rounded-bl-sm border bg-muted/40 px-3 py-2 text-sm whitespace-pre-wrap"
              >
                <template v-if="result.content">{{ result.content }}</template>
                <span v-else class="text-muted-foreground">
                  {{
                    result.ok
                      ? "The provider returned no content."
                      : result.message
                  }}
                </span>
              </div>
            </div>
          </div>

          <dl
            v-if="stats.length"
            class="grid grid-cols-2 gap-px overflow-hidden rounded-lg border bg-border sm:grid-cols-3"
          >
            <div
              v-for="stat in stats"
              :key="stat.label"
              class="flex min-w-0 flex-col gap-1 bg-card p-3"
            >
              <dt
                class="flex items-center gap-1.5 text-[11px] font-medium tracking-wide text-muted-foreground uppercase"
              >
                <component :is="stat.icon" class="size-3.5 shrink-0" />
                {{ stat.label }}
              </dt>
              <dd
                class="truncate font-medium"
                :class="stat.mono ? 'font-mono text-xs' : 'text-sm'"
                :title="stat.value"
              >
                {{ stat.value }}
              </dd>
            </div>
          </dl>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)"
          >Close</Button
        >
        <Button :disabled="running" @click="run">
          <Play :class="running ? 'animate-pulse' : ''" />
          {{ running ? "Running…" : "Run completion" }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
