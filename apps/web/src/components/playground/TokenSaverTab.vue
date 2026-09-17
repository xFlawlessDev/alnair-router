<script setup lang="ts">
import {
  ArrowRight,
  CircleAlert,
  CircleCheck,
  FlaskConical,
  Info,
  Play,
} from "@lucide/vue";
import { computed, ref } from "vue";
import { RouterLink } from "vue-router";

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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { ApiError, api } from "@/lib/api";
import { formatCost, formatNumber } from "@/lib/format";
import type { SaverToggle } from "@/lib/playground";
import type {
  PlaygroundMessage,
  PlaygroundResult,
  SavingsTotals,
  TokenSaverSettings,
} from "@/types/api";

/**
 * A sample request, so the playground is useful without typing anything. Each
 * one targets a different behaviour: the three tool results should compress,
 * while the prose one proves the slimmer leaves ordinary language alone.
 */
interface Preset {
  id: string;
  label: string;
  /** What the sample is meant to demonstrate. */
  expectation: string;
  messages: PlaygroundMessage[];
}

function lines(count: number, build: (index: number) => string): string {
  return Array.from({ length: count }, (_, index) => build(index)).join("\n");
}

const DIFF = [
  "diff --git a/src/router.ts b/src/router.ts",
  "index 1a2b3c4..5d6e7f8 100644",
  "--- a/src/router.ts",
  "+++ b/src/router.ts",
  "@@ -12,9 +12,14 @@ export function resolve(request: Request) {",
  ...Array.from({ length: 180 }, (_, index) =>
    index % 7 === 0
      ? `+  const step${index} = compute(${index});`
      : `+  // padding line ${index}`,
  ),
  "@@ -40,6 +45,7 @@ export function dispatch(route: Route) {",
  "+  return route.handler(request);",
  " }",
].join("\n");

const GREP = lines(
  160,
  (index) =>
    `src/features/module${index % 12}/handler${index}.ts:${index + 10}:  const result = await resolve(payload);`,
);

const BUILD_LOG = lines(200, (index) => {
  if (index === 40) return "error: cannot find name 'Router'";
  if (index === 120) return "warning: unused variable 'cache'";
  return `[build] compiling module ${index}/200 ... ok (${index * 3}ms)`;
});

const PRESETS: Preset[] = [
  {
    id: "diff",
    label: "Code diff",
    expectation:
      "A large unified diff should shrink to a summary plus head/tail.",
    messages: [
      { role: "system", content: "You are a careful code reviewer." },
      {
        role: "user",
        content: "Review this change and tell me if it is safe.",
      },
      { role: "tool", content: DIFF, tool_call_id: "call_diff" },
    ],
  },
  {
    id: "grep",
    label: "Grep results",
    expectation:
      "Repetitive match lines should collapse into a deduplicated view.",
    messages: [
      { role: "user", content: "Where is resolve() called from?" },
      { role: "tool", content: GREP, tool_call_id: "call_grep" },
    ],
  },
  {
    id: "build",
    label: "Build log",
    expectation:
      "Routine progress lines go; errors and warnings stay verbatim.",
    messages: [
      { role: "user", content: "Did the build pass?" },
      { role: "tool", content: BUILD_LOG, tool_call_id: "call_build" },
    ],
  },
  {
    id: "prose",
    label: "Ordinary prose",
    expectation:
      "Nothing should change — the savers must never rewrite user language.",
    messages: [
      { role: "system", content: "You are a helpful assistant." },
      {
        role: "user",
        content:
          "Please explain, in a friendly and thorough way, how the routing layer decides which upstream serves a request, and why that ordering matters for reliability.",
      },
    ],
  },
];

const props = defineProps<{
  toggles: SaverToggle[];
  overrides: Partial<TokenSaverSettings>;
}>();

const emit = defineEmits<{
  toggle: [string];
  reset: [];
  /** A run invalidates any result the other tab was showing. */
  run: [];
}>();

const preset = ref(PRESETS[0]!.id);
/**
 * The editor is the source of truth for what runs: the samples seed it, and
 * whatever is in the box is what gets posted. Parsing happens on run so a
 * half-typed edit is never a problem.
 */
const messagesText = ref(JSON.stringify(PRESETS[0]!.messages, null, 2));
const messagesError = ref<string | null>(null);
const model = ref("");
/**
 * Held as a string so "empty" is expressible: an empty box means "do not
 * estimate the output side", which is different from a zero token count.
 */
const completionTokens = ref("1000");
const result = ref<PlaygroundResult | null>(null);
const running = ref(false);
const error = ref<string | null>(null);
const showTranscripts = ref(false);

const promptBefore = computed(() => result.value?.tokens_before ?? 0);
const promptAfter = computed(() => result.value?.tokens_after ?? 0);
const promptDelta = computed(() => promptAfter.value - promptBefore.value);

/** Share of the original prompt the input savers removed. */
const promptReduction = computed(() => {
  if (!result.value || promptBefore.value <= 0) return null;
  return (Math.max(0, -promptDelta.value) / promptBefore.value) * 100;
});

const totals = computed<SavingsTotals | null>(
  () => result.value?.totals ?? null,
);

const outputTokens = computed(() => {
  const value = totals.value;
  if (!value) return 0;
  return (
    value.saved_terse_tokens +
    value.saved_caveman_tokens +
    value.saved_ponytail_tokens
  );
});

function selectPreset(id: string): void {
  preset.value = id;
  const entry = PRESETS.find((candidate) => candidate.id === id);
  if (entry) messagesText.value = JSON.stringify(entry.messages, null, 2);
  messagesError.value = null;
  result.value = null;
  error.value = null;
}

/** Parses the editor, returning null and setting `messagesError` when invalid. */
function parseMessages(): PlaygroundMessage[] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(messagesText.value);
  } catch (caught) {
    messagesError.value = `Not valid JSON: ${
      caught instanceof Error ? caught.message : "parse failed"
    }`;
    return null;
  }

  if (!Array.isArray(parsed) || parsed.length === 0) {
    messagesError.value = "Provide a non-empty array of messages.";
    return null;
  }

  const messages = parsed as PlaygroundMessage[];
  const malformed = messages.findIndex(
    (message) =>
      !message ||
      typeof message.role !== "string" ||
      typeof message.content !== "string",
  );
  if (malformed !== -1) {
    messagesError.value = `Message ${malformed + 1} needs a role and string content.`;
    return null;
  }

  messagesError.value = null;
  return messages;
}

/** The assumed completion length, or undefined when the box is left empty. */
const assumedCompletion = computed<number | undefined>(() => {
  const raw = completionTokens.value.trim();
  if (!raw) return undefined;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed) || parsed < 0) return undefined;
  return Math.trunc(parsed);
});

async function run(): Promise<void> {
  const parsed = parseMessages();
  if (!parsed) return;

  running.value = true;
  error.value = null;
  try {
    result.value = await api.runPlayground({
      messages: parsed,
      model: model.value.trim() || undefined,
      assumed_completion_tokens: assumedCompletion.value,
      overrides: Object.keys(props.overrides).length
        ? (props.overrides as never)
        : undefined,
    });
  } catch (caught) {
    result.value = null;
    error.value =
      caught instanceof ApiError ? caught.message : "The playground run failed";
  } finally {
    running.value = false;
  }
}

/** `-12` / `+340` / `0`, so a step's direction reads at a glance. */
function formatDelta(delta: number): string {
  if (delta === 0) return "0";
  return delta > 0 ? `+${formatNumber(delta)}` : `-${formatNumber(-delta)}`;
}

function asText(message: PlaygroundMessage): string {
  return message.content ?? "";
}

defineExpose({ run });
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex justify-end">
      <Button :disabled="running" @click="run">
        <Play :class="running ? 'animate-pulse' : ''" />
        {{ running ? "Running…" : "Run pipeline" }}
      </Button>
    </div>

    <Card>
      <CardHeader>
        <CardTitle class="flex items-center gap-2">
          <FlaskConical class="size-4" />
          Sample request
        </CardTitle>
        <CardDescription>
          Pick a sample, then edit the messages below. The results refresh only
          when you run the pipeline.
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="flex flex-wrap gap-2">
          <Button
            v-for="entry in PRESETS"
            :key="entry.id"
            :variant="preset === entry.id ? 'default' : 'outline'"
            size="sm"
            @click="selectPreset(entry.id)"
          >
            {{ entry.label }}
          </Button>
        </div>
        <p class="flex items-start gap-2 text-xs text-muted-foreground">
          <Info class="mt-0.5 size-3.5 shrink-0" />
          {{ PRESETS.find((entry) => entry.id === preset)?.expectation }}
        </p>

        <div class="grid gap-3 lg:grid-cols-2">
          <div class="grid gap-2">
            <Label for="playground-messages">
              Messages (JSON, as sent to the router)
            </Label>
            <Textarea
              id="playground-messages"
              v-model="messagesText"
              class="min-h-64 font-mono text-xs"
              spellcheck="false"
            />
            <p v-if="messagesError" class="text-xs text-destructive">
              {{ messagesError }}
            </p>
          </div>

          <div class="grid content-start gap-3">
            <div class="grid gap-2">
              <Label for="playground-model">Model (optional)</Label>
              <Input
                id="playground-model"
                v-model="model"
                placeholder="gpt-4o"
              />
              <p class="text-xs text-muted-foreground">
                Used for the cost estimate and passed to Headroom.
              </p>
            </div>
            <div class="grid gap-2">
              <Label for="playground-completion">
                Assumed completion tokens (optional)
              </Label>
              <Input
                id="playground-completion"
                v-model="completionTokens"
                type="number"
                min="0"
                placeholder="1000"
              />
              <p class="text-xs text-muted-foreground">
                Give the output savers something to price. Leave empty and no
                output estimate is reported, because none can be known before
                the model answers.
              </p>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Savers for this run</CardTitle>
        <CardDescription>
          Defaults to your saved configuration.
          <RouterLink to="/token-saver" class="underline underline-offset-4">
            Change the defaults in Configuration
          </RouterLink>
          — or toggle them here to compare without saving.
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

    <Card v-if="error" class="border-destructive/40">
      <CardHeader>
        <CardTitle class="text-destructive">The run was rejected</CardTitle>
        <CardDescription>{{ error }}</CardDescription>
      </CardHeader>
    </Card>

    <template v-if="result">
      <div class="grid gap-4 sm:grid-cols-3">
        <Card>
          <CardHeader class="pb-2">
            <CardDescription>Prompt tokens</CardDescription>
          </CardHeader>
          <CardContent>
            <p
              class="flex items-center gap-2 text-2xl font-semibold tabular-nums"
            >
              {{ formatNumber(promptBefore) }}
              <ArrowRight class="size-4 text-muted-foreground" />
              {{ formatNumber(promptAfter) }}
            </p>
            <p class="pt-1 text-xs text-muted-foreground">
              {{
                promptDelta < 0
                  ? `${formatNumber(-promptDelta)} fewer, measured from the rewritten messages`
                  : promptDelta > 0
                    ? `${formatNumber(promptDelta)} more, the directives that were injected`
                    : "unchanged"
              }}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardDescription>Prompt reduction</CardDescription>
          </CardHeader>
          <CardContent>
            <p class="text-2xl font-semibold tabular-nums">
              {{
                promptReduction === null ? "—" : `${promptReduction.toFixed(1)}%`
              }}
            </p>
            <p class="pt-1 text-xs text-muted-foreground">
              Input savers only; directives add back a little, so this can be
              lower than the RTK figure below.
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader class="pb-2">
            <CardDescription>Completion tokens avoided</CardDescription>
          </CardHeader>
          <CardContent>
            <p class="text-2xl font-semibold tabular-nums">
              {{ totals ? formatNumber(outputTokens) : "—" }}
            </p>
            <p class="pt-1 text-xs text-muted-foreground">
              <template v-if="totals">
                Estimated from
                {{ formatNumber(assumedCompletion ?? 0) }} assumed completion
                tokens. Cost avoided {{ formatCost(totals.saved_cost_usd) }}.
              </template>
              <template v-else>
                No completion length was supplied, so nothing is estimated.
              </template>
            </p>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>What each step did</CardTitle>
          <CardDescription>
            Every saver that ran, in pipeline order. A step that declined is
            listed too, with unchanged counts.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div
            v-if="!result.steps.length"
            class="flex items-center gap-3 py-6 text-sm text-muted-foreground"
          >
            <Info class="size-5" />
            Every saver is off, so the request would be forwarded untouched.
          </div>
          <Table v-else>
            <TableHeader>
              <TableRow>
                <TableHead>Saver</TableHead>
                <TableHead>Side</TableHead>
                <TableHead>Result</TableHead>
                <TableHead class="text-right">Tokens in</TableHead>
                <TableHead class="text-right">Tokens out</TableHead>
                <TableHead class="text-right">Change</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow v-for="step in result.steps" :key="step.saver">
                <TableCell class="font-medium">{{ step.label }}</TableCell>
                <TableCell>
                  <Badge
                    :variant="step.side === 'input' ? 'secondary' : 'outline'"
                  >
                    {{ step.side }}
                  </Badge>
                </TableCell>
                <TableCell class="text-muted-foreground">
                  <span
                    class="flex items-center gap-1.5"
                    :class="step.applied ? 'text-foreground' : ''"
                  >
                    <CircleCheck
                      v-if="step.applied"
                      class="size-3.5 text-emerald-500"
                    />
                    <CircleAlert v-else class="size-3.5 text-amber-500" />
                    {{ step.detail }}
                  </span>
                </TableCell>
                <TableCell class="text-right tabular-nums">
                  {{ formatNumber(step.tokens_before) }}
                </TableCell>
                <TableCell class="text-right tabular-nums">
                  {{ formatNumber(step.tokens_after) }}
                </TableCell>
                <TableCell
                  class="text-right tabular-nums"
                  :class="
                    step.delta < 0
                      ? 'text-emerald-500'
                      : step.delta > 0
                        ? 'text-amber-500'
                        : ''
                  "
                >
                  {{ formatDelta(step.delta) }}
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
          <p class="pt-3 text-xs text-muted-foreground">
            Negative is a smaller prompt. Positive means the step added the
            instruction it injects — that cost is real, and is paid back by a
            shorter completion.
          </p>
        </CardContent>
      </Card>

      <Card v-if="result.notes.length">
        <CardHeader>
          <CardTitle>Notes</CardTitle>
          <CardDescription>
            Why a saver declined, or what it did. Failures never block a
            request.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ul class="grid gap-1 font-mono text-xs text-muted-foreground">
            <li v-for="(note, index) in result.notes" :key="index">
              {{ note }}
            </li>
          </ul>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Proof: the request that would be sent</CardTitle>
          <CardDescription>
            The exact messages the pipeline produced. A directive can add a
            system message, so this is shown as two full transcripts rather than
            a line-by-line diff.
          </CardDescription>
        </CardHeader>
        <CardContent class="grid gap-3">
          <Button
            variant="outline"
            size="sm"
            class="w-fit"
            @click="showTranscripts = !showTranscripts"
          >
            {{ showTranscripts ? "Hide transcripts" : "Show transcripts" }}
          </Button>
          <div v-if="showTranscripts" class="grid gap-4 lg:grid-cols-2">
            <div class="grid gap-2">
              <Label>Before · {{ formatNumber(promptBefore) }} tokens</Label>
              <pre
                class="max-h-96 overflow-auto rounded-md border bg-muted/40 p-3 text-xs whitespace-pre-wrap"
                >{{ result.before.map(asText).join("\n\n---\n\n") }}</pre>
            </div>
            <div class="grid gap-2">
              <Label>After · {{ formatNumber(promptAfter) }} tokens</Label>
              <pre
                class="max-h-96 overflow-auto rounded-md border bg-muted/40 p-3 text-xs whitespace-pre-wrap"
                >{{ result.after.map(asText).join("\n\n---\n\n") }}</pre>
            </div>
          </div>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
