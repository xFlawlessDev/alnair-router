<script setup lang="ts">
import { KeyRound } from '@lucide/vue';
import { computed, ref } from 'vue';
import { RouterLink, useRoute } from 'vue-router';
import { toast } from 'vue-sonner';

import Logo from '@/components/Logo.vue';
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
import { Separator } from '@/components/ui/separator';
import { SidebarTrigger } from '@/components/ui/sidebar';
import { useAdminToken } from '@/lib/adminToken';

const route = useRoute();
const pageTitle = computed(() => route.meta.title ?? 'Alnair Router');
const { token, set } = useAdminToken();

const tokenDialogOpen = ref(false);
const draft = ref('');

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
  <header
    class="sticky top-0 z-10 flex min-h-16 shrink-0 items-center justify-between gap-2 border-b bg-background/95 px-3 backdrop-blur supports-[backdrop-filter]:bg-background/80 sm:gap-3 sm:px-4 lg:px-6"
  >
    <div class="flex min-w-0 items-center gap-2">
      <SidebarTrigger class="shrink-0" />
      <Separator orientation="vertical" class="h-4" />
      <RouterLink to="/" class="flex shrink-0 items-center gap-2 md:hidden" aria-label="Alnair Router">
        <Logo class="size-6" />
      </RouterLink>
      <span class="truncate text-sm font-medium">{{ pageTitle }}</span>
    </div>

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
