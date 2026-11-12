<script setup lang="ts">
import { ArrowDown, ArrowUp, Plus, Trash2 } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { ApiError, api } from "@/lib/api";
import {
  draftToTier,
  emptyTierDraft,
  tierToDraft,
  type TierDraft,
} from "@/lib/pricingTiers";
import type {
  ModelPrice,
  ModelPriceInput,
  PriceTier,
  UnitPrice,
} from "@/types/api";

const props = defineProps<{ open: boolean; price: ModelPrice | null }>();
const emit = defineEmits<{ "update:open": [boolean]; saved: [] }>();

const model = ref("");
const input = ref("0");
const output = ref("0");
const cacheRead = ref("");
const cacheWrite = ref("");
const reasoning = ref("");
const unit = ref<UnitPrice["unit"] | "">("");
const unitQuantity = ref("1");
const unitRate = ref("");
const tiers = ref<TierDraft[]>([]);
const saving = ref(false);

const isEdit = computed(() => props.price !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const price = props.price;
    model.value = price?.model ?? "";
    unit.value = price?.unit_kind ?? "";
    unitQuantity.value = String(price?.unit_quantity ?? 1);
    unitRate.value =
      price?.unit_price_usd == null ? "" : String(price.unit_price_usd);
    input.value = price ? String(price.input_per_million_usd) : "0";
    output.value = price ? String(price.output_per_million_usd) : "0";
    cacheRead.value =
      price?.cache_read_per_million_usd != null
        ? String(price.cache_read_per_million_usd)
        : "";
    cacheWrite.value =
      price?.cache_write_per_million_usd != null
        ? String(price.cache_write_per_million_usd)
        : "";
    reasoning.value =
      price?.reasoning_per_million_usd != null
        ? String(price.reasoning_per_million_usd)
        : "";
    tiers.value = (price?.tiers ?? []).map(tierToDraft);
  },
  { immediate: true },
);

/** Rate input: blank clears the field, anything else must be a non-negative number. */
function parseRate(value: string | number): number | null {
  const trimmed = String(value).trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : Number.NaN;
}

function addTier(): void {
  tiers.value.push(emptyTierDraft());
}

function removeTier(index: number): void {
  tiers.value.splice(index, 1);
}

/** Rules are evaluated top to bottom, so order is part of the meaning. */
function moveTier(index: number, offset: -1 | 1): void {
  const target = index + offset;
  if (target < 0 || target >= tiers.value.length) return;
  const [draft] = tiers.value.splice(index, 1);
  tiers.value.splice(target, 0, draft);
}

async function save(): Promise<void> {
  const name = model.value.trim();
  if (!name) {
    toast.error("Model is required");
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
    toast.error(
      "Input and output rates are required and must be zero or positive",
    );
    return;
  }
  const optional = [
    rates.cache_read_per_million_usd,
    rates.cache_write_per_million_usd,
    rates.reasoning_per_million_usd,
  ];
  if (optional.some((rate) => Number.isNaN(rate))) {
    toast.error("Cache and reasoning rates must be zero or positive");
    return;
  }

  const quantity = parseRate(unitQuantity.value);
  const unitPrice = parseRate(unitRate.value);
  if (
    unit.value &&
    (quantity === null ||
      !Number.isFinite(quantity) ||
      quantity <= 0 ||
      unitPrice === null ||
      !Number.isFinite(unitPrice))
  ) {
    toast.error("Enter a positive unit quantity and a non-negative unit price");
    return;
  }

  const parsedTiers: PriceTier[] = [];
  for (const [index, draft] of tiers.value.entries()) {
    const parsed = draftToTier(draft);
    if ("error" in parsed) {
      toast.error(`Rule ${index + 1}: ${parsed.error}`);
      return;
    }
    parsedTiers.push(parsed.tier);
  }

  const body: ModelPriceInput = {
    model: name,
    input_per_million_usd: rates.input_per_million_usd as number,
    output_per_million_usd: rates.output_per_million_usd as number,
    cache_read_per_million_usd: rates.cache_read_per_million_usd,
    cache_write_per_million_usd: rates.cache_write_per_million_usd,
    reasoning_per_million_usd: rates.reasoning_per_million_usd,
    tiers: parsedTiers,
    unit_price: unit.value
      ? {
          unit: unit.value,
          quantity: quantity as number,
          price_usd: unitPrice as number,
        }
      : null,
  };

  saving.value = true;
  try {
    await api.upsertPricing([body]);
    toast.success(`Price for “${name}” saved`);
    emit("saved");
    emit("update:open", false);
  } catch (error) {
    toast.error(
      error instanceof ApiError ? error.message : "Failed to save the price",
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
          isEdit ? "Edit price override" : "Add price override"
        }}</DialogTitle>
        <DialogDescription>
          Token rates are USD per million tokens. Overrides take precedence over
          synced rates. Unit rates are saved for reference and are not yet used
          to calculate media usage costs.
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
            The upstream model id sent to the provider, e.g.
            <code>gpt-4o</code>.
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="price-input">Input rate</Label>
            <Input
              id="price-input"
              v-model="input"
              type="number"
              min="0"
              step="0.0001"
              placeholder="2.5"
            />
          </div>
          <div class="grid gap-2">
            <Label for="price-output">Output rate</Label>
            <Input
              id="price-output"
              v-model="output"
              type="number"
              min="0"
              step="0.0001"
              placeholder="10"
            />
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

        <fieldset class="grid gap-3 rounded-md border p-4">
          <legend class="px-1 text-sm font-medium">
            Conditional rules (optional)
          </legend>
          <p class="text-xs text-muted-foreground">
            Rules are checked in order and the <strong>first match wins</strong>
            — that rule's rates bill the entire request. A rule matches only
            when every condition it sets matches. Context compares strictly
            greater than the prompt tokens, cached tokens included, and times
            use the UTC request start. With no rules, the base rates above
            apply.
          </p>

          <div
            v-for="(tier, index) in tiers"
            :key="tier.key"
            class="grid gap-3 rounded-md border bg-muted/30 p-3"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <p class="text-xs font-medium">Rule {{ index + 1 }}</p>
              <div class="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :disabled="index === 0"
                  :aria-label="`Move rule ${index + 1} up`"
                  @click="moveTier(index, -1)"
                >
                  <ArrowUp />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :disabled="index === tiers.length - 1"
                  :aria-label="`Move rule ${index + 1} down`"
                  @click="moveTier(index, 1)"
                >
                  <ArrowDown />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="text-destructive hover:text-destructive"
                  :aria-label="`Remove rule ${index + 1}`"
                  @click="removeTier(index)"
                >
                  <Trash2 />
                </Button>
              </div>
            </div>

            <div class="grid gap-4 sm:grid-cols-3">
              <div class="grid gap-2">
                <Label :for="`tier-context-${index}`"
                  >Context above (tokens)</Label
                >
                <Input
                  :id="`tier-context-${index}`"
                  v-model="tier.contextAbove"
                  type="number"
                  min="0"
                  step="1"
                  placeholder="Any"
                />
              </div>
              <div class="grid gap-2">
                <Label :for="`tier-start-${index}`">UTC start</Label>
                <Input
                  :id="`tier-start-${index}`"
                  v-model="tier.start"
                  type="time"
                  :disabled="!tier.timeWindow"
                />
              </div>
              <div class="grid gap-2">
                <Label :for="`tier-end-${index}`">UTC end</Label>
                <Input
                  :id="`tier-end-${index}`"
                  v-model="tier.end"
                  type="time"
                  :disabled="!tier.timeWindow"
                />
              </div>
            </div>

            <div class="flex items-start gap-2 text-xs text-muted-foreground">
              <Checkbox
                :id="`tier-window-${index}`"
                class="mt-0.5"
                :aria-label="`Limit rule ${index + 1} to a daily UTC window`"
                :model-value="tier.timeWindow"
                @update:model-value="tier.timeWindow = $event === true"
              />
              <span
                class="cursor-pointer select-none"
                @click="tier.timeWindow = !tier.timeWindow"
              >
                Limit to a daily UTC window (start inclusive, end exclusive; a
                start after the end wraps past midnight)
              </span>
            </div>

            <div class="grid gap-4 sm:grid-cols-2">
              <div class="grid gap-2">
                <Label :for="`tier-input-${index}`">Input rate</Label>
                <Input
                  :id="`tier-input-${index}`"
                  v-model="tier.input"
                  type="number"
                  min="0"
                  step="0.0001"
                />
              </div>
              <div class="grid gap-2">
                <Label :for="`tier-output-${index}`">Output rate</Label>
                <Input
                  :id="`tier-output-${index}`"
                  v-model="tier.output"
                  type="number"
                  min="0"
                  step="0.0001"
                />
              </div>
            </div>

            <div class="grid gap-4 sm:grid-cols-3">
              <div class="grid gap-2">
                <Label :for="`tier-cache-read-${index}`">Cache read</Label>
                <Input
                  :id="`tier-cache-read-${index}`"
                  v-model="tier.cacheRead"
                  type="number"
                  min="0"
                  step="0.0001"
                  placeholder="= input"
                />
              </div>
              <div class="grid gap-2">
                <Label :for="`tier-cache-write-${index}`">Cache write</Label>
                <Input
                  :id="`tier-cache-write-${index}`"
                  v-model="tier.cacheWrite"
                  type="number"
                  min="0"
                  step="0.0001"
                  placeholder="= input"
                />
              </div>
              <div class="grid gap-2">
                <Label :for="`tier-reasoning-${index}`">Reasoning</Label>
                <Input
                  :id="`tier-reasoning-${index}`"
                  v-model="tier.reasoning"
                  type="number"
                  min="0"
                  step="0.0001"
                  placeholder="= output"
                />
              </div>
            </div>
          </div>

          <div>
            <Button variant="outline" size="sm" @click="addTier">
              <Plus /> Add rule
            </Button>
          </div>
        </fieldset>

        <fieldset class="grid gap-3 rounded-md border p-4">
          <legend class="px-1 text-sm font-medium">Unit rate (optional)</legend>
          <div class="grid gap-4 sm:grid-cols-3">
            <div class="grid gap-2">
              <Label for="price-unit">Unit</Label>
              <select
                id="price-unit"
                v-model="unit"
                class="border-input bg-background h-9 w-full rounded-md border px-3 text-sm"
              >
                <option value="">None</option>
                <option value="second">Second</option>
                <option value="image">Image</option>
                <option value="character">Character</option>
                <option value="request">Request</option>
              </select>
            </div>
            <div class="grid gap-2">
              <Label for="price-quantity">Units per rate</Label>
              <Input
                id="price-quantity"
                v-model="unitQuantity"
                type="number"
                min="0"
                step="any"
                :disabled="!unit"
              />
            </div>
            <div class="grid gap-2">
              <Label for="price-unit-rate">Price (USD)</Label>
              <Input
                id="price-unit-rate"
                v-model="unitRate"
                type="number"
                min="0"
                step="any"
                :disabled="!unit"
              />
            </div>
          </div>
          <p class="text-xs text-muted-foreground">
            For example, enter 60 seconds for a per-minute rate, or 1,000
            characters for a per-1,000-character rate.
          </p>
        </fieldset>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)"
          >Cancel</Button
        >
        <Button :disabled="saving" @click="save">{{
          saving ? "Saving…" : "Save price"
        }}</Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
