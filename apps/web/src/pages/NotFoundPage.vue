<script setup lang="ts">
import { RouterLink } from 'vue-router';
import { Button } from '@/components/ui/button';
</script>

<template>
  <div class="flex min-h-[60vh] flex-col items-center justify-center gap-4 text-center">
    <p class="text-sm font-medium text-primary">404</p>
    <h1 class="text-3xl font-semibold tracking-tight">Page not found</h1>
    <p class="max-w-md text-muted-foreground">The route you requested does not exist in this dashboard.</p>
    <Button as-child>
      <RouterLink to="/">Back to overview</RouterLink>
    </Button>
  </div>
</template>
