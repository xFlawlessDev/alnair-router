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
import { ApiError, api } from '@/lib/api';
import type { ModelPrice, ModelPriceInput } from '@/types/api';

const props = defineProps<{ open: boolean; price: ModelPrice | null }>();
const emit = defineEmits<{ 'update:open': [boolean]; saved: [] }>();

const model = ref('');
const input = ref('');
const output = ref('');
const cacheRead = ref('');
const cacheWrite = ref('');
const reasoning = ref('');
const saving = ref(false);

const isEdit = computed(() => props.price !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const price = props.price;
    model.value = price?.model ?? '';
    input.value = price ? String(price.input_per_million_usd) : '';
    output.value = price ? String(price.output_per_million_usd) : '';
    cacheRead.value = price?.cache_read_per_million_usd != null ? String(price.cache_read_per_million_usd) : '';
    cacheWrite.value =
      price?.cache_write_per_million_usd != null ? String(price.cache_write_per_million_usd) : '';
    reasoning.value =
      price?.reasoning_per_million_usd != null ? String(price.reasoning_per_million_usd) : '';
  },
);

/** Rate input: blank clears the field, anything else must be a non-negative number. */
function parseRate(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : Number.NaN;
}

async function save(): Promise<void> {
  const name = model.value.trim();
  if (!name) {
    toast.error('Model is required');
    return;
  }

  const rates = {
    input_per_million_usd: parseRate(input.value),
    output_per_million_usd: parseRate(output.value),
    cache_read_per_million_usd: parseRate(cacheRead.value),
    cache_write_per_million_usd: parseRate(cacheWrite.value),
    reasoning_per_million_usd: parseRate(reasoning.value),
  };
  const required = [rates.input_per_million_usd, rates.output_per_million_usd];
  if (required.some((rate) => rate === null || Number.isNaN(rate))) {
    toast.error('Input and output rates are required and must be zero or positive');
    return;
  }
  const optional = [
    rates.cache_read_per_million_usd,
    rates.cache_write_per_million_usd,
    rates.reasoning_per_million_usd,
  ];
  if (optional.some((rate) => Number.isNaN(rate))) {
    toast.error('Cache and reasoning rates must be zero or positive');
    return;
  }

  const body: ModelPriceInput = {
    model: name,
    input_per_million_usd: rates.input_per_million_usd as number,
    output_per_million_usd: rates.output_per_million_usd as number,
    cache_read_per_million_usd: rates.cache_read_per_million_usd,
    cache_write_per_million_usd: rates.cache_write_per_million_usd,
    reasoning_per_million_usd: rates.reasoning_per_million_usd,
  };

  saving.value = true;
  try {
    await api.upsertPricing([body]);
    toast.success(`Price for “${name}” saved`);
    emit('saved');
    emit('update:open', false);
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Failed to save the price');
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent>
      <DialogHeader>
        <DialogTitle>{{ isEdit ? 'Edit price override' : 'Add price override' }}</DialogTitle>
        <DialogDescription>
          Rates are USD per million tokens. Overrides win over crawled catalog rows and survive
          syncs; leave cache or reasoning blank to fall back to the input or output rate.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="price-model">Model id</Label>
          <Input
            id="price-model"
            v-model="model"
            placeholder="claude-sonnet-4-5"
            autocapitalize="off"
            :disabled="isEdit"
          />
          <p class="text-xs text-muted-foreground">
            The upstream model id sent to the provider, e.g. <code>gpt-4o</code>.
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="price-input">Input rate</Label>
            <Input id="price-input" v-model="input" type="number" min="0" step="0.0001" placeholder="2.5" />
          </div>
          <div class="grid gap-2">
            <Label for="price-output">Output rate</Label>
            <Input id="price-output" v-model="output" type="number" min="0" step="0.0001" placeholder="10" />
          </div>
        </div>

        <div class="grid gap-4 sm:grid-cols-3">
          <div class="grid gap-2">
            <Label for="price-cache-read">Cache read</Label>
            <Input
              id="price-cache-read"
              v-model="cacheRead"
              type="number"
              min="0"
              step="0.0001"
              placeholder="= input"
            />
          </div>
          <div class="grid gap-2">
            <Label for="price-cache-write">Cache write</Label>
            <Input
              id="price-cache-write"
              v-model="cacheWrite"
              type="number"
              min="0"
              step="0.0001"
              placeholder="= input"
            />
          </div>
          <div class="grid gap-2">
            <Label for="price-reasoning">Reasoning</Label>
            <Input
              id="price-reasoning"
              v-model="reasoning"
              type="number"
              min="0"
              step="0.0001"
              placeholder="= output"
            />
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">Cancel</Button>
        <Button :disabled="saving" @click="save">{{ saving ? 'Saving…' : 'Save price' }}</Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
