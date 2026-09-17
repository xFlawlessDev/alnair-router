<script setup lang="ts">
import { Label } from "@/components/ui/label";

defineProps<{
  title: string;
  /** Plain-text description; use the `description` slot for markup. */
  description?: string;
  /** Makes the whole row clickable by binding the label to the control. */
  controlId?: string;
}>();
</script>

<template>
  <div
    class="rounded-lg border border-border/70 bg-card px-4 py-3.5 transition-colors has-[[data-state=checked]]:border-primary/40 has-[[data-state=checked]]:bg-primary/[0.03]"
  >
    <div class="flex items-start justify-between gap-6">
      <div class="grid min-w-0 gap-1">
        <Label
          v-if="controlId"
          :for="controlId"
          class="cursor-pointer text-sm leading-snug"
        >
          {{ title }}
        </Label>
        <span v-else class="text-sm leading-snug font-medium">{{ title }}</span>

        <p
          v-if="description || $slots.description"
          class="text-xs leading-relaxed text-pretty text-muted-foreground"
        >
          <slot name="description">{{ description }}</slot>
        </p>
      </div>
      <div class="shrink-0 pt-0.5">
        <slot name="control" />
      </div>
    </div>

    <!-- Full-width body for controls that depend on the switch above. -->
    <div v-if="$slots.default" class="mt-3.5 border-t pt-3.5">
      <slot />
    </div>
  </div>
</template>
