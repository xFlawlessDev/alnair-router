<script setup lang="ts">
import { KeyRound, LogOut } from '@lucide/vue';
import { computed } from 'vue';
import { RouterLink, useRoute, useRouter } from 'vue-router';
import { toast } from 'vue-sonner';

import Logo from '@/components/Logo.vue';
import ThemeToggle from '@/components/layout/ThemeToggle.vue';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { SidebarTrigger } from '@/components/ui/sidebar';
import { api } from '@/lib/api';
import { clearAuthStatus } from '@/lib/authState';
import { getSession, setSession } from '@/lib/session';

const route = useRoute();
const router = useRouter();
const pageTitle = computed(() => route.meta.title ?? 'Alnair Router');
const signedIn = computed(() => getSession() !== null);

async function signOut(): Promise<void> {
  try {
    await api.logout();
  } catch {
    // The local session is dropped either way.
  }
  setSession(null);
  clearAuthStatus();
  toast.success('Signed out');
  await router.replace({ name: 'login' });
}
</script>

<template>
  <header
    class="sticky top-0 z-10 flex min-h-16 shrink-0 items-center justify-between gap-2 border-b bg-background/95 px-3 backdrop-blur supports-[backdrop-filter]:bg-background/80 sm:gap-3 sm:px-4 lg:px-6"
  >
    <div class="flex min-w-0 items-center gap-2">
      <SidebarTrigger class="shrink-0" />
      <Separator orientation="vertical" class="h-4" />
      <RouterLink
        to="/"
        class="flex shrink-0 items-center gap-2 md:hidden"
        aria-label="Alnair Router"
      >
        <Logo class="size-6" />
      </RouterLink>
      <span class="truncate text-sm font-medium">{{ pageTitle }}</span>
    </div>

    <div class="flex shrink-0 items-center gap-1">
      <Button
        v-if="signedIn"
        variant="ghost"
        size="icon"
        aria-label="Sign out"
        title="Sign out"
        @click="signOut"
      >
        <LogOut />
      </Button>
      <Button v-else as-child variant="ghost" size="icon">
        <RouterLink to="/login" aria-label="Dashboard password" title="Dashboard password">
          <KeyRound />
        </RouterLink>
      </Button>
      <ThemeToggle />
    </div>
  </header>
</template>
