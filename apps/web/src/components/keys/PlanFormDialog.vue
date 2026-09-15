<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { toast } from 'vue-sonner';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import ModelAllowlistInput from '@/components/keys/ModelAllowlistInput.vue';
import { ApiError, api } from '@/lib/api';
import type { Alias, BudgetMode, ComboWithEntries, KeyPlan } from '@/types/api';

const props = defineProps<{
  open: boolean;
  plan: KeyPlan | null;
  aliases: Alias[];
  combos: ComboWithEntries[];
}>();
const emit = defineEmits<{ 'update:open': [boolean]; saved: [] }>();

const BUDGET_MODES: { value: BudgetMode; label: string }[] = [
  { value: 'off', label: 'Off — no cap' },
  { value: 'warn', label: 'Warn — allow, flag the overspend' },
  { value: 'block', label: 'Block — reject once exhausted' },
];

const name = ref('');
const description = ref('');
const models = ref<string[]>([]);
const rateLimit = ref('');
const budget = ref('');
const budgetMode = ref<BudgetMode>('off');
const saving = ref(false);

const isEdit = computed(() => props.plan !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const plan = props.plan;
    name.value = plan?.name ?? '';
    description.value = plan?.description ?? '';
    models.value = plan ? [...plan.allowed_models] : [];
    rateLimit.value =
      plan?.rate_limit_per_minute != null ? String(plan.rate_limit_per_minute) : '';
    budget.value = plan?.monthly_budget_usd != null ? String(plan.monthly_budget_usd) : '';
    budgetMode.value = plan?.budget_mode ?? 'off';
  },
);

function parsePositive(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

async function save(): Promise<void> {
  if (!name.value.trim()) {
    toast.error('Name is required');
    return;
  }
  if (rateLimit.value.trim() && parsePositive(rateLimit.value) === null) {
    toast.error('Rate limit must be a positive number');
    return;
  }
  if (budget.value.trim() && parsePositive(budget.value) === null) {
    toast.error('Budget must be a positive number');
    return;
  }

  const rpm = parsePositive(rateLimit.value);
  const usd = parsePositive(budget.value);
  if (budgetMode.value !== 'off' && usd === null) {
    toast.error('Set a monthly budget before choosing warn or block');
    return;
  }

  const body = {
    name: name.value.trim(),
    description: description.value.trim(),
    allowed_models: models.value,
    rate_limit_per_minute: rpm,
    monthly_budget_usd: usd,
    budget_mode: budgetMode.value,
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
    emit('saved');
    emit('update:open', false);
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Failed to save plan');
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent>
      <DialogHeader>
        <DialogTitle>{{ isEdit ? 'Edit plan' : 'Create plan' }}</DialogTitle>
        <DialogDescription>
          Set the rules once, then apply the plan to any key. Values set on a key win over the plan.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="plan-name">Name</Label>
          <Input id="plan-name" v-model="name" placeholder="team-free" />
        </div>

        <div class="grid gap-2">
          <Label for="plan-description">Description</Label>
          <Input id="plan-description" v-model="description" placeholder="What this plan is for" />
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

        <div class="grid gap-2">
          <Label for="plan-rate-limit">Rate limit (requests/minute)</Label>
          <Input
            id="plan-rate-limit"
            v-model="rateLimit"
            type="number"
            min="1"
            placeholder="Blank leaves the server default"
          />
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="plan-budget">Monthly budget (USD)</Label>
            <Input
              id="plan-budget"
              v-model="budget"
              type="number"
              min="0.01"
              step="0.01"
              placeholder="Blank is uncapped"
            />
          </div>
          <div class="grid gap-2">
            <Label>Budget mode</Label>
            <Select v-model="budgetMode">
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="mode in BUDGET_MODES" :key="mode.value" :value="mode.value">
                  {{ mode.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">Cancel</Button>
        <Button :disabled="saving" @click="save">
          {{ saving ? 'Saving…' : isEdit ? 'Save changes' : 'Create plan' }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
