<script setup lang="ts">
defineProps<{
  title: string;
  description?: string;
}>();
</script>

<template>
  <div
    class="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed p-10 text-center"
  >
    <div
      class="flex size-10 items-center justify-center rounded-full bg-muted text-muted-foreground"
    >
      <slot name="icon" />
    </div>
    <div>
      <p class="font-medium">{{ title }}</p>
      <p v-if="description" class="mt-1 max-w-md text-sm text-muted-foreground">
        {{ description }}
      </p>
    </div>
    <slot name="action" />
  </div>
</template>
