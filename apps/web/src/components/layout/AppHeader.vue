<script setup lang="ts">
import { KeyRound, LogOut } from "@lucide/vue";
import { computed } from "vue";
import { RouterLink, useRoute, useRouter } from "vue-router";
import { toast } from "vue-sonner";

import Logo from "@/components/Logo.vue";
import ThemeToggle from "@/components/layout/ThemeToggle.vue";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { api } from "@/lib/api";
import { clearAuthStatus } from "@/lib/authState";
import { getSession, setSession } from "@/lib/session";

const route = useRoute();
const router = useRouter();
const pageTitle = computed(() => route.meta.title ?? "Alnair Router");
const signedIn = computed(() => getSession() !== null);
const repoUrl = "https://github.com/xFlawlessDev/alnair-router";

async function signOut(): Promise<void> {
  try {
    await api.logout();
  } catch {
    // The local session is dropped either way.
  }
  setSession(null);
  clearAuthStatus();
  toast.success("Signed out");
  await router.replace({ name: "login" });
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
      <Button as-child variant="ghost" size="sm" class="hidden sm:inline-flex">
        <a
          :href="repoUrl"
          target="_blank"
          rel="noopener noreferrer"
          aria-label="Open the Alnair Router GitHub repository and star it"
          title="Star xFlawlessDev/alnair-router on GitHub"
        >
          <svg
            viewBox="0 0 24 24"
            fill="currentColor"
            aria-hidden="true"
            class="size-4"
          >
            <path
              d="M12 .5C5.37.5 0 5.87 0 12.5c0 5.3 3.44 9.8 8.21 11.39.6.11.82-.26.82-.58 0-.29-.01-1.05-.02-2.06-3.34.73-4.04-1.61-4.04-1.61-.55-1.39-1.34-1.76-1.34-1.76-1.09-.75.08-.73.08-.73 1.21.09 1.84 1.24 1.84 1.24 1.07 1.84 2.81 1.31 3.5 1 .11-.78.42-1.31.76-1.61-2.67-.3-5.47-1.34-5.47-5.95 0-1.31.47-2.38 1.24-3.22-.12-.3-.54-1.52.12-3.18 0 0 1.01-.32 3.3 1.23a11.5 11.5 0 0 1 3-.4c1.02 0 2.05.14 3 .4 2.29-1.55 3.3-1.23 3.3-1.23.66 1.66.24 2.88.12 3.18.77.84 1.24 1.91 1.24 3.22 0 4.62-2.81 5.65-5.49 5.94.43.37.82 1.1.82 2.22 0 1.6-.02 2.9-.02 3.29 0 .32.22.7.83.58A12.01 12.01 0 0 0 24 12.5C24 5.87 18.63.5 12 .5Z"
            />
          </svg>
          <span class="hidden md:inline">Star on GitHub</span>
        </a>
      </Button>
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
        <RouterLink
          to="/login"
          aria-label="Dashboard password"
          title="Dashboard password"
        >
          <KeyRound />
        </RouterLink>
      </Button>
      <ThemeToggle />
    </div>
  </header>
</template>
