<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import LimitFields from "@/components/keys/LimitFields.vue";
import ModelAllowlistInput from "@/components/keys/ModelAllowlistInput.vue";
import { ApiError, api } from "@/lib/api";
import {
  fromLocalDateTime,
  parsePositive,
  parsePositiveInt,
  toLocalDateTime,
} from "@/lib/limits";
import type {
  Alias,
  ApiKey,
  BudgetMode,
  ComboWithEntries,
  KeyPlan,
} from "@/types/api";

const props = defineProps<{
  open: boolean;
  apiKey: ApiKey | null;
  plans: KeyPlan[];
  aliases: Alias[];
  combos: ComboWithEntries[];
}>();
const emit = defineEmits<{
  "update:open": [boolean];
  saved: [secret?: string];
}>();

/** Sentinel because Select values cannot be empty strings. */
const NO_PLAN = "__no_plan__";

const name = ref("");
const enabled = ref(true);
const rateLimit = ref("");
const daily = ref("");
const weekly = ref("");
const monthly = ref("");
const lifetime = ref("");
const dailyTokens = ref("");
const weeklyTokens = ref("");
const monthlyTokens = ref("");
const lifetimeTokens = ref("");
const budgetMode = ref<BudgetMode>("off");
const expires = ref("");
const planId = ref(NO_PLAN);
const models = ref<string[]>([]);
const saving = ref(false);

const isEdit = computed(() => props.apiKey !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const key = props.apiKey;
    name.value = key?.name ?? "";
    enabled.value = key ? key.enabled !== 0 : true;
    rateLimit.value =
      key?.rate_limit_per_minute != null
        ? String(key.rate_limit_per_minute)
        : "";
    daily.value =
      key?.daily_budget_usd != null ? String(key.daily_budget_usd) : "";
    weekly.value =
      key?.weekly_budget_usd != null ? String(key.weekly_budget_usd) : "";
    monthly.value =
      key?.monthly_budget_usd != null ? String(key.monthly_budget_usd) : "";
    lifetime.value =
      key?.lifetime_budget_usd != null ? String(key.lifetime_budget_usd) : "";
    dailyTokens.value =
      key?.daily_token_limit != null ? String(key.daily_token_limit) : "";
    weeklyTokens.value =
      key?.weekly_token_limit != null ? String(key.weekly_token_limit) : "";
    monthlyTokens.value =
      key?.monthly_token_limit != null ? String(key.monthly_token_limit) : "";
    lifetimeTokens.value =
      key?.lifetime_token_limit != null ? String(key.lifetime_token_limit) : "";
    budgetMode.value = key?.budget_mode ?? "off";
    expires.value = toLocalDateTime(key?.expires_at);
    planId.value = key?.plan_id ?? NO_PLAN;
    models.value = key?.allowed_models ? [...key.allowed_models] : [];
  },
);

async function save(): Promise<void> {
  if (!name.value.trim()) {
    toast.error("Name is required");
    return;
  }
  if (rateLimit.value.trim() && parsePositive(rateLimit.value) === null) {
    toast.error("Rate limit must be a positive number");
    return;
  }

  for (const [label, value] of [
    ["Daily", daily.value],
    ["Weekly", weekly.value],
    ["Monthly", monthly.value],
    ["Lifetime", lifetime.value],
  ] as const) {
    if (value.trim() && parsePositive(value) === null) {
      toast.error(`${label} budget must be a positive number`);
      return;
    }
  }

  for (const [label, value] of [
    ["Daily", dailyTokens.value],
    ["Weekly", weeklyTokens.value],
    ["Monthly", monthlyTokens.value],
    ["Lifetime", lifetimeTokens.value],
  ] as const) {
    if (value.trim() && parsePositiveInt(value) === null) {
      toast.error(`${label} token limit must be a positive whole number`);
      return;
    }
  }

  const rpm = parsePositive(rateLimit.value);
  const dailyUsd = parsePositive(daily.value);
  const weeklyUsd = parsePositive(weekly.value);
  const monthlyUsd = parsePositive(monthly.value);
  const lifetimeUsd = parsePositive(lifetime.value);
  const dailyTok = parsePositiveInt(dailyTokens.value);
  const weeklyTok = parsePositiveInt(weeklyTokens.value);
  const monthlyTok = parsePositiveInt(monthlyTokens.value);
  const lifetimeTok = parsePositiveInt(lifetimeTokens.value);
  if (
    budgetMode.value !== "off" &&
    dailyUsd === null &&
    weeklyUsd === null &&
    monthlyUsd === null &&
    lifetimeUsd === null &&
    dailyTok === null &&
    weeklyTok === null &&
    monthlyTok === null &&
    lifetimeTok === null
  ) {
    toast.error(
      "Set at least one budget or token limit before choosing warn or block",
    );
    return;
  }

  const body = {
    name: name.value.trim(),
    enabled: enabled.value,
    rate_limit_per_minute: rpm,
    daily_budget_usd: dailyUsd,
    weekly_budget_usd: weeklyUsd,
    monthly_budget_usd: monthlyUsd,
    lifetime_budget_usd: lifetimeUsd,
    daily_token_limit: dailyTok,
    weekly_token_limit: weeklyTok,
    monthly_token_limit: monthlyTok,
    lifetime_token_limit: lifetimeTok,
    budget_mode: budgetMode.value,
    plan_id: planId.value === NO_PLAN ? null : planId.value,
    allowed_models: models.value.length ? models.value : null,
    expires_at: fromLocalDateTime(expires.value),
  };

  saving.value = true;
  try {
    if (props.apiKey) {
      await api.updateKey(props.apiKey.id, body);
      toast.success(`Key “${body.name}” updated`);
      emit("saved");
    } else {
      const created = await api.createKey(body);
      toast.success(`Key “${created.key.name}” created`);
      emit("saved", created.secret);
    }
    emit("update:open", false);
  } catch (error) {
    toast.error(
      error instanceof ApiError ? error.message : "Failed to save key",
    );
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent>
      <DialogHeader>
        <DialogTitle>{{
          isEdit ? "Edit API key" : "Create API key"
        }}</DialogTitle>
        <DialogDescription>
          <template v-if="isEdit"
            >Changes apply immediately to new requests.</template
          >
          <template v-else>
            The plaintext secret is shown exactly once and never stored.
          </template>
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="key-name">Name</Label>
          <Input id="key-name" v-model="name" placeholder="laptop-cli" />
        </div>

        <div
          class="flex items-center justify-between gap-4 rounded-md border p-3"
        >
          <div>
            <Label for="key-enabled">Enabled</Label>
            <p class="text-xs text-muted-foreground">
              Disabled keys are rejected on /v1.
            </p>
          </div>
          <Switch id="key-enabled" v-model="enabled" />
        </div>

        <LimitFields
          id-prefix="key"
          scope="key"
          v-model:rate-limit="rateLimit"
          v-model:daily="daily"
          v-model:weekly="weekly"
          v-model:monthly="monthly"
          v-model:lifetime="lifetime"
          v-model:daily-tokens="dailyTokens"
          v-model:weekly-tokens="weeklyTokens"
          v-model:monthly-tokens="monthlyTokens"
          v-model:lifetime-tokens="lifetimeTokens"
          v-model:budget-mode="budgetMode"
          v-model:expires="expires"
        />

        <div class="grid gap-2">
          <Label>Plan</Label>
          <Select v-model="planId">
            <SelectTrigger class="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem :value="NO_PLAN"
                >No plan — key stands alone</SelectItem
              >
              <SelectItem v-for="plan in plans" :key="plan.id" :value="plan.id">
                {{ plan.name }}
              </SelectItem>
            </SelectContent>
          </Select>
          <p class="text-xs text-muted-foreground">
            The plan fills every field this key leaves empty; anything set here
            wins.
          </p>
        </div>

        <div class="grid gap-2">
          <Label>Model allowlist</Label>
          <ModelAllowlistInput
            v-model="models"
            :aliases="aliases"
            :combos="combos"
            hint="Empty inherits the plan; with no plan it allows every model. Wildcards: openai/*, *."
          />
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)"
          >Cancel</Button
        >
        <Button :disabled="saving" @click="save">
          {{ saving ? "Saving…" : isEdit ? "Save changes" : "Create key" }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
