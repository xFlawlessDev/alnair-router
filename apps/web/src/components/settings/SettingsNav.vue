<script setup lang="ts">
import { CircleDot } from "@lucide/vue";
import type { Component } from "vue";

import { cn } from "@/lib/utils";

export interface NavEntry {
  id: string;
  label: string;
  icon: Component;
  /** Unsaved edits live in this section. */
  dirty: boolean;
  /** A stored override exists for this section. */
  customized: boolean;
}

defineProps<{
  entries: NavEntry[];
  active: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{ select: [string] }>();
</script>

<template>
  <nav aria-label="Settings sections" class="lg:sticky lg:top-24">
    <ul
      class="flex gap-1 overflow-x-auto pb-1 lg:flex-col lg:gap-0.5 lg:overflow-visible lg:pb-0"
    >
      <li v-for="entry in entries" :key="entry.id" class="shrink-0 lg:shrink">
        <button
          type="button"
          :disabled="disabled"
          :aria-current="entry.id === active ? 'true' : undefined"
          :class="
            cn(
              'group flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-sm font-medium whitespace-nowrap transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50',
              entry.id === active
                ? 'bg-accent text-accent-foreground'
                : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground',
            )
          "
          @click="emit('select', entry.id)"
        >
          <component :is="entry.icon" class="size-4 shrink-0" />
          <span class="truncate">{{ entry.label }}</span>
          <CircleDot
            v-if="entry.dirty"
            class="ml-auto size-3.5 shrink-0 text-primary"
            aria-label="Unsaved changes"
          />
          <span
            v-else-if="entry.customized"
            class="ml-auto size-1.5 shrink-0 rounded-full bg-muted-foreground/50 lg:mr-1.5"
            title="Customized"
          />
        </button>
      </li>
    </ul>
  </nav>
</template>
