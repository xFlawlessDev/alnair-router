<script setup lang="ts">
import { Layers, Pencil, Plus, RefreshCw, Trash2 } from '@lucide/vue';
import { onMounted, ref } from 'vue';
import { toast } from 'vue-sonner';

import ConfirmDialog from '@/components/ConfirmDialog.vue';
import EmptyState from '@/components/EmptyState.vue';
import PageHeader from '@/components/PageHeader.vue';
import StatusBadge from '@/components/StatusBadge.vue';
import ComboFormDialog from '@/components/combos/ComboFormDialog.vue';
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
import { formatDateTime, isEnabled } from '@/lib/format';
import type { Alias, ComboWithEntries } from '@/types/api';

const combos = ref<ComboWithEntries[]>([]);
const aliases = ref<Alias[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const formOpen = ref(false);
const editing = ref<ComboWithEntries | null>(null);
const deleting = ref<ComboWithEntries | null>(null);
const deletingBusy = ref(false);

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [comboList, aliasList] = await Promise.all([api.listCombos(), api.listAliases()]);
    combos.value = comboList;
    aliases.value = aliasList;
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught.message : 'Failed to load combos';
  } finally {
    loading.value = false;
  }
}

function openCreate(): void {
  editing.value = null;
  formOpen.value = true;
}

function openEdit(combo: ComboWithEntries): void {
  editing.value = combo;
  formOpen.value = true;
}

async function toggleEnabled(combo: ComboWithEntries): Promise<void> {
  const enabled = !isEnabled(combo.combo.enabled);
  try {
    await api.updateCombo(combo.combo.id, { enabled });
    await load();
    toast.success(`Combo “${combo.combo.name}” ${enabled ? 'enabled' : 'disabled'}`);
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to update combo');
  }
}

async function confirmDelete(): Promise<void> {
  if (!deleting.value) return;
  deletingBusy.value = true;
  const name = deleting.value.combo.name;
  try {
    await api.deleteCombo(deleting.value.combo.id);
    toast.success(`Combo “${name}” deleted`);
    deleting.value = null;
    await load();
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : 'Failed to delete combo');
  } finally {
    deletingBusy.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="Combos"
      description="Named fallback chains. Each tier is tried in order until one emits content."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button @click="openCreate"><Plus /> New combo</Button>
      </template>
    </PageHeader>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{ error }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">Loading combos…</p>

    <EmptyState
      v-else-if="!combos.length"
      title="No combos yet"
      description="A combo is a named fallback chain: requests asking for its name try each tier in order."
    >
      <template #icon><Layers class="size-5" /></template>
      <template #action>
        <Button @click="openCreate"><Plus /> New combo</Button>
      </template>
    </EmptyState>

    <Card v-else>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Fallback tiers</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Enabled</TableHead>
            <TableHead>Updated</TableHead>
            <TableHead class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="combo in combos" :key="combo.combo.id">
            <TableCell>
              <div class="flex flex-col gap-1">
                <code class="text-sm font-medium">{{ combo.combo.name }}</code>
                <span v-if="combo.combo.description" class="max-w-64 text-xs text-muted-foreground">
                  {{ combo.combo.description }}
                </span>
              </div>
            </TableCell>
            <TableCell>
              <div v-if="combo.entries.length" class="flex flex-wrap items-center gap-1">
                <template v-for="(entry, index) in combo.entries" :key="entry.id">
                  <Badge variant="secondary" class="font-mono text-xs">
                    {{ index + 1 }} · {{ entry.model_ref }}
                  </Badge>
                </template>
              </div>
              <span v-else class="text-xs text-muted-foreground">No tiers</span>
            </TableCell>
            <TableCell><StatusBadge :enabled="isEnabled(combo.combo.enabled)" /></TableCell>
            <TableCell>
              <Switch
                :model-value="isEnabled(combo.combo.enabled)"
                :aria-label="`Toggle ${combo.combo.name}`"
                @update:model-value="toggleEnabled(combo)"
              />
            </TableCell>
            <TableCell class="text-xs text-muted-foreground">
              {{ formatDateTime(combo.combo.updated_at) }}
            </TableCell>
            <TableCell class="text-right">
              <div class="flex justify-end gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="`Edit ${combo.combo.name}`"
                  @click="openEdit(combo)"
                >
                  <Pencil />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="text-destructive hover:text-destructive"
                  :aria-label="`Delete ${combo.combo.name}`"
                  @click="deleting = combo"
                >
                  <Trash2 />
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Card>

    <ComboFormDialog
      v-model:open="formOpen"
      :combo="editing"
      :aliases="aliases"
      :combos="combos"
      @saved="load"
    />

    <ConfirmDialog
      :open="deleting !== null"
      title="Delete combo?"
      :description="`Requests for “${deleting?.combo.name}” will stop resolving and fail with a 404.`"
      :pending="deletingBusy"
      @update:open="deleting = $event ? deleting : null"
      @confirm="confirmDelete"
    />
  </div>
</template>
