<script setup lang="ts">
import { RefreshCw, Save, Sparkles } from "@lucide/vue";
import { computed, onMounted, ref } from "vue";

import PageHeader from "@/components/PageHeader.vue";
import SettingsSection from "@/components/settings/SettingsSection.vue";
import HeadroomGuide from "@/components/token-saver/HeadroomGuide.vue";
import TokenSaverConfig from "@/components/token-saver/TokenSaverConfig.vue";
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ApiError, api } from "@/lib/api";
import { formatCompact, formatCost, formatNumber } from "@/lib/format";
import { USAGE_RANGES, rangeToSince } from "@/lib/ranges";
import { useSettingsController } from "@/lib/settings/controller";
import { TOKEN_SAVER_SECTION, sectionIsDirty } from "@/lib/settings/sections";
import type { SavingsSummary, UsageSummary } from "@/types/api";

/** One saver's contribution, and whether the number is a measurement. */
interface SaverRow {
  saver: string;
  detail: string;
  tokens: number;
  estimated: boolean;
}

const {
  settings,
  loading: configLoading,
  saving,
  patch,
  failed: configFailed,
  error: configError,
  load: loadConfig,
  save,
} = useSettingsController();

const tab = ref<"savings" | "configuration">("savings");

const summary = ref<UsageSummary | null>(null);
const range = ref("all");
const usageLoading = ref(true);
const usageError = ref<string | null>(null);

const savings = computed<SavingsSummary | null>(
  () => summary.value?.savings ?? null,
);

const activeSavers = computed(() => {
  const config = settings.value?.token_saver;
  if (!config) return [];
  const active: string[] = [];
  if (config.slimmer_enabled) active.push("RTK");
  if (config.headroom_enabled) active.push("Headroom");
  if (config.terse_enabled) active.push("Terse");
  if (config.caveman_enabled) active.push("Caveman");
  if (config.ponytail_enabled) active.push("Ponytail");
  return active;
});

/** Measured input tokens: RTK and Headroom are counted, not estimated. */
const measuredTokens = computed(() =>
  savings.value
    ? savings.value.saved_rtk_tokens + savings.value.saved_headroom_tokens
    : 0,
);

const estimatedTokens = computed(() => {
  const totals = savings.value;
  if (!totals) return 0;
  return (
    totals.saved_terse_tokens +
    totals.saved_caveman_tokens +
    totals.saved_ponytail_tokens
  );
});

const totalSaved = computed(() => measuredTokens.value + estimatedTokens.value);

/**
 * Share of all prompt tokens the pipeline removed. Measured savings only: the
 * output estimates would double-count against a denominator we do not control.
 */
const saveRate = computed(() => {
  const totals = summary.value;
  if (!totals || totals.prompt_tokens <= 0) return null;
  const billed = totals.prompt_tokens;
  return (measuredTokens.value / (billed + measuredTokens.value)) * 100;
});

const rows = computed<SaverRow[]>(() => {
  const totals = savings.value;
  if (!totals) return [];
  return [
    {
      saver: "RTK / Slimmer",
      detail: "Tool output compressed before dispatch",
      tokens: totals.saved_rtk_tokens,
      estimated: false,
    },
    {
      saver: "Headroom",
      detail: "Deeper compression via the external proxy",
      tokens: totals.saved_headroom_tokens,
      estimated: false,
    },
    {
      saver: "Terse",
      detail: "Concise-output directive",
      tokens: totals.saved_terse_tokens,
      estimated: true,
    },
    {
      saver: "Caveman",
      detail: "Stronger terseness directive",
      tokens: totals.saved_caveman_tokens,
      estimated: true,
    },
    {
      saver: "Ponytail",
      detail: "Lazy-senior-dev directive, stacks on Terse/Caveman",
      tokens: totals.saved_ponytail_tokens,
      estimated: true,
    },
  ].filter((row) => row.tokens > 0);
});

const rangeLabel = computed(
  () =>
    USAGE_RANGES.find((entry) => entry.value === range.value)?.label ??
    "All time",
);

/** The header text has to describe whichever tab is on screen. */
const description = computed(() =>
  tab.value === "savings"
    ? "What the deterministic pipeline removed before requests reached a provider. Input savings are measured; output savings are estimates."
    : "Which savers run on every request, in order. Changes are stored as settings overrides and apply to new requests once saved.",
);

/** This page edits only the token-saving keys, so the bar tracks just those. */
const configDirty = computed(() =>
  sectionIsDirty(patch.value, TOKEN_SAVER_SECTION),
);

async function loadSavings(): Promise<void> {
  usageLoading.value = true;
  usageError.value = null;
  try {
    summary.value = await api.usageSummary({
      since: rangeToSince(range.value),
    });
  } catch (caught) {
    usageError.value =
      caught instanceof ApiError ? caught.message : "Failed to load savings";
  } finally {
    usageLoading.value = false;
  }
}

async function refresh(): Promise<void> {
  await Promise.all([loadSavings(), loadConfig()]);
}

onMounted(refresh);
</script>

<template>
  <div class="flex flex-col gap-6 pb-20">
    <PageHeader title="Token Saving" :description="description">
      <template #actions>
        <!-- The range only narrows the savings window, so it stays off the
             configuration tab where it would read as a no-op. -->
        <Select
          v-if="tab === 'savings'"
          v-model="range"
          @update:model-value="loadSavings"
        >
          <SelectTrigger class="w-[10.5rem]" aria-label="Time range">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="entry in USAGE_RANGES"
              :key="entry.value"
              :value="entry.value"
            >
              {{ entry.label }}
            </SelectItem>
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="sm"
          :disabled="usageLoading || configLoading"
          @click="refresh"
        >
          <RefreshCw :class="usageLoading ? 'animate-spin' : ''" /> Refresh
        </Button>
      </template>
    </PageHeader>

    <Tabs v-model="tab" class="gap-6">
      <TabsList>
        <TabsTrigger value="savings">Savings</TabsTrigger>
        <TabsTrigger value="configuration">Configuration</TabsTrigger>
      </TabsList>

      <TabsContent value="savings" class="flex flex-col gap-4">
        <Card v-if="usageError" class="border-destructive/40">
          <CardHeader>
            <CardTitle class="text-destructive">Cannot load savings</CardTitle>
            <CardDescription>{{ usageError }}</CardDescription>
          </CardHeader>
        </Card>

        <template v-else>
          <div class="grid gap-4 sm:grid-cols-3">
            <Card>
              <CardHeader class="pb-2">
                <CardDescription>
                  Tokens saved · {{ rangeLabel }}
                </CardDescription>
              </CardHeader>
              <CardContent>
                <Skeleton v-if="usageLoading" class="h-8 w-24" />
                <p v-else class="text-2xl font-semibold tabular-nums">
                  {{ formatCompact(totalSaved) }}
                </p>
                <p class="pt-1 text-xs text-muted-foreground">
                  {{ formatNumber(measuredTokens) }} measured input ·
                  {{ formatNumber(estimatedTokens) }} estimated output
                </p>
              </CardContent>
            </Card>

            <Card>
              <CardHeader class="pb-2">
                <CardDescription>Cost avoided</CardDescription>
              </CardHeader>
              <CardContent>
                <Skeleton v-if="usageLoading" class="h-8 w-24" />
                <p v-else class="text-2xl font-semibold tabular-nums">
                  {{ formatCost(savings?.saved_cost_usd ?? 0) }}
                </p>
                <p class="pt-1 text-xs text-muted-foreground">
                  Priced at each request's serving tier rate
                </p>
              </CardContent>
            </Card>

            <Card>
              <CardHeader class="pb-2">
                <CardDescription>Prompt tokens avoided</CardDescription>
              </CardHeader>
              <CardContent>
                <Skeleton v-if="usageLoading" class="h-8 w-24" />
                <p v-else class="text-2xl font-semibold tabular-nums">
                  {{ saveRate === null ? "—" : `${saveRate.toFixed(1)}%` }}
                </p>
                <p class="pt-1 text-xs text-muted-foreground">
                  Measured input savings as a share of what would have been sent
                </p>
              </CardContent>
            </Card>
          </div>

          <Card>
            <CardHeader>
              <CardTitle>Configured savers</CardTitle>
              <CardDescription>
                Set under
                <button
                  type="button"
                  class="underline underline-offset-4"
                  @click="tab = 'configuration'"
                >
                  Configuration</button
                >, on this page.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Skeleton v-if="configLoading" class="h-6 w-64" />
              <div v-else-if="activeSavers.length" class="flex flex-wrap gap-2">
                <Badge
                  v-for="saver in activeSavers"
                  :key="saver"
                  variant="secondary"
                >
                  {{ saver }}
                </Badge>
              </div>
              <p v-else class="text-sm text-muted-foreground">
                Every saver is off, so requests are forwarded untouched.
              </p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Breakdown by saver</CardTitle>
              <CardDescription>
                One row per saver that contributed in this window.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Skeleton v-if="usageLoading" class="h-24 w-full" />
              <div
                v-else-if="!rows.length"
                class="flex flex-wrap items-center gap-3 py-6 text-sm text-muted-foreground"
              >
                <Sparkles class="size-5" />
                Nothing saved yet.
                <button
                  type="button"
                  class="underline underline-offset-4"
                  @click="tab = 'configuration'"
                >
                  Enable a saver</button
                >, then send a request carrying tool output.
              </div>
              <Table v-else>
                <TableHeader>
                  <TableRow>
                    <TableHead>Saver</TableHead>
                    <TableHead>What it does</TableHead>
                    <TableHead>Source</TableHead>
                    <TableHead class="text-right">Tokens saved</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  <TableRow v-for="row in rows" :key="row.saver">
                    <TableCell class="font-medium">{{ row.saver }}</TableCell>
                    <TableCell class="text-muted-foreground">{{
                      row.detail
                    }}</TableCell>
                    <TableCell>
                      <Badge :variant="row.estimated ? 'outline' : 'secondary'">
                        {{ row.estimated ? "estimated" : "measured" }}
                      </Badge>
                    </TableCell>
                    <TableCell class="text-right tabular-nums">
                      {{ formatNumber(row.tokens) }}
                    </TableCell>
                  </TableRow>
                </TableBody>
              </Table>
              <p class="pt-3 text-xs text-muted-foreground">
                Estimated rows are derived from a documented reduction ratio
                applied to the completion, because a terser answer cannot be
                measured after the fact.
              </p>
            </CardContent>
          </Card>
        </template>
      </TabsContent>

      <TabsContent value="configuration">
        <div class="grid gap-4">
          <div
            v-if="configFailed"
            class="flex items-start gap-3 rounded-lg border border-destructive/40 bg-destructive/5 px-4 py-3.5"
          >
            <div class="grid gap-1">
              <p class="text-sm font-medium text-destructive">
                Cannot load configuration
              </p>
              <p class="text-xs text-pretty text-muted-foreground">
                {{ configError }}
              </p>
              <Button
                variant="outline"
                size="sm"
                class="mt-1 w-fit"
                :disabled="configLoading"
                @click="loadConfig"
              >
                <RefreshCw :class="configLoading ? 'animate-spin' : ''" /> Try
                again
              </Button>
            </div>
          </div>

          <SettingsSection v-else :section="TOKEN_SAVER_SECTION">
            <template #title>
              <Sparkles class="size-4 text-muted-foreground" />
              Pipeline
            </template>

            <TokenSaverConfig />
          </SettingsSection>

          <HeadroomGuide @open-savings="tab = 'savings'" />
        </div>
      </TabsContent>
    </Tabs>

    <!-- The header's Save scrolls away, so unsaved work gets its own bar. -->
    <Transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="translate-y-2 opacity-0"
      leave-active-class="transition duration-100 ease-in"
      leave-to-class="translate-y-2 opacity-0"
    >
      <div
        v-if="configDirty && !configFailed"
        class="pointer-events-none sticky bottom-4 z-20 flex justify-center"
      >
        <div
          class="pointer-events-auto flex w-full max-w-3xl flex-wrap items-center gap-3 rounded-xl border bg-background/95 px-4 py-3 shadow-lg backdrop-blur supports-[backdrop-filter]:bg-background/80"
        >
          <div class="min-w-0 flex-1">
            <p class="text-sm font-medium">Unsaved changes</p>
            <p class="text-xs text-muted-foreground">
              The pipeline applies to new requests as soon as you save.
            </p>
          </div>
          <div class="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              :disabled="saving"
              @click="loadConfig"
            >
              Discard
            </Button>
            <Button size="sm" :disabled="saving || configLoading" @click="save">
              <Save /> {{ saving ? "Saving…" : "Save changes" }}
            </Button>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>
