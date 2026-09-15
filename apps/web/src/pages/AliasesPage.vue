<script setup lang="ts">
import { Activity, ListPlus, Loader2, MessageSquare, Pencil, Plus, RefreshCw, Tags, Trash2 } from '@lucide/vue';
import { computed, onMounted, ref } from 'vue';
import { toast } from 'vue-sonner';

import ConfirmDialog from '@/components/ConfirmDialog.vue';
import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import AliasFormDialog from '@/components/aliases/AliasFormDialog.vue';
import AliasChatDialog from '@/components/aliases/AliasChatDialog.vue';
import ImportAliasesDialog from '@/components/aliases/ImportAliasesDialog.vue';
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
import { formatDateTime, isEnabled } from '@/lib/format';
import type { Alias, Connection } from '@/types/api';

const aliases = ref<Alias[]>([]);
const connections = ref<Connection[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const formOpen = ref(false);
const importOpen = ref(false);
const editing = ref<Alias | null>(null);
const deleting = ref<Alias | null>(null);
const deletingBusy = ref(false);
const testingId = ref<string | null>(null);
const chatOpen = ref(false);
const chatAlias = ref<Alias | null>(null);

const connectionNames = computed(() => {
  const names = new Map<string, string>();
  for (const connection of connections.value) names.set(connection.id, connection.name);
  return names;
});

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [aliasList, connectionList] = await Promise.all([api.listAliases(), api.listConnections()]);
    aliases.value = aliasList;
    connections.value = connectionList;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load aliases';
  } finally {
    loading.value = false;
  }
}

function openCreate(): void {
  editing.value = null;
  formOpen.value = true;
}

function openEdit(alias: Alias): void {
  editing.value = alias;
  formOpen.value = true;
}

async function toggleEnabled(alias: Alias): Promise<void> {
  try {
    await api.updateAlias(alias.id, { enabled: !isEnabled(alias.enabled) });
    await load();
    toast.success(`Alias “${alias.prefix}” ${isEnabled(alias.enabled) ? 'disabled' : 'enabled'}`);
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to update alias');
  }
}

async function confirmDelete(): Promise<void> {
  if (!deleting.value) return;
  deletingBusy.value = true;
  const prefix = deleting.value.prefix;
  try {
    await api.deleteAlias(deleting.value.id);
    toast.success(`Alias “${prefix}” deleted`);
    deleting.value = null;
    await load();
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to delete alias');
  } finally {
    deletingBusy.value = false;
  }
}

function openChat(alias: Alias): void {
  chatAlias.value = alias;
  chatOpen.value = true;
}

async function runTest(alias: Alias): Promise<void> {
  testingId.value = alias.id;
  try {
    const result = await api.testAlias(alias.id);
    if (result.ok) toast.success(`${alias.prefix}/ — ${result.message}`);
    else toast.warning(`${alias.prefix}/ — ${result.message}`);
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Alias test failed');
  } finally {
    testingId.value = null;
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="Aliases"
      description="Prefixes that map model references like glm/glm-4.6 to a connection."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button
          variant="outline"
          :disabled="!connections.length"
          title="Fetch the connection's /models and create aliases in bulk"
          @click="importOpen = true"
        >
          <ListPlus /> Import models
        </Button>
        <Button @click="openCreate"><Plus /> Add alias</Button>
      </template>
    </PageHeader>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">Loading aliases…</p>

    <EmptyState
      v-else-if="!aliases.length"
      title="No aliases yet"
      description="An alias turns a short prefix into a connection. Without one, references fall back to bare-model resolution."
    >
      <template #icon><Tags class="size-5" /></template>
      <template #action>
        <Button :disabled="!connections.length" @click="openCreate">
          <Plus /> Add alias
        </Button>
        <p v-if="!connections.length" class="text-xs text-muted-foreground">
          Create a connection first.
        </p>
      </template>
    </EmptyState>

    <Card v-else>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Prefix</TableHead>
            <TableHead>Connection</TableHead>
            <TableHead>Model override</TableHead>
            <TableHead>Sort order</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Enabled</TableHead>
            <TableHead>Updated</TableHead>
            <TableHead class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="alias in aliases" :key="alias.id">
            <TableCell>
              <code class="rounded bg-muted px-1.5 py-0.5 text-xs">{{ alias.prefix }}/</code>
            </TableCell>
            <TableCell>
              {{ connectionNames.get(alias.connection_id) ?? '(missing connection)' }}
            </TableCell>
            <TableCell>
              <code v-if="alias.model_override" class="text-xs">{{ alias.model_override }}</code>
              <span v-else class="text-muted-foreground">—</span>
            </TableCell>
            <TableCell class="text-muted-foreground">{{ alias.sort_order }}</TableCell>
            <TableCell><StatusBadge :enabled="isEnabled(alias.enabled)" /></TableCell>
            <TableCell>
              <Switch
                :model-value="isEnabled(alias.enabled)"
                :aria-label="`Toggle ${alias.prefix}`"
                @update:model-value="toggleEnabled(alias)"
              />
            </TableCell>
            <TableCell class="text-xs text-muted-foreground">
              {{ formatDateTime(alias.updated_at) }}
            </TableCell>
            <TableCell class="text-right">
              <div class="flex justify-end gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="`Chat test ${alias.prefix}`"
                  :title="`Run a real completion through ${alias.prefix}/`"
                  @click="openChat(alias)"
                >
                  <MessageSquare />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :disabled="testingId === alias.id"
                  :aria-label="`Test ${alias.prefix}`"
                  :title="`Test ${alias.prefix}`"
                  @click="runTest(alias)"
                >
                  <Loader2 v-if="testingId === alias.id" class="animate-spin" />
                  <Activity v-else />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="`Edit ${alias.prefix}`"
                  @click="openEdit(alias)"
                >
                  <Pencil />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="text-destructive hover:text-destructive"
                  :aria-label="`Delete ${alias.prefix}`"
                  @click="deleting = alias"
                >
                  <Trash2 />
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Card>

    <AliasFormDialog
      v-model:open="formOpen"
      :alias="editing"
      :connections="connections"
      @saved="load"
    />

    <ImportAliasesDialog
      v-model:open="importOpen"
      :connections="connections"
      :existing-prefixes="aliases.map((alias) => alias.prefix)"
      @saved="load"
    />

    <AliasChatDialog v-model:open="chatOpen" :alias="chatAlias" />

    <ConfirmDialog
      :open="deleting !== null"
      title="Delete alias?"
      :description="`Requests using “${deleting?.prefix}/…” will stop resolving until another prefix or combo matches.`"
      :pending="deletingBusy"
      @update:open="deleting = $event ? deleting : null"
      @confirm="confirmDelete"
    />
  </div>
</template>
