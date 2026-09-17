<script setup lang="ts">
import { cn } from "@/lib/utils";

export interface SegmentOption {
  value: string;
  label: string;
}

const props = defineProps<{
  modelValue: string;
  options: SegmentOption[];
  disabled?: boolean;
  /** Accessible name for the group. */
  groupLabel: string;
}>();

const emit = defineEmits<{ "update:modelValue": [string] }>();

function pick(value: string): void {
  if (props.disabled || value === props.modelValue) return;
  emit("update:modelValue", value);
}
</script>

<template>
  <div
    role="radiogroup"
    :aria-label="groupLabel"
    class="inline-flex items-center gap-1 rounded-lg bg-muted p-1"
  >
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      role="radio"
      :aria-checked="option.value === modelValue"
      :disabled="disabled"
      :class="
        cn(
          'rounded-md px-3 py-1 text-sm font-medium transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50',
          option.value === modelValue
            ? 'bg-background text-foreground shadow-sm'
            : 'text-muted-foreground hover:text-foreground',
        )
      "
      @click="pick(option.value)"
    >
      {{ option.label }}
    </button>
  </div>
</template>
