<script setup lang="ts">
import { ChevronDown } from '@lucide/vue';
import { computed, ref } from 'vue';

import ExpiryPicker from '@/components/keys/ExpiryPicker.vue';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { BudgetMode } from '@/types/api';

const props = defineProps<{
  idPrefix: string;
  scope: 'key' | 'plan';
  rateLimit: string;
  daily: string;
  weekly: string;
  monthly: string;
  lifetime: string;
  dailyTokens: string;
  weeklyTokens: string;
  monthlyTokens: string;
  lifetimeTokens: string;
  budgetMode: BudgetMode;
  expires: string;
}>();
const emit = defineEmits<{
  'update:rate-limit': [value: string];
  'update:daily': [value: string];
  'update:weekly': [value: string];
  'update:monthly': [value: string];
  'update:lifetime': [value: string];
  'update:daily-tokens': [value: string];
  'update:weekly-tokens': [value: string];
  'update:monthly-tokens': [value: string];
  'update:lifetime-tokens': [value: string];
  'update:budget-mode': [value: BudgetMode];
  'update:expires': [value: string];
}>();

const BUDGET_MODES: { value: BudgetMode; label: string }[] = [
  { value: 'off', label: 'Off — no cap' },
  { value: 'warn', label: 'Warn — allow, flag overspend' },
  { value: 'block', label: 'Block — reject when exhausted' },
];

const WINDOWS = [
  { key: 'daily', label: 'Daily (USD)' },
  { key: 'weekly', label: 'Weekly (USD)' },
  { key: 'monthly', label: 'Monthly (USD)' },
  { key: 'lifetime', label: 'Lifetime (USD)' },
] as const;

const TOKEN_WINDOWS = [
  { key: 'dailyTokens', label: 'Daily tokens' },
  { key: 'weeklyTokens', label: 'Weekly tokens' },
  { key: 'monthlyTokens', label: 'Monthly tokens' },
  { key: 'lifetimeTokens', label: 'Lifetime tokens' },
] as const;

type WindowKey = (typeof WINDOWS)[number]['key'];
type TokenWindowKey = (typeof TOKEN_WINDOWS)[number]['key'];

/** Collapsed by default so the form stays compact; the trigger shows a summary. */
const open = ref(false);

const budgetCount = computed(
  () => [props.daily, props.weekly, props.monthly, props.lifetime].filter((value) => value.trim()).length,
);
const tokenCount = computed(
  () =>
    [props.dailyTokens, props.weeklyTokens, props.monthlyTokens, props.lifetimeTokens].filter(
      (value) => value.trim(),
    ).length,
);
const summary = computed(() => {
  const parts: string[] = [];
  if (budgetCount.value) {
    parts.push(`${budgetCount.value} USD budget${budgetCount.value > 1 ? 's' : ''}`);
  }
  if (tokenCount.value) {
    parts.push(`${tokenCount.value} token limit${tokenCount.value > 1 ? 's' : ''}`);
  }
  if (props.budgetMode !== 'off') parts.push(props.budgetMode);
  return parts.length ? parts.join(' · ') : 'No budgets or token limits';
});

function windowValue(key: WindowKey): string {
  if (key === 'daily') return props.daily;
  if (key === 'weekly') return props.weekly;
  if (key === 'monthly') return props.monthly;
  return props.lifetime;
}

function setWindow(key: WindowKey, value: string | number): void {
  const text = String(value);
  if (key === 'daily') emit('update:daily', text);
  else if (key === 'weekly') emit('update:weekly', text);
  else if (key === 'monthly') emit('update:monthly', text);
  else emit('update:lifetime', text);
}

function tokenValue(key: TokenWindowKey): string {
  if (key === 'dailyTokens') return props.dailyTokens;
  if (key === 'weeklyTokens') return props.weeklyTokens;
  if (key === 'monthlyTokens') return props.monthlyTokens;
  return props.lifetimeTokens;
}

function setTokenWindow(key: TokenWindowKey, value: string | number): void {
  const text = String(value);
  if (key === 'dailyTokens') emit('update:daily-tokens', text);
  else if (key === 'weeklyTokens') emit('update:weekly-tokens', text);
  else if (key === 'monthlyTokens') emit('update:monthly-tokens', text);
  else emit('update:lifetime-tokens', text);
}
</script>

<template>
  <div class="grid gap-4">
    <div class="grid gap-2">
      <Label :for="`${idPrefix}-rate-limit`">Rate limit (requests/minute)</Label>
      <Input
        :id="`${idPrefix}-rate-limit`"
        :model-value="rateLimit"
        type="number"
        min="1"
        placeholder="Blank inherits the plan or server default"
        @update:model-value="emit('update:rate-limit', String($event))"
      />
    </div>

    <Collapsible v-model:open="open" class="rounded-md border">
      <CollapsibleTrigger
        class="flex w-full items-center justify-between gap-3 p-3 text-left transition-colors hover:bg-muted/50"
      >
        <span class="grid gap-0.5">
          <span class="text-sm font-medium">Budgets, token limits &amp; expiry</span>
          <span class="text-xs text-muted-foreground">{{ summary }}</span>
        </span>
        <ChevronDown
          class="size-4 shrink-0 text-muted-foreground transition-transform"
          :class="open ? 'rotate-180' : ''"
        />
      </CollapsibleTrigger>

      <CollapsibleContent>
        <div class="grid gap-3 border-t p-3">
          <div>
            <Label>Spend budgets (USD)</Label>
            <p class="text-xs text-muted-foreground">
              Caps stack: a request is refused when any configured window is exhausted. Daily,
              weekly and monthly reset on the UTC calendar; lifetime never resets. Blank windows
              are uncapped.
            </p>
          </div>

          <div class="grid gap-3 sm:grid-cols-2">
            <div v-for="window in WINDOWS" :key="window.key" class="grid gap-2">
              <Label :for="`${idPrefix}-budget-${window.key}`">{{ window.label }}</Label>
              <Input
                :id="`${idPrefix}-budget-${window.key}`"
                :model-value="windowValue(window.key)"
                type="number"
                min="0.01"
                step="0.01"
                placeholder="Uncapped"
                @update:model-value="setWindow(window.key, $event)"
              />
            </div>
          </div>

          <div class="grid gap-3 border-t pt-3">
            <div>
              <Label>Token limits (prompt + completion)</Label>
              <p class="text-xs text-muted-foreground">
                Enforced with the same mode as the USD budgets; blank windows are uncapped.
              </p>
            </div>
            <div class="grid gap-3 sm:grid-cols-2">
              <div v-for="window in TOKEN_WINDOWS" :key="window.key" class="grid gap-2">
                <Label :for="`${idPrefix}-tokens-${window.key}`">{{ window.label }}</Label>
                <Input
                  :id="`${idPrefix}-tokens-${window.key}`"
                  :model-value="tokenValue(window.key)"
                  type="number"
                  min="1"
                  step="1"
                  placeholder="Uncapped"
                  @update:model-value="setTokenWindow(window.key, $event)"
                />
              </div>
            </div>
          </div>

          <div class="grid gap-3 border-t pt-3 sm:grid-cols-2">
            <div class="grid gap-2">
              <Label>Budget mode</Label>
              <Select
                :model-value="budgetMode"
                @update:model-value="emit('update:budget-mode', $event as BudgetMode)"
              >
                <SelectTrigger class="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent :side-offset="4" class="max-w-[min(20rem,calc(100vw-2rem))]">
                  <SelectItem v-for="mode in BUDGET_MODES" :key="mode.value" :value="mode.value">
                    {{ mode.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div class="grid gap-2">
              <Label :for="`${idPrefix}-expires`">Expires at (optional)</Label>
              <ExpiryPicker
                :id="`${idPrefix}-expires`"
                :model-value="expires"
                @update:model-value="emit('update:expires', $event)"
              />
            </div>
          </div>
          <p class="text-xs text-muted-foreground">
            <template v-if="scope === 'plan'">
              Keys attached to an expired plan are rejected until the plan is extended.
            </template>
            <template v-else>
              An expired key is rejected with 401; leave blank to never expire.
            </template>
          </p>
        </div>
      </CollapsibleContent>
    </Collapsible>
  </div>
</template>
