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
  <Badge :variant="enabled ? 'default' : 'outline'">
    {{ enabled ? enabledLabel : disabledLabel }}
  </Badge>
</template>
