<script setup lang="ts">
import { KeyRound, Waypoints } from '@lucide/vue';
import { ref } from 'vue';
import { RouterLink, useRoute } from 'vue-router';
import { toast } from 'vue-sonner';

import ThemeToggle from '@/components/layout/ThemeToggle.vue';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useAdminToken } from '@/lib/adminToken';

const links = [
  { to: '/', label: 'Overview' },
  { to: '/connections', label: 'Connections' },
  { to: '/aliases', label: 'Aliases' },
  { to: '/combos', label: 'Combos' },
  { to: '/keys', label: 'API Keys' },
  { to: '/usage', label: 'Usage' },
  { to: '/about', label: 'About' },
];

const route = useRoute();
const { token, set } = useAdminToken();

const tokenDialogOpen = ref(false);
const draft = ref('');

function isActive(path: string): boolean {
  return path === '/' ? route.path === '/' : route.path.startsWith(path);
}

function openTokenDialog(): void {
  draft.value = token.value;
  tokenDialogOpen.value = true;
}

function saveToken(): void {
  set(draft.value);
  tokenDialogOpen.value = false;
  toast.success(draft.value.trim() ? 'Admin token saved' : 'Admin token cleared');
}
</script>

<template>
  <header class="sticky top-0 z-10 border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80">
    <div class="mx-auto flex h-16 max-w-7xl items-center justify-between gap-4 px-4 sm:px-6 lg:px-8">
      <RouterLink to="/" class="flex shrink-0 items-center gap-2 font-semibold tracking-tight">
        <span class="flex size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
          <Waypoints class="size-4" />
        </span>
        <span class="hidden sm:inline">Alnair Router</span>
      </RouterLink>

      <nav class="flex min-w-0 items-center gap-1 overflow-x-auto text-sm" aria-label="Main navigation">
        <RouterLink
          v-for="link in links"
          :key="link.to"
          :to="link.to"
          class="rounded-md px-3 py-2 whitespace-nowrap transition-colors"
          :class="
            isActive(link.to)
              ? 'bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
          "
        >
          {{ link.label }}
        </RouterLink>
      </nav>

      <div class="flex shrink-0 items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          aria-label="Configure admin token"
          :title="token ? 'Admin token configured' : 'Configure admin token'"
          @click="openTokenDialog"
        >
          <KeyRound :class="token ? 'text-primary' : ''" />
        </Button>
        <ThemeToggle />
      </div>
    </div>
  </header>

  <Dialog v-model:open="tokenDialogOpen">
    <DialogContent>
      <DialogHeader>
        <DialogTitle>Admin token</DialogTitle>
        <DialogDescription>
          Sent as a bearer token on every <code>/api/*</code> request. Leave empty when the router
          binds loopback without <code>server.admin_token</code>.
        </DialogDescription>
      </DialogHeader>
      <div class="grid gap-2">
        <Label for="admin-token">Bearer token</Label>
        <Input
          id="admin-token"
          v-model="draft"
          type="password"
          autocomplete="off"
          placeholder="ALNAIR_ROUTER__SERVER__ADMIN_TOKEN"
        />
      </div>
      <DialogFooter>
        <Button variant="outline" @click="tokenDialogOpen = false">Cancel</Button>
        <Button @click="saveToken">Save</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
