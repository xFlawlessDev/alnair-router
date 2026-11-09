<script setup lang="ts">
defineProps<{
  title: string;
  description?: string;
}>();
</script>

<template>
  <div
    class="editorial-panel flex flex-col items-center justify-center gap-4 border-2 border-dashed px-5 py-12 text-center"
  >
    <div
      class="editorial-sticker editorial-sticker--accent flex size-14 items-center justify-center"
    >
      <slot name="icon" />
    </div>
    <div>
      <p class="editorial-section-title">{{ title }}</p>
      <p v-if="description" class="mt-1 max-w-md text-sm text-muted-foreground">
        {{ description }}
      </p>
    </div>
    <slot name="action" />
  </div>
</template>
