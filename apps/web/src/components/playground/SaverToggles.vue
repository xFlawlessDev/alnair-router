<script setup lang="ts">
import { CircleAlert, CircleCheck, RotateCcw } from "@lucide/vue";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { SaverToggle } from "@/lib/playground";

defineProps<{
  toggles: SaverToggle[];
  /** How many switches differ from the saved configuration. */
  overrideCount: number;
}>();

const emit = defineEmits<{ toggle: [string]; reset: [] }>();
</script>

<template>
  <div class="grid gap-3">
    <div class="flex flex-wrap gap-2">
      <Button
        v-for="entry in toggles"
        :key="entry.key"
        :variant="entry.on ? 'default' : 'outline'"
        size="sm"
        :aria-pressed="entry.on"
        @click="emit('toggle', entry.key)"
      >
        <CircleCheck v-if="entry.on" class="size-3.5" />
        <CircleAlert v-else class="size-3.5" />
        {{ entry.label }}
      </Button>
    </div>
    <div v-if="overrideCount" class="flex items-center gap-3">
      <Badge variant="outline">{{ overrideCount }} unsaved change(s)</Badge>
      <Button variant="ghost" size="sm" @click="emit('reset')">
        <RotateCcw class="size-3.5" />
        Use saved settings
      </Button>
    </div>
  </div>
</template>
