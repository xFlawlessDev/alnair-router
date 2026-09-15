<script setup lang="ts">
import { computed } from 'vue';
import { RouterView, useRoute } from 'vue-router';

import Logo from '@/components/Logo.vue';
import AppHeader from '@/components/layout/AppHeader.vue';
import AppSidebar from '@/components/layout/AppSidebar.vue';
import ThemeToggle from '@/components/layout/ThemeToggle.vue';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { Toaster } from '@/components/ui/sonner';

const route = useRoute();
const isPublic = computed(() => route.meta.public === true);
</script>

<template>
  <div v-if="isPublic" class="flex min-h-svh flex-col bg-muted/30">
    <header class="flex h-16 shrink-0 items-center gap-2 border-b bg-background px-4 sm:px-6">
      <Logo class="size-6" />
      <span class="font-semibold">Alnair Router</span>
      <span class="text-sm text-muted-foreground">· {{ route.meta.title ?? 'My usage' }}</span>
      <div class="ml-auto">
        <ThemeToggle />
      </div>
    </header>
    <main class="mx-auto w-full max-w-4xl flex-1 px-4 py-8 sm:px-6">
      <RouterView />
    </main>
  </div>

  <SidebarProvider v-else>
    <AppSidebar />
    <SidebarInset class="min-h-svh overflow-hidden md:h-svh">
      <AppHeader />
      <div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <div class="mx-auto w-full max-w-7xl px-4 py-8 sm:px-6 lg:px-8">
          <RouterView />
        </div>
      </div>
    </SidebarInset>
  </SidebarProvider>

  <Toaster position="bottom-right" rich-colors />
</template>
