<script setup lang="ts">
import { RouterView } from 'vue-router';
import { Toaster } from '@/components/ui/sonner';

import AppHeader from './AppHeader.vue';
</script>

<template>
  <div class="min-h-svh bg-background text-foreground">
    <AppHeader />
    <main class="mx-auto w-full max-w-7xl px-4 py-8 sm:px-6 lg:px-8">
      <RouterView />
    </main>
    <Toaster position="bottom-right" rich-colors />
  </div>
</template>
