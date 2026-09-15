<script setup lang="ts">
import { RouterView } from 'vue-router';

import AppHeader from '@/components/layout/AppHeader.vue';
import AppSidebar from '@/components/layout/AppSidebar.vue';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { Toaster } from '@/components/ui/sonner';
</script>

<template>
  <SidebarProvider>
    <AppSidebar />
    <SidebarInset class="min-h-svh overflow-hidden md:h-svh">
      <AppHeader />
      <div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <div class="mx-auto w-full max-w-7xl px-4 py-8 sm:px-6 lg:px-8">
          <RouterView />
        </div>
      </div>
      <Toaster position="bottom-right" rich-colors />
    </SidebarInset>
  </SidebarProvider>
</template>
