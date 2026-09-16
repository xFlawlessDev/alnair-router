<script setup lang="ts">
import { computed } from "vue";
import { Wallet } from "@lucide/vue";
import { Progress } from "@/components/ui/progress";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { formatCompact, formatCost } from "@/lib/format";
import {
  configuredCaps,
  configuredTokenCaps,
  type BudgetCap,
  type BudgetCaps,
  type TokenCap,
  type TokenCaps,
} from "@/lib/limits";
import type { KeySpend, PublicBudgetCaps } from "@/types/api";

const props = defineProps<{
  spend: KeySpend | null;
  budget: PublicBudgetCaps;
}>();

const caps = computed<BudgetCaps>(() => ({
  daily_budget_usd: props.budget.daily_budget_usd,
  weekly_budget_usd: props.budget.weekly_budget_usd,
  monthly_budget_usd: props.budget.monthly_budget_usd,
  lifetime_budget_usd: props.budget.lifetime_budget_usd,
}));

const tokenCaps = computed<TokenCaps>(() => ({
  daily_token_limit: props.budget.daily_token_limit,
  weekly_token_limit: props.budget.weekly_token_limit,
  monthly_token_limit: props.budget.monthly_token_limit,
  lifetime_token_limit: props.budget.lifetime_token_limit,
}));

const configured = computed(() => configuredCaps(caps.value));
const configuredTokens = computed(() => configuredTokenCaps(tokenCaps.value));
const hasCaps = computed(
  () => configured.value.length > 0 || configuredTokens.value.length > 0,
);

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

function barClass(amount: number, spent: number): string {
  const used = spent / amount;
  if (used >= 1) return "[&>div]:bg-destructive";
  if (used >= 0.75) return "[&>div]:bg-amber-500";
  return "";
}
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle class="flex items-center gap-2 text-base">
        <Wallet class="size-4" />
        Budget & Limits
      </CardTitle>
      <CardDescription
        >Your spend against configured budget limits.</CardDescription
      >
    </CardHeader>
    <CardContent class="grid gap-4">
      <template v-if="hasCaps">
        <div
          v-for="{ cap, amount } in configured"
          :key="cap.field"
          class="grid gap-1.5"
        >
          <div class="flex items-baseline justify-between gap-2 text-sm">
            <span class="text-muted-foreground">{{ cap.label }}</span>
            <span
              :class="[
                'tabular-nums',
                spentFor(cap.field) >= amount
                  ? 'font-medium text-destructive'
                  : '',
              ]"
            >
              {{ formatCost(spentFor(cap.field)) }} / {{ formatCost(amount) }}
            </span>
          </div>
          <Progress
            :model-value="percent(amount, spentFor(cap.field))"
            class="h-2"
            :class="barClass(amount, spentFor(cap.field))"
            :aria-label="`${cap.label} budget usage`"
          />
        </div>

        <div
          v-for="{ cap, amount } in configuredTokens"
          :key="cap.field"
          class="grid gap-1.5"
        >
          <div class="flex items-baseline justify-between gap-2 text-sm">
            <span class="text-muted-foreground">{{ cap.label }} tokens</span>
            <span
              :class="[
                'tabular-nums',
                tokensSpentFor(cap.field) >= amount
                  ? 'font-medium text-destructive'
                  : '',
              ]"
            >
              {{ formatCompact(tokensSpentFor(cap.field)) }} /
              {{ formatCompact(amount) }}
            </span>
          </div>
          <Progress
            :model-value="percent(amount, tokensSpentFor(cap.field))"
            class="h-2"
            :class="barClass(amount, tokensSpentFor(cap.field))"
            :aria-label="`${cap.label} token usage`"
          />
        </div>
      </template>

      <p v-else class="text-sm text-muted-foreground">
        No budget limits configured for this key.
      </p>
    </CardContent>
  </Card>
</template>
