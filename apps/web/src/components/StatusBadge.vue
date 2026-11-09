<script setup lang="ts">
import { Badge } from "@/components/ui/badge";

withDefaults(
  defineProps<{
    enabled: boolean;
    enabledLabel?: string;
    disabledLabel?: string;
  }>(),
  { enabledLabel: "Enabled", disabledLabel: "Disabled" },
);
</script>

<template>
  <Badge
    :variant="enabled ? 'default' : 'outline'"
    class="gap-1.5 rounded-sm font-semibold"
  >
    <span aria-hidden="true">{{ enabled ? "+" : "−" }}</span>
    {{ enabled ? enabledLabel : disabledLabel }}
  </Badge>
</template>
