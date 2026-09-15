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
import { Switch } from '@/components/ui/switch';
import { ApiError, api } from '@/lib/api';
import type { ApiKey, BudgetMode } from '@/types/api';

const props = defineProps<{ open: boolean; apiKey: ApiKey | null }>();
const emit = defineEmits<{ 'update:open': [boolean]; saved: [secret?: string] }>();

const BUDGET_MODES: { value: BudgetMode; label: string }[] = [
  { value: 'off', label: 'Off — no cap' },
  { value: 'warn', label: 'Warn — allow, flag the overspend' },
  { value: 'block', label: 'Block — reject once exhausted' },
];

const name = ref('');
const enabled = ref(true);
const rateLimit = ref('');
const budget = ref('');
const budgetMode = ref<BudgetMode>('off');
const saving = ref(false);

const isEdit = computed(() => props.apiKey !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const key = props.apiKey;
    name.value = key?.name ?? '';
    enabled.value = key ? key.enabled !== 0 : true;
    rateLimit.value =
      key?.rate_limit_per_minute != null ? String(key.rate_limit_per_minute) : '';
    budget.value = key?.monthly_budget_usd != null ? String(key.monthly_budget_usd) : '';
    budgetMode.value = key?.budget_mode ?? 'off';
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
    enabled: enabled.value,
    rate_limit_per_minute: rpm,
    monthly_budget_usd: usd,
    budget_mode: budgetMode.value,
  };

  saving.value = true;
  try {
    if (props.apiKey) {
      await api.updateKey(props.apiKey.id, body);
      toast.success(`Key “${body.name}” updated`);
      emit('saved');
    } else {
      const created = await api.createKey(body);
      toast.success(`Key “${created.key.name}” created`);
      emit('saved', created.secret);
    }
    emit('update:open', false);
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Failed to save key');
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent>
      <DialogHeader>
        <DialogTitle>{{ isEdit ? 'Edit API key' : 'Create API key' }}</DialogTitle>
        <DialogDescription>
          <template v-if="isEdit">Changes apply immediately to new requests.</template>
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

        <div class="flex items-center justify-between gap-4 rounded-md border p-3">
          <div>
            <Label for="key-enabled">Enabled</Label>
            <p class="text-xs text-muted-foreground">Disabled keys are rejected on /v1.</p>
          </div>
          <Switch id="key-enabled" v-model="enabled" />
        </div>

        <div class="grid gap-2">
          <Label for="key-rate-limit">Rate limit (requests/minute)</Label>
          <Input
            id="key-rate-limit"
            v-model="rateLimit"
            type="number"
            min="1"
            placeholder="Blank inherits the server default"
          />
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="key-budget">Monthly budget (USD)</Label>
            <Input
              id="key-budget"
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
          {{ saving ? 'Saving…' : isEdit ? 'Save changes' : 'Create key' }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
