<script setup lang="ts">
import {
  BookOpen,
  CircleDollarSign,
  FlaskConical,
  History,
  KeyRound,
  Layers,
  LayoutDashboard,
  LogOut,
  Plug,
  ScrollText,
  Settings,
  Sparkles,
  Tags,
  Terminal,
} from "@lucide/vue";
import { computed, type Component } from "vue";
import { RouterLink, useRoute, useRouter } from "vue-router";
import { toast } from "vue-sonner";

import Logo from "@/components/Logo.vue";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
  SidebarSeparator,
} from "@/components/ui/sidebar";
import { api } from "@/lib/api";
import { clearAuthStatus } from "@/lib/authState";
import { getSession, setSession } from "@/lib/session";

interface NavItem {
  label: string;
  to: string;
  icon: Component;
}

const monitorItems: NavItem[] = [
  { label: "Overview", to: "/", icon: LayoutDashboard },
  { label: "Usage", to: "/usage", icon: ScrollText },
  { label: "Token Saving", to: "/token-saver", icon: Sparkles },
  { label: "Playground", to: "/playground", icon: FlaskConical },
  { label: "Console", to: "/logs", icon: Terminal },
];

const manageItems: NavItem[] = [
  { label: "Connections", to: "/connections", icon: Plug },
  { label: "Aliases", to: "/aliases", icon: Tags },
  { label: "Combos", to: "/combos", icon: Layers },
  { label: "API Keys", to: "/keys", icon: KeyRound },
  { label: "Pricing", to: "/pricing", icon: CircleDollarSign },
  { label: "Settings", to: "/settings", icon: Settings },
];

const route = useRoute();
const router = useRouter();
const isActive = (path: string): boolean => route.path === path;
const signedIn = computed(() => getSession() !== null);

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
  <Sidebar collapsible="icon">
    <SidebarHeader>
      <SidebarMenu>
        <SidebarMenuItem>
          <SidebarMenuButton size="lg" as-child tooltip="Alnair Router">
            <RouterLink to="/">
              <Logo class="size-8 shrink-0" />
              <span
                class="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden"
              >
                <span class="truncate font-semibold">Alnair Router</span>
                <span class="truncate text-xs text-muted-foreground"
                  >Admin dashboard</span
                >
              </span>
            </RouterLink>
          </SidebarMenuButton>
        </SidebarMenuItem>
      </SidebarMenu>
    </SidebarHeader>

    <SidebarContent>
      <SidebarGroup>
        <SidebarGroupLabel>Monitor</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            <SidebarMenuItem v-for="item in monitorItems" :key="item.to">
              <SidebarMenuButton
                as-child
                :is-active="isActive(item.to)"
                :tooltip="item.label"
              >
                <RouterLink :to="item.to">
                  <component :is="item.icon" />
                  <span>{{ item.label }}</span>
                </RouterLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>

      <SidebarGroup>
        <SidebarGroupLabel>Manage</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            <SidebarMenuItem v-for="item in manageItems" :key="item.to">
              <SidebarMenuButton
                as-child
                :is-active="isActive(item.to)"
                :tooltip="item.label"
              >
                <RouterLink :to="item.to">
                  <component :is="item.icon" />
                  <span>{{ item.label }}</span>
                </RouterLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
    </SidebarContent>

    <SidebarFooter>
      <SidebarMenu>
        <SidebarMenuItem>
          <SidebarMenuButton
            as-child
            :is-active="isActive('/changelog')"
            tooltip="Changelog"
          >
            <RouterLink to="/changelog">
              <History />
              <span>Changelog</span>
            </RouterLink>
          </SidebarMenuButton>
        </SidebarMenuItem>
        <SidebarMenuItem>
          <SidebarMenuButton
            as-child
            :is-active="isActive('/guide')"
            tooltip="API Guide"
          >
            <RouterLink to="/guide">
              <BookOpen />
              <span>API Guide</span>
            </RouterLink>
          </SidebarMenuButton>
        </SidebarMenuItem>
        <SidebarSeparator />
        <SidebarMenuItem v-if="signedIn">
          <SidebarMenuButton tooltip="Sign out" @click="signOut">
            <LogOut />
            <span>Sign out</span>
          </SidebarMenuButton>
        </SidebarMenuItem>
      </SidebarMenu>
    </SidebarFooter>

    <SidebarRail />
  </Sidebar>
</template>
