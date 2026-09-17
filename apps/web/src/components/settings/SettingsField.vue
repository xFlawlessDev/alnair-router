<script setup lang="ts">
import { Label } from "@/components/ui/label";

defineProps<{
  label: string;
  /** Dom id, also used to bind the label to the control. */
  id?: string;
  /** Plain-text helper text; use the `hint` slot for markup. */
  hint?: string;
  /** Renders the label as a plain span, for radio/checkbox groups. */
  asGroup?: boolean;
}>();
</script>

<template>
  <div class="grid gap-1.5">
    <span v-if="asGroup" class="text-sm leading-none font-medium">{{
      label
    }}</span>
    <Label v-else :for="id" class="text-sm">{{ label }}</Label>

    <slot />

    <p
      v-if="hint || $slots.hint"
      class="text-xs leading-relaxed text-pretty text-muted-foreground"
    >
      <slot name="hint">{{ hint }}</slot>
    </p>
  </div>
</template>
