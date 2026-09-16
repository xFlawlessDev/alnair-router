<script setup lang="ts">
import { computed } from "vue";

import { Progress } from "@/components/ui/progress";
import { formatCompact, formatCost } from "@/lib/format";
import {
  configuredCaps,
  configuredTokenCaps,
  type BudgetCap,
  type BudgetCaps,
  type TokenCap,
  type TokenCaps,
} from "@/lib/limits";
import type { KeySpend } from "@/types/api";

const props = defineProps<{
  spend: KeySpend | null;
  caps: BudgetCaps;
  tokenCaps: TokenCaps;
}>();

const configured = computed(() => configuredCaps(props.caps));
const configuredTokens = computed(() => configuredTokenCaps(props.tokenCaps));
const lifetimeUsd = computed(() => props.spend?.lifetime_usd ?? 0);
const lifetimeTokens = computed(() => props.spend?.lifetime_tokens ?? 0);

function spentFor(field: BudgetCap["field"]): number {
  const spend = props.spend;
  if (!spend) return 0;
  if (field === "daily_budget_usd") return spend.daily_usd;
  if (field === "weekly_budget_usd") return spend.weekly_usd;
  if (field === "monthly_budget_usd") return spend.monthly_usd;
  return spend.lifetime_usd;
}

function tokensSpentFor(field: TokenCap["field"]): number {
  const spend = props.spend;
  if (!spend) return 0;
  if (field === "daily_token_limit") return spend.daily_tokens;
  if (field === "weekly_token_limit") return spend.weekly_tokens;
  if (field === "monthly_token_limit") return spend.monthly_tokens;
  return spend.lifetime_tokens;
}

function ratio(amount: number, spent: number): number {
  if (amount <= 0) return 0;
  return Math.min(spent / amount, 1);
}

function percent(amount: number, spent: number): number {
  return ratio(amount, spent) * 100;
}

/** Progress bars turn amber near the cap and destructive once exhausted. */
function barClass(amount: number, spent: number): string {
  const used = spent / amount;
  if (used >= 1) return "[&>div]:bg-destructive";
  if (used >= 0.75) return "[&>div]:bg-amber-500";
  return "";
}
</script>

<template>
  <div class="grid min-w-40 gap-2">
    <template v-if="configured.length || configuredTokens.length">
      <div
        v-for="{ cap, amount } in configured"
        :key="cap.field"
        class="grid gap-1"
      >
        <div class="flex items-baseline justify-between gap-2 text-xs">
          <span class="capitalize text-muted-foreground">{{ cap.suffix }}</span>
          <span
            :class="
              spentFor(cap.field) >= amount
                ? 'font-medium text-destructive'
                : ''
            "
          >
            {{ formatCost(spentFor(cap.field)) }} / {{ formatCost(amount) }}
          </span>
        </div>
        <Progress
          :model-value="percent(amount, spentFor(cap.field))"
          class="h-1.5"
          :class="barClass(amount, spentFor(cap.field))"
          :aria-label="`${cap.label} spend`"
        />
      </div>
      <div
        v-for="{ cap, amount } in configuredTokens"
        :key="cap.field"
        class="grid gap-1"
      >
        <div class="flex items-baseline justify-between gap-2 text-xs">
          <span class="capitalize text-muted-foreground"
            >{{ cap.suffix }} tok</span
          >
          <span
            :class="
              tokensSpentFor(cap.field) >= amount
                ? 'font-medium text-destructive'
                : ''
            "
          >
            {{ formatCompact(tokensSpentFor(cap.field)) }} /
            {{ formatCompact(amount) }}
          </span>
        </div>
        <Progress
          :model-value="percent(amount, tokensSpentFor(cap.field))"
          class="h-1.5"
          :class="barClass(amount, tokensSpentFor(cap.field))"
          :aria-label="`${cap.label} token spend`"
        />
      </div>
    </template>
    <p class="text-xs text-muted-foreground">
      <template v-if="configured.length || configuredTokens.length">
        {{ formatCost(lifetimeUsd) }} · {{ formatCompact(lifetimeTokens) }} tok
        total
      </template>
      <template v-else-if="spend">
        {{ formatCost(lifetimeUsd) }} · {{ formatCompact(lifetimeTokens) }} tok
        spent · uncapped
      </template>
      <template v-else>No usage yet</template>
    </p>
  </div>
</template>
