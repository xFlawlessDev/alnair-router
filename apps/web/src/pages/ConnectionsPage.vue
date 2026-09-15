<script setup lang="ts">
import { Activity, Copy, Eye, EyeOff, Loader2, Pencil, Plug, Plus, RefreshCw, Trash2 } from '@lucide/vue';
import { onMounted, ref } from 'vue';
import { toast } from 'vue-sonner';

import ConfirmDialog from '@/components/ConfirmDialog.vue';
import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import ConnectionFormDialog from '@/components/connections/ConnectionFormDialog.vue';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Switch } from '@/components/ui/switch';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { ApiError, api } from '@/lib/api';
import { formatDateTime, isEnabled, maskSecret, parseHeaders } from '@/lib/format';
import type { Connection } from '@/types/api';

const connections = ref<Connection[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const formOpen = ref(false);
const editing = ref<Connection | null>(null);
const deleting = ref<Connection | null>(null);
const deletingBusy = ref(false);
const revealed = ref<Set<string>>(new Set());
const testingId = ref<string | null>(null);

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    connections.value = await api.listConnections();
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load connections';
  } finally {
    loading.value = false;
  }
}

function openCreate(): void {
  editing.value = null;
  formOpen.value = true;
}

function openEdit(connection: Connection): void {
  editing.value = connection;
  formOpen.value = true;
}

function headerCount(connection: Connection): number {
  return Object.keys(parseHeaders(connection.custom_headers)).length;
}

function toggleReveal(id: string): void {
  const next = new Set(revealed.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  revealed.value = next;
}

async function copySecret(secret: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(secret);
    toast.success('API key copied');
  } catch {
    toast.error('Clipboard is unavailable');
  }
}

async function toggleEnabled(connection: Connection): Promise<void> {
  try {
    await api.updateConnection(connection.id, { enabled: !isEnabled(connection.enabled) });
    await load();
    toast.success(`Connection “${connection.name}” ${isEnabled(connection.enabled) ? 'disabled' : 'enabled'}`);
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to update connection');
  }
}

async function confirmDelete(): Promise<void> {
  if (!deleting.value) return;
  deletingBusy.value = true;
  const name = deleting.value.name;
  try {
    await api.deleteConnection(deleting.value.id);
    toast.success(`Connection “${name}” deleted`);
    deleting.value = null;
    await load();
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to delete connection');
  } finally {
    deletingBusy.value = false;
  }
}

async function runTest(connection: Connection): Promise<void> {
  testingId.value = connection.id;
  try {
    const result = await api.testConnection(connection.id);
    toast.success(
      `${connection.name} — ${result.message} (${result.latency_ms} ms)`,
    );
  } catch (caught) {
    toast.error(
      `${connection.name} — ${caught instanceof ApiError ? caught.message : 'Test failed'}`,
    );
  } finally {
    testingId.value = null;
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="Connections"
      description="Upstream endpoints the router can dispatch to. Aliases and combos reference these."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button @click="openCreate"><Plus /> Add connection</Button>
      </template>
    </PageHeader>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">Loading connections…</p>

    <EmptyState
      v-else-if="!connections.length"
      title="No connections yet"
      description="Add an OpenAI-compatible or Anthropic-native endpoint to start routing requests."
    >
      <template #icon><Plug class="size-5" /></template>
      <template #action>
        <Button @click="openCreate"><Plus /> Add connection</Button>
      </template>
    </EmptyState>

    <Card v-else>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Base URL</TableHead>
            <TableHead>API key</TableHead>
            <TableHead>Headers</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Enabled</TableHead>
            <TableHead>Updated</TableHead>
            <TableHead class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="connection in connections" :key="connection.id">
            <TableCell>
              <div class="flex flex-col gap-1">
                <span class="font-medium">{{ connection.name }}</span>
                <Badge variant="outline" class="w-fit">{{ connection.provider_type }}</Badge>
              </div>
            </TableCell>
            <TableCell class="max-w-64 truncate font-mono text-xs" :title="connection.base_url">
              {{ connection.base_url }}
            </TableCell>
            <TableCell>
              <div v-if="connection.api_key" class="flex items-center gap-1">
                <code class="text-xs">
                  {{
                    revealed.has(connection.id)
                      ? connection.api_key
                      : maskSecret(connection.api_key)
                  }}
                </code>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="revealed.has(connection.id) ? 'Hide API key' : 'Reveal API key'"
                  @click="toggleReveal(connection.id)"
                >
                  <EyeOff v-if="revealed.has(connection.id)" />
                  <Eye v-else />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Copy API key"
                  @click="copySecret(connection.api_key)"
                >
                  <Copy />
                </Button>
              </div>
              <span v-else class="text-muted-foreground">—</span>
            </TableCell>
            <TableCell>
              <Badge v-if="headerCount(connection)" variant="secondary">
                {{ headerCount(connection) }}
              </Badge>
              <span v-else class="text-muted-foreground">—</span>
            </TableCell>
            <TableCell>
              <StatusBadge :enabled="isEnabled(connection.enabled)" />
            </TableCell>
            <TableCell>
              <Switch
                :model-value="isEnabled(connection.enabled)"
                :aria-label="`Toggle ${connection.name}`"
                @update:model-value="toggleEnabled(connection)"
              />
            </TableCell>
            <TableCell class="text-xs text-muted-foreground">
              {{ formatDateTime(connection.updated_at) }}
            </TableCell>
            <TableCell class="text-right">
              <div class="flex justify-end gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :disabled="testingId === connection.id"
                  :aria-label="`Test ${connection.name}`"
                  :title="`Test ${connection.name} against its /models endpoint`"
                  @click="runTest(connection)"
                >
                  <Loader2 v-if="testingId === connection.id" class="animate-spin" />
                  <Activity v-else />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="`Edit ${connection.name}`"
                  @click="openEdit(connection)"
                >
                  <Pencil />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="text-destructive hover:text-destructive"
                  :aria-label="`Delete ${connection.name}`"
                  @click="deleting = connection"
                >
                  <Trash2 />
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Card>

    <ConnectionFormDialog
      v-model:open="formOpen"
      :connection="editing"
      @saved="load"
    />

    <ConfirmDialog
      :open="deleting !== null"
      title="Delete connection?"
      :description="`“${deleting?.name}” will be removed. Its aliases cascade-delete; combos referencing it may stop resolving.`"
      :pending="deletingBusy"
      @update:open="deleting = $event ? deleting : null"
      @confirm="confirmDelete"
    />
  </div>
</template>
