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
  <nav aria-label="Settings sections" class="min-w-0 lg:sticky lg:top-24">
    <ul
      class="flex gap-2 overflow-x-auto border-b border-border pb-3 lg:flex-col lg:gap-1 lg:border-b-0 lg:border-r lg:pb-0 lg:pr-4"
    >
      <li v-for="entry in entries" :key="entry.id" class="shrink-0 lg:shrink">
        <button
          type="button"
          :disabled="disabled"
          :aria-current="entry.id === active ? 'true' : undefined"
          :class="
            cn(
              'group flex min-h-11 w-full items-center gap-2.5 rounded-md px-3 py-2 text-sm font-bold whitespace-nowrap transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50',
              entry.id === active
                ? 'bg-primary text-primary-foreground'
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
