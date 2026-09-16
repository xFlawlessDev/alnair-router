<script setup lang="ts">
import { Activity, Copy, Eye, EyeOff, Loader2, Pencil, Plug, Plus, RefreshCw, Trash2 } from '@lucide/vue';
import { onMounted, ref } from 'vue';
import { toast } from 'vue-sonner';

import ConfirmDialog from '@/components/ConfirmDialog.vue';
import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import ProviderIcon from '@/components/ProviderIcon.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import ConnectionFormDialog from '@/components/connections/ConnectionFormDialog.vue';
import ProviderPickerDialog from '@/components/connections/ProviderPickerDialog.vue';
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
import type { Connection, ProviderPreset } from '@/types/api';

const connections = ref<Connection[]>([]);
const presets = ref<ProviderPreset[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const formOpen = ref(false);
const pickerOpen = ref(false);
const selectedPreset = ref<ProviderPreset | null>(null);
const editing = ref<Connection | null>(null);
const deleting = ref<Connection | null>(null);
const deletingBusy = ref(false);
const revealed = ref<Set<string>>(new Set());
const testingId = ref<string | null>(null);

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [connectionList, providerList] = await Promise.all([
      api.listConnections(),
      api.listProviders(),
    ]);
    connections.value = connectionList;
    presets.value = providerList.data;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load connections';
  } finally {
    loading.value = false;
  }
}

function openCreate(): void {
  selectedPreset.value = null;
  editing.value = null;
  formOpen.value = true;
}

function openPicker(): void {
  pickerOpen.value = true;
}

function onPresetSelected(preset: ProviderPreset): void {
  selectedPreset.value = preset;
  editing.value = null;
  pickerOpen.value = false;
  formOpen.value = true;
}

function openEdit(connection: Connection): void {
  selectedPreset.value = null;
  editing.value = connection;
  formOpen.value = true;
}

function providerLabel(connection: Connection): string {
  if (!connection.provider_id) return '';
  return (
    presets.value.find((preset) => preset.id === connection.provider_id)?.label ??
    connection.provider_id
  );
}

/** Free connection name for a preset, suffixed when the base name is taken. */
function suggestName(preset: ProviderPreset): string {
  const base = preset.label
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');
  const taken = new Set(connections.value.map((connection) => connection.name.toLowerCase()));
  if (!taken.has(base)) return base;

  for (let index = 2; ; index += 1) {
    const candidate = `${base}-${index}`;
    if (!taken.has(candidate)) return candidate;
  }
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
        <Button variant="outline" @click="openCreate"><Plus /> Add connection</Button>
        <Button @click="openPicker"><Plug /> Add provider</Button>
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
        <div class="flex flex-wrap items-center justify-center gap-2">
          <Button @click="openPicker"><Plug /> Add provider</Button>
          <Button variant="outline" @click="openCreate"><Plus /> Add connection</Button>
        </div>
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
                <div class="flex flex-wrap items-center gap-1">
                  <ProviderIcon
                    :id="connection.provider_id"
                    :type="connection.provider_type"
                    :label="providerLabel(connection) || connection.provider_type"
                    class="mr-0.5 text-muted-foreground"
                  />
                  <Badge v-if="connection.provider_id" variant="secondary">
                    {{ providerLabel(connection) }}
                  </Badge>
                  <Badge variant="outline">{{ connection.provider_type }}</Badge>
                  <Badge
                    v-if="connection.account_count"
                    variant="secondary"
                    :title="`${connection.account_count} extra key(s) rotate behind this connection`"
                  >
                    +{{ connection.account_count }} keys
                  </Badge>
                </div>
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

    <ProviderPickerDialog v-model:open="pickerOpen" @select="onPresetSelected" />

    <ConnectionFormDialog
      v-model:open="formOpen"
      :connection="editing"
      :preset="selectedPreset"
      :default-name="selectedPreset ? suggestName(selectedPreset) : ''"
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
