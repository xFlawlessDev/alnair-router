<script setup lang="ts">
import { Play } from "@lucide/vue";
import { ref, watch } from "vue";
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

const model = ref("");
const prompt = ref("");
const running = ref(false);
const result = ref<AliasChatTestResult | null>(null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    model.value = props.alias?.model_override ?? "";
    prompt.value = "Reply with the single word: pong";
    result.value = null;
  },
);

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
        <DialogTitle>Chat test — {{ alias?.prefix }}/</DialogTitle>
        <DialogDescription>
          Runs one real non-streaming completion through the resolver and
          executor, so it exercises the same path as a client request.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="chat-test-model">Model segment</Label>
          <Input
            id="chat-test-model"
            v-model="model"
            placeholder="Uses the alias model override"
            autocapitalize="off"
          />
          <p class="text-xs text-muted-foreground">
            The request reference becomes
            <code>{{ alias?.prefix }}/{{ model.trim() || "…" }}</code
            >.
          </p>
        </div>

        <div class="grid gap-2">
          <Label for="chat-test-prompt">Prompt</Label>
          <Textarea id="chat-test-prompt" v-model="prompt" rows="3" />
        </div>

        <div v-if="result" class="grid gap-3 rounded-md border p-4">
          <div class="flex items-center gap-2">
            <Badge :variant="result.ok ? 'default' : 'destructive'">
              {{ result.ok ? "Completed" : "Failed" }}
            </Badge>
            <span class="text-sm text-muted-foreground">{{
              result.message
            }}</span>
          </div>

          <pre
            v-if="result.content"
            class="max-h-48 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap"
            >{{ result.content }}</pre>

          <dl
            v-if="result.ok"
            class="grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-muted-foreground sm:grid-cols-3"
          >
            <div>
              <dt class="inline font-medium">Source</dt>
              <dd class="inline">{{ result.source }}</dd>
            </div>
            <div>
              <dt class="inline font-medium">Model</dt>
              <dd class="inline">{{ result.model }}</dd>
            </div>
            <div>
              <dt class="inline font-medium">Provider</dt>
              <dd class="inline">{{ result.provider_type }}</dd>
            </div>
            <div>
              <dt class="inline font-medium">Tiers</dt>
              <dd class="inline">{{ result.attempts }}</dd>
            </div>
            <div>
              <dt class="inline font-medium">Tokens</dt>
              <dd class="inline">
                {{ formatNumber(result.prompt_tokens ?? 0) }} /
                {{ formatNumber(result.completion_tokens ?? 0) }}
              </dd>
            </div>
            <div>
              <dt class="inline font-medium">Cost</dt>
              <dd class="inline">{{ formatCost(result.cost_usd ?? 0) }}</dd>
            </div>
            <div>
              <dt class="inline font-medium">Latency</dt>
              <dd class="inline">
                {{ formatLatency(result.latency_ms ?? 0) }}
              </dd>
            </div>
            <div v-if="result.finish_reason">
              <dt class="inline font-medium">Finish</dt>
              <dd class="inline">{{ result.finish_reason }}</dd>
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
