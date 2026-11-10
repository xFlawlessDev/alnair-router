<script setup lang="ts">
import {
  Check,
  Copy,
  Layers,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
} from "@lucide/vue";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { toast } from "vue-sonner";

import ConfirmDialog from "@/components/ConfirmDialog.vue";
import CapabilityPanel from "@/components/CapabilityPanel.vue";
import EmptyState from "@/components/EmptyState.vue";
import PageHeader from "@/components/PageHeader.vue";
import StatusBadge from "@/components/StatusBadge.vue";
import ComboFormDialog from "@/components/combos/ComboFormDialog.vue";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { ApiError, api } from "@/lib/api";
import { comboTargets, useCapabilities } from "@/lib/capabilities";
import { formatDateTime, isEnabled } from "@/lib/format";
import type { Alias, ComboWithEntries, Connection } from "@/types/api";

const combos = ref<ComboWithEntries[]>([]);
const aliases = ref<Alias[]>([]);
const connections = ref<Connection[]>([]);
const {
  catalog: capabilities,
  error: capabilityError,
  syncing: capabilitySyncing,
  load: loadCapabilities,
  sync: syncCapabilities,
  initialize: initializeCapabilities,
} = useCapabilities();
const loading = ref(true);
const error = ref<string | null>(null);
const formOpen = ref(false);
const editing = ref<ComboWithEntries | null>(null);
const deleting = ref<ComboWithEntries | null>(null);
const deletingBusy = ref(false);
const copiedId = ref<string | null>(null);
let copiedTimer: ReturnType<typeof setTimeout> | null = null;

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [comboList, aliasList, connectionList] = await Promise.all([
      api.listCombos(),
      api.listAliases(),
      api.listConnections(),
    ]);
    combos.value = comboList;
    aliases.value = aliasList;
    connections.value = connectionList;
  } catch (caught) {
    error.value =
      caught instanceof ApiError ? caught.message : "Failed to load combos";
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

async function copyCombo(combo: ComboWithEntries): Promise<void> {
  const value = combo.combo.name;
  try {
    await navigator.clipboard.writeText(value);
    copiedId.value = combo.combo.id;
    if (copiedTimer) clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copiedId.value = null), 1500);
    toast.success(`Copied “${value}”`);
  } catch {
    toast.error("Clipboard is unavailable");
  }
}

async function toggleEnabled(combo: ComboWithEntries): Promise<void> {
  const enabled = !isEnabled(combo.combo.enabled);
  try {
    await api.updateCombo(combo.combo.id, { enabled });
    await load();
    toast.success(
      `Combo “${combo.combo.name}” ${enabled ? "enabled" : "disabled"}`,
    );
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to update combo",
    );
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
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to delete combo",
    );
  } finally {
    deletingBusy.value = false;
  }
}

onMounted(() => {
  void load();
  void initializeCapabilities();
});
onBeforeUnmount(() => {
  if (copiedTimer) clearTimeout(copiedTimer);
});
</script>

<template>
  <div class="editorial-page">
    <PageHeader
      title="Combos"
      description="Named fallback chains. Each tier is tried in order until one emits content."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button
          variant="outline"
          :disabled="capabilitySyncing"
          @click="syncCapabilities"
          >{{
            capabilitySyncing ? "Syncing capabilities…" : "Sync capabilities"
          }}</Button
        >
        <Button @click="openCreate"><Plus /> New combo</Button>
      </template>
    </PageHeader>
    <p v-if="capabilityError" class="text-sm text-destructive">
      {{ capabilityError }}
    </p>
    <p
      v-else-if="capabilities.updated_at"
      class="text-xs text-muted-foreground"
    >
      Model capabilities from models.dev · updated
      {{ formatDateTime(capabilities.updated_at) }}
    </p>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{
        error
      }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">
      Loading combos…
    </p>

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

    <div v-else class="editorial-directory grid gap-3">
      <article
        v-for="(combo, index) in combos"
        :key="combo.combo.id"
        class="editorial-panel grid gap-4 border p-4 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"
      >
        <div class="min-w-0">
          <p class="editorial-kicker mb-3 border-b pb-2">
            Combo / {{ String(index + 1).padStart(2, "0") }}
          </p>
          <div class="flex flex-wrap items-center gap-2">
            <code class="break-all text-base font-bold">{{
              combo.combo.name
            }}</code>
            <Button
              variant="ghost"
              size="icon-sm"
              :aria-label="`Copy ${combo.combo.name}`"
              @click="copyCombo(combo)"
              ><Check
                v-if="copiedId === combo.combo.id"
                class="text-primary" /><Copy v-else
            /></Button>
            <StatusBadge :enabled="isEnabled(combo.combo.enabled)" />
          </div>
          <p
            v-if="combo.combo.description"
            class="mt-2 break-words text-sm text-muted-foreground"
          >
            {{ combo.combo.description }}
          </p>
          <p class="mt-3 text-xs text-muted-foreground">
            Updated {{ formatDateTime(combo.combo.updated_at) }}
          </p>
          <CapabilityPanel
            class="mt-3"
            scope="combo"
            :id="combo.combo.id"
            :targets="comboTargets(combo, combos, aliases, connections)"
            :catalog="capabilities"
            @changed="loadCapabilities"
          />
        </div>
        <div class="min-w-0">
          <p class="editorial-kicker mb-2">Fallback order</p>
          <ol v-if="combo.entries.length" class="grid gap-1">
            <li
              v-for="(entry, index) in combo.entries"
              :key="entry.id"
              class="flex items-start gap-2 border-b py-1 text-sm"
            >
              <span class="font-bold tabular-nums">{{
                String(index + 1).padStart(2, "0")
              }}</span>
              <code class="min-w-0 break-all">{{ entry.model_ref }}</code>
            </li>
          </ol>
          <span v-else class="text-sm text-muted-foreground">No tiers</span>
        </div>
        <div
          class="flex items-center gap-2 border-t pt-3 md:flex-col md:items-end md:border-t-0 md:pt-0"
        >
          <Switch
            :model-value="isEnabled(combo.combo.enabled)"
            :aria-label="`Toggle ${combo.combo.name}`"
            @update:model-value="toggleEnabled(combo)"
          />
          <div class="flex gap-1">
            <Button
              variant="ghost"
              size="icon-sm"
              :aria-label="`Edit ${combo.combo.name}`"
              @click="openEdit(combo)"
              ><Pencil
            /></Button>
            <Button
              variant="ghost"
              size="icon-sm"
              class="text-destructive hover:text-destructive"
              :aria-label="`Delete ${combo.combo.name}`"
              @click="deleting = combo"
              ><Trash2
            /></Button>
          </div>
        </div>
      </article>
    </div>

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
