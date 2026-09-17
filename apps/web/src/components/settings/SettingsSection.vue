<script setup lang="ts">
import { computed } from "vue";

import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { useSettingsForm } from "@/lib/settings/context";
import {
  sectionIsCustomized,
  type SettingsSection,
} from "@/lib/settings/sections";

const props = defineProps<{ section: SettingsSection }>();

const { settings } = useSettingsForm();

/** The stored config has a dashboard override for at least one of our keys. */
const customized = computed(() =>
  sectionIsCustomized(settings.value?.overrides ?? [], props.section),
);
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle class="flex flex-wrap items-center gap-2 text-base">
        <slot name="title">{{ section.label }}</slot>
        <Badge v-if="customized" variant="secondary" class="font-normal">
          Customized
        </Badge>
      </CardTitle>
      <CardDescription>
        <slot name="description">{{ section.description }}</slot>
      </CardDescription>
    </CardHeader>
    <CardContent>
      <slot />
    </CardContent>
  </Card>
</template>
