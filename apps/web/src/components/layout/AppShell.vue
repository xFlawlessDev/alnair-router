<script setup lang="ts">
import { computed } from "vue";
import { RouterView, useRoute } from "vue-router";

import Logo from "@/components/Logo.vue";
import AppHeader from "@/components/layout/AppHeader.vue";
import AppSidebar from "@/components/layout/AppSidebar.vue";
import ThemeToggle from "@/components/layout/ThemeToggle.vue";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { Toaster } from "@/components/ui/sonner";

const route = useRoute();
const isPublic = computed(() => route.meta.public === true);
</script>

<template>
  <a href="#main-content" class="skip-to-content">Skip to content</a>
  <div
    v-if="isPublic"
    class="paper-workspace flex min-h-svh flex-col bg-background"
  >
    <header
      class="flex min-h-16 shrink-0 flex-wrap items-center gap-2 border-b-2 border-foreground bg-card px-4 py-3 sm:px-6"
    >
      <Logo class="size-6" />
      <span class="font-semibold">Alnair Router</span>
      <span class="text-sm text-muted-foreground"
        >· {{ route.meta.title ?? "My usage" }}</span
      >
      <div class="ml-auto">
        <ThemeToggle />
      </div>
    </header>
    <main
      id="main-content"
      class="editorial-content mx-auto w-full max-w-5xl flex-1 px-4 py-6 sm:px-6 sm:py-10"
      tabindex="-1"
    >
      <RouterView />
    </main>
  </div>

  <SidebarProvider v-else>
    <AppSidebar />
    <SidebarInset class="paper-workspace min-h-svh overflow-hidden md:h-svh">
      <AppHeader />
      <main
        id="main-content"
        class="flex min-h-0 flex-1 flex-col overflow-y-auto"
        tabindex="-1"
      >
        <div
          class="editorial-content mx-auto w-full max-w-7xl px-4 py-6 sm:px-6 lg:px-8 lg:py-8"
        >
          <RouterView />
        </div>
      </main>
    </SidebarInset>
  </SidebarProvider>

  <Toaster position="bottom-right" rich-colors />
</template>

<style scoped>
.skip-to-content {
  position: fixed;
  top: 0.5rem;
  left: 0.5rem;
  z-index: 100;
  transform: translateY(-150%);
  border: 2px solid var(--foreground);
  padding: 0.75rem 1rem;
  background: var(--card);
  color: var(--foreground);
  font-weight: 700;
}
.skip-to-content:focus {
  transform: translateY(0);
}
</style>
