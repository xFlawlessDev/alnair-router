<script setup lang="ts">
import { Check, Copy, KeyRound, Pencil, Plus, RefreshCw, Trash2, TriangleAlert } from '@lucide/vue';
import { onMounted, ref } from 'vue';
import { toast } from 'vue-sonner';

import ConfirmDialog from '@/components/ConfirmDialog.vue';
import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import KeyFormDialog from '@/components/keys/KeyFormDialog.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { ApiError, api } from '@/lib/api';
import { formatDateTime, formatCost, isEnabled } from '@/lib/format';
import type { ApiKey } from '@/types/api';

const keys = ref<ApiKey[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const formOpen = ref(false);
const editing = ref<ApiKey | null>(null);
const secret = ref<string | null>(null);
const copied = ref(false);
const deleting = ref<ApiKey | null>(null);
const deletingBusy = ref(false);

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    keys.value = await api.listKeys();
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load API keys';
  } finally {
    loading.value = false;
  }
}

function openCreate(): void {
  editing.value = null;
  formOpen.value = true;
}

function openEdit(key: ApiKey): void {
  editing.value = key;
  formOpen.value = true;
}

function handleSaved(createdSecret?: string): void {
  if (createdSecret) {
    secret.value = createdSecret;
    copied.value = false;
  }
  load();
}

async function copySecret(): Promise<void> {
  if (!secret.value) return;
  try {
    await navigator.clipboard.writeText(secret.value);
    copied.value = true;
    toast.success('Key copied to clipboard');
  } catch {
    toast.error('Clipboard is unavailable');
  }
}

async function confirmDelete(): Promise<void> {
  if (!deleting.value) return;
  deletingBusy.value = true;
  const name = deleting.value.name;
  try {
    await api.deleteKey(deleting.value.id);
    toast.success(`Key “${name}” deleted`);
    deleting.value = null;
    await load();
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to delete key');
  } finally {
    deletingBusy.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="API Keys"
      description="Router-issued client keys for /v1/*. Only a SHA-256 hash is stored server-side."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button @click="openCreate"><Plus /> Create key</Button>
      </template>
    </PageHeader>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">Loading API keys…</p>

    <EmptyState
      v-else-if="!keys.length"
      title="No API keys yet"
      description="Keys authenticate /v1 requests when server.require_api_key is enabled. The secret is shown once at creation."
    >
      <template #icon><KeyRound class="size-5" /></template>
      <template #action>
        <Button @click="openCreate"><Plus /> Create key</Button>
      </template>
    </EmptyState>

    <Card v-else>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Prefix</TableHead>
            <TableHead>Limits</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Created</TableHead>
            <TableHead>Last used</TableHead>
            <TableHead class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="key in keys" :key="key.id">
            <TableCell class="font-medium">{{ key.name }}</TableCell>
            <TableCell>
              <code class="rounded bg-muted px-1.5 py-0.5 text-xs">{{ key.prefix }}…</code>
            </TableCell>
            <TableCell>
              <div class="flex flex-wrap items-center gap-1">
                <Badge v-if="key.rate_limit_per_minute" variant="secondary">
                  {{ key.rate_limit_per_minute }}/min
                </Badge>
                <Badge
                  v-if="key.monthly_budget_usd && key.budget_mode !== 'off'"
                  :variant="key.budget_mode === 'block' ? 'destructive' : 'outline'"
                >
                  {{ formatCost(key.monthly_budget_usd) }} · {{ key.budget_mode }}
                </Badge>
                <span
                  v-if="!key.rate_limit_per_minute && (!key.monthly_budget_usd || key.budget_mode === 'off')"
                  class="text-muted-foreground"
                >
                  Default
                </span>
              </div>
            </TableCell>
            <TableCell><StatusBadge :enabled="isEnabled(key.enabled)" /></TableCell>
            <TableCell class="text-xs text-muted-foreground">
              {{ formatDateTime(key.created_at) }}
            </TableCell>
            <TableCell class="text-xs text-muted-foreground">
              {{ formatDateTime(key.last_used_at) }}
            </TableCell>
            <TableCell class="text-right">
              <div class="flex justify-end gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="`Edit ${key.name}`"
                  @click="openEdit(key)"
                >
                  <Pencil />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="text-destructive hover:text-destructive"
                  :aria-label="`Delete ${key.name}`"
                  @click="deleting = key"
                >
                  <Trash2 />
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Card>

    <KeyFormDialog v-model:open="formOpen" :api-key="editing" @saved="handleSaved" />

    <Dialog :open="secret !== null" @update:open="secret = null">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Copy your key now</DialogTitle>
          <DialogDescription class="flex items-start gap-2">
            <TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
            This is the only time the secret is visible. Store it somewhere safe.
          </DialogDescription>
        </DialogHeader>
        <div class="flex items-center gap-2">
          <code class="min-w-0 flex-1 overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs">
            {{ secret }}
          </code>
          <Button variant="outline" size="icon" aria-label="Copy key" @click="copySecret">
            <Check v-if="copied" class="text-primary" />
            <Copy v-else />
          </Button>
        </div>
        <DialogFooter>
          <Button @click="secret = null">Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <ConfirmDialog
      :open="deleting !== null"
      title="Delete API key?"
      :description="`Clients using “${deleting?.name}” will immediately receive 401 responses.`"
      :pending="deletingBusy"
      @update:open="deleting = $event ? deleting : null"
      @confirm="confirmDelete"
    />
  </div>
</template>
