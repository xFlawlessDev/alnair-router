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
import LimitFields from "@/components/keys/LimitFields.vue";
import ModelAllowlistInput from "@/components/keys/ModelAllowlistInput.vue";
import { ApiError, api } from "@/lib/api";
import {
  fromLocalDateTime,
  parsePositive,
  parsePositiveInt,
  toLocalDateTime,
} from "@/lib/limits";
import type { Alias, BudgetMode, ComboWithEntries, KeyPlan } from "@/types/api";

const props = defineProps<{
  open: boolean;
  plan: KeyPlan | null;
  aliases: Alias[];
  combos: ComboWithEntries[];
}>();
const emit = defineEmits<{ "update:open": [boolean]; saved: [] }>();

const name = ref("");
const description = ref("");
const models = ref<string[]>([]);
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
const saving = ref(false);

const isEdit = computed(() => props.plan !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const plan = props.plan;
    name.value = plan?.name ?? "";
    description.value = plan?.description ?? "";
    models.value = plan ? [...plan.allowed_models] : [];
    rateLimit.value =
      plan?.rate_limit_per_minute != null
        ? String(plan.rate_limit_per_minute)
        : "";
    daily.value =
      plan?.daily_budget_usd != null ? String(plan.daily_budget_usd) : "";
    weekly.value =
      plan?.weekly_budget_usd != null ? String(plan.weekly_budget_usd) : "";
    monthly.value =
      plan?.monthly_budget_usd != null ? String(plan.monthly_budget_usd) : "";
    lifetime.value =
      plan?.lifetime_budget_usd != null ? String(plan.lifetime_budget_usd) : "";
    dailyTokens.value =
      plan?.daily_token_limit != null ? String(plan.daily_token_limit) : "";
    weeklyTokens.value =
      plan?.weekly_token_limit != null ? String(plan.weekly_token_limit) : "";
    monthlyTokens.value =
      plan?.monthly_token_limit != null ? String(plan.monthly_token_limit) : "";
    lifetimeTokens.value =
      plan?.lifetime_token_limit != null
        ? String(plan.lifetime_token_limit)
        : "";
    budgetMode.value = plan?.budget_mode ?? "off";
    expires.value = toLocalDateTime(plan?.expires_at);
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
    description: description.value.trim(),
    allowed_models: models.value,
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
    expires_at: fromLocalDateTime(expires.value),
  };

  saving.value = true;
  try {
    if (props.plan) {
      await api.updatePlan(props.plan.id, body);
      toast.success(`Plan “${body.name}” updated`);
    } else {
      await api.createPlan(body);
      toast.success(`Plan “${body.name}” created`);
    }
    emit("saved");
    emit("update:open", false);
  } catch (error) {
    toast.error(
      error instanceof ApiError ? error.message : "Failed to save plan",
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
        <DialogTitle>{{ isEdit ? "Edit plan" : "Create plan" }}</DialogTitle>
        <DialogDescription>
          Set the rules once, then apply the plan to any key. Values set on a
          key win over the plan.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="plan-name">Name</Label>
          <Input id="plan-name" v-model="name" placeholder="team-free" />
        </div>

        <div class="grid gap-2">
          <Label for="plan-description">Description</Label>
          <Input
            id="plan-description"
            v-model="description"
            placeholder="What this plan is for"
          />
        </div>

        <div class="grid gap-2">
          <Label>Allowed models</Label>
          <ModelAllowlistInput
            v-model="models"
            :aliases="aliases"
            :combos="combos"
            hint="Empty allows every model. Wildcards: openai/*, *."
          />
        </div>

        <LimitFields
          id-prefix="plan"
          scope="plan"
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
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)"
          >Cancel</Button
        >
        <Button :disabled="saving" @click="save">
          {{ saving ? "Saving…" : isEdit ? "Save changes" : "Create plan" }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
