<script setup lang="ts">
import {
  CircleStop,
  Route,
  Send,
  Sparkles,
  Trash2,
  TriangleAlert,
} from "@lucide/vue";
import { computed, onMounted, ref, watch } from "vue";
import { toast } from "vue-sonner";

import SaverToggles from "@/components/playground/SaverToggles.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Combobox,
  ComboboxAnchor,
  ComboboxEmpty,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxSeparator,
} from "@/components/ui/combobox";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { ApiError, api, streamPlaygroundChat } from "@/lib/api";
import { formatCost, formatNumber } from "@/lib/format";
import { buildModelSuggestions, type SaverToggle } from "@/lib/playground";
import type { Suggestion } from "@/lib/suggestions";
import type {
  ModelCatalogEntry,
  PlaygroundChatRouter,
  PlaygroundChatUsage,
  PlaygroundChatMessage,
  TokenSaverSettings,
} from "@/types/api";

/** One rendered turn. A streaming reply grows its assistant entry in place. */
interface Turn {
  role: "user" | "assistant";
  content: string;
  /** The model's reasoning, when the provider reports it separately. */
  thinking?: string;
  /** Routing diagnostics, attached to the assistant turn that reported them. */
  router?: PlaygroundChatRouter;
  usage?: PlaygroundChatUsage;
  error?: string;
}

const props = defineProps<{
  toggles: SaverToggle[];
  overrides: Partial<TokenSaverSettings>;
}>();

const emit = defineEmits<{
  toggle: [string];
  reset: [];
}>();

const catalog = ref<ModelCatalogEntry[]>([]);
const model = ref("");
const systemPrompt = ref("");
const input = ref("");
const turns = ref<Turn[]>([]);
const streaming = ref(false);
const error = ref<string | null>(null);

/**
 * Reka's combobox drives the search; clearing the pick after each selection
 * keeps the field free text, so a reference the catalog does not list can still
 * be typed by hand.
 */
const picked = ref<Suggestion | null>(null);

let controller: AbortController | null = null;

onMounted(async () => {
  try {
    const response = await api.modelCatalog();
    catalog.value = response.data;
    if (!model.value && catalog.value.length) model.value = catalog.value[0]!.id;
  } catch (caught) {
    // An empty catalog is not fatal: the field is free text, so a reference
    // can still be typed by hand.
    error.value =
      caught instanceof ApiError ? caught.message : "Could not load the catalog";
  }
});

const suggestions = computed(() => buildModelSuggestions(catalog.value));

const hasSuggestions = computed(
  () =>
    suggestions.value.aliases.length > 0 || suggestions.value.combos.length > 0,
);

watch(picked, (suggestion) => {
  if (!suggestion) return;
  model.value = suggestion.pattern;
  picked.value = null;
});

/** The input doubles as the value, so arbitrary references still work. */
function updateModel(value: unknown): void {
  model.value = typeof value === "string" ? value : "";
}

const canSend = computed(
  () => !streaming.value && !!model.value.trim() && !!input.value.trim(),
);

/**
 * Tokens the input savers measured, mirroring the token saver page: RTK and
 * Headroom shrink the prompt, while the directive savers only add wording.
 */
function savedTokens(usage: PlaygroundChatUsage): number {
  return usage.savings.saved_rtk_tokens + usage.savings.saved_headroom_tokens;
}

/** The conversation as the endpoint expects it, system prompt included. */
function wireMessages(): PlaygroundChatMessage[] {
  const messages: PlaygroundChatMessage[] = [];
  const system = systemPrompt.value.trim();
  if (system) messages.push({ role: "system", content: system });
  for (const turn of turns.value) {
    messages.push({ role: turn.role, content: turn.content });
  }
  return messages;
}

async function send(): Promise<void> {
  const text = input.value.trim();
  if (!text || !model.value.trim()) return;

  error.value = null;
  turns.value.push({ role: "user", content: text });
  input.value = "";

  // Built before the empty assistant turn is appended, so the placeholder is
  // never sent as part of the conversation.
  const messages = wireMessages();

  turns.value.push({ role: "assistant", content: "" });
  const target = turns.value[turns.value.length - 1]!;

  streaming.value = true;
  controller = new AbortController();
  try {
    await streamPlaygroundChat(
      {
        model: model.value.trim(),
        messages,
        overrides: Object.keys(props.overrides).length
          ? props.overrides
          : undefined,
      },
      {
        onRouter: (info) => {
          target.router = info;
        },
        onDelta: (chunk) => {
          target.content += chunk;
        },
        onThinking: (chunk) => {
          target.thinking = (target.thinking ?? "") + chunk;
        },
        onUsage: (usage) => {
          target.usage = usage;
        },
        onError: (message) => {
          target.error = message;
        },
      },
      controller.signal,
    );
  } catch (caught) {
    if (caught instanceof DOMException && caught.name === "AbortError") {
      // Stopping is a deliberate act, so the partial answer simply stays.
      if (!target.content && !target.error)
        turns.value = turns.value.filter((turn) => turn !== target);
    } else {
      const message =
        caught instanceof ApiError ? caught.message : "The request failed";
      target.error = message;
      toast.error(message);
    }
  } finally {
    streaming.value = false;
    controller = null;
  }
}

function stop(): void {
  controller?.abort();
}

function clear(): void {
  turns.value = [];
  error.value = null;
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <Card>
      <CardHeader>
        <CardTitle class="flex items-center gap-2">
          <Sparkles class="size-4" />
          Chat
        </CardTitle>
        <CardDescription>
          Streams a real completion through the resolver and executor, so the
          answer below is exactly what a client would receive — including which
          tier served it.
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4 lg:grid-cols-2">
        <div class="grid gap-2">
          <Label for="playground-chat-model">Model</Label>
          <Combobox
            v-model="picked"
            by="id"
            :reset-search-term-on-select="false"
            :reset-search-term-on-blur="false"
            open-on-focus
          >
            <ComboboxAnchor class="w-full">
              <ComboboxInput
                id="playground-chat-model"
                :model-value="model"
                placeholder="alias, combo, or prefix/model"
                class="h-9 w-full"
                @update:model-value="updateModel"
              />
            </ComboboxAnchor>

            <ComboboxList class="w-[var(--reka-combobox-trigger-width)]">
              <ComboboxEmpty
                >No alias or combo matches — free text is kept as-is</ComboboxEmpty
              >

              <ComboboxGroup v-if="suggestions.aliases.length">
                <div
                  class="px-2 py-1.5 text-xs font-medium text-muted-foreground"
                >
                  Aliases
                </div>
                <ComboboxItem
                  v-for="item in suggestions.aliases"
                  :key="item.id"
                  :value="item"
                  class="text-xs"
                >
                  <span class="truncate font-mono">{{ item.pattern }}</span>
                  <span
                    v-if="item.detail"
                    class="truncate text-muted-foreground"
                    >{{ item.detail }}</span
                  >
                </ComboboxItem>
              </ComboboxGroup>

              <ComboboxSeparator
                v-if="
                  suggestions.aliases.length && suggestions.combos.length
                "
              />

              <ComboboxGroup v-if="suggestions.combos.length">
                <div
                  class="px-2 py-1.5 text-xs font-medium text-muted-foreground"
                >
                  Combos
                </div>
                <ComboboxItem
                  v-for="item in suggestions.combos"
                  :key="item.id"
                  :value="item"
                  class="text-xs"
                >
                  <span class="truncate font-mono">{{ item.pattern }}</span>
                  <span
                    v-if="item.detail"
                    class="truncate text-muted-foreground"
                    >{{ item.detail }}</span
                  >
                </ComboboxItem>
              </ComboboxGroup>
            </ComboboxList>
          </Combobox>
          <p class="text-xs text-muted-foreground">
            Any reference the router can resolve. Pick from the catalog or type
            one that is not listed yet.
          </p>
        </div>
        <div class="grid gap-2">
          <Label for="playground-chat-system">System prompt (optional)</Label>
          <Textarea
            id="playground-chat-system"
            v-model="systemPrompt"
            class="min-h-20"
            placeholder="Kept at the head of every request in this conversation."
          />
        </div>
        <p v-if="error" class="text-xs text-destructive lg:col-span-2">
          {{ error }}
        </p>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Savers for this run</CardTitle>
        <CardDescription>
          The pipeline runs on every message you send, exactly as it would for a
          client.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <SaverToggles
          :toggles="toggles"
          :override-count="Object.keys(overrides).length"
          @toggle="emit('toggle', $event)"
          @reset="emit('reset')"
        />
      </CardContent>
    </Card>

    <Card class="flex min-h-80 flex-col">
      <CardHeader class="flex-row items-center justify-between gap-4">
        <div>
          <CardTitle>Transcript</CardTitle>
          <CardDescription>
            Recorded as usage, so these completions appear on the Usage page and
            in the Console feed.
          </CardDescription>
        </div>
        <Button
          variant="outline"
          size="sm"
          :disabled="!turns.length || streaming"
          @click="clear"
        >
          <Trash2 class="size-3.5" />
          Clear
        </Button>
      </CardHeader>
      <CardContent class="flex flex-1 flex-col gap-4">
        <div
          v-if="!turns.length"
          class="flex flex-1 items-center justify-center rounded-md border border-dashed p-8 text-center text-sm text-muted-foreground"
        >
          Send a message to stream a real completion.
        </div>

        <div v-else class="grid gap-4">
          <div
            v-for="(turn, index) in turns"
            :key="index"
            class="grid gap-2"
            :class="turn.role === 'user' ? 'justify-items-end' : ''"
          >
            <div
              v-if="turn.thinking"
              class="max-w-prose rounded-lg border border-dashed px-3 py-2 text-xs whitespace-pre-wrap text-muted-foreground"
            >
              {{ turn.thinking }}
            </div>

            <div
              class="max-w-prose rounded-lg border px-3 py-2 text-sm whitespace-pre-wrap"
              :class="
                turn.role === 'user'
                  ? 'bg-primary text-primary-foreground'
                  : 'bg-muted/40'
              "
            >
              <template v-if="turn.content">{{ turn.content }}</template>
              <span
                v-else-if="streaming && index === turns.length - 1"
                class="text-muted-foreground"
                >…</span
              >
            </div>

            <div
              v-if="turn.error"
              class="flex items-start gap-2 text-xs text-destructive"
            >
              <TriangleAlert class="mt-0.5 size-3.5 shrink-0" />
              {{ turn.error }}
            </div>

            <div
              v-if="turn.router || turn.usage"
              class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground"
            >
              <template v-if="turn.router">
                <Badge variant="secondary" class="gap-1">
                  <Route class="size-3" />
                  {{ turn.router.model }}
                </Badge>
                <span>{{ turn.router.source }}</span>
                <span aria-hidden="true">·</span>
                <span>{{ turn.router.provider_type }}</span>
                <span aria-hidden="true">·</span>
                <span>
                  {{ turn.router.attempts }} attempt{{
                    turn.router.attempts === 1 ? "" : "s"
                  }}
                </span>
              </template>
              <template v-if="turn.usage">
                <span aria-hidden="true">·</span>
                <span class="tabular-nums">
                  {{ formatNumber(turn.usage.prompt_tokens) }} in /
                  {{ formatNumber(turn.usage.completion_tokens) }} out
                </span>
                <span aria-hidden="true">·</span>
                <span class="tabular-nums">{{
                  formatCost(turn.usage.cost_usd)
                }}</span>
              </template>
            </div>

            <p
              v-if="turn.usage && savedTokens(turn.usage)"
              class="text-xs text-muted-foreground"
            >
              Pipeline removed {{ formatNumber(savedTokens(turn.usage)) }}
              prompt tokens
              <template v-if="turn.usage.savings.saved_cost_usd > 0">
                ({{ formatCost(turn.usage.savings.saved_cost_usd) }} saved)
              </template>
            </p>
          </div>
        </div>

        <div class="mt-auto grid gap-2 pt-2">
          <Label for="playground-chat-input">Message</Label>
          <Textarea
            id="playground-chat-input"
            v-model="input"
            class="min-h-20"
            placeholder="Type a message and press Send."
            @keydown.enter.exact.prevent="canSend && send()"
          />
          <div class="flex items-center gap-2">
            <Button :disabled="!canSend" @click="send">
              <Send class="size-3.5" />
              {{ streaming ? "Streaming…" : "Send" }}
            </Button>
            <Button v-if="streaming" variant="outline" @click="stop">
              <CircleStop class="size-3.5" />
              Stop
            </Button>
            <p class="text-xs text-muted-foreground">
              Enter sends; Shift+Enter adds a line.
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
