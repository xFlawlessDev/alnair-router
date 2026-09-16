<script setup lang="ts">
import { RefreshCw } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ApiError, api } from "@/lib/api";
import type { Alias, AliasInput, Connection, UpstreamModel } from "@/types/api";

const props = defineProps<{
  open: boolean;
  alias: Alias | null;
  connections: Connection[];
}>();
const emit = defineEmits<{ "update:open": [boolean]; saved: [] }>();

type OverrideMode = "none" | "list" | "custom";

const prefix = ref("");
const connectionId = ref("");
const overrideMode = ref<OverrideMode>("none");
const selectedModel = ref("");
const modelOverride = ref("");
const sortOrder = ref(0);
const enabled = ref(true);
const saving = ref(false);

const models = ref<UpstreamModel[]>([]);
const loadingModels = ref(false);
const modelsError = ref<string | null>(null);

const isEdit = computed(() => props.alias !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const alias = props.alias;
    prefix.value = alias?.prefix ?? "";
    connectionId.value = alias?.connection_id ?? props.connections[0]?.id ?? "";
    modelOverride.value = alias?.model_override ?? "";
    overrideMode.value = alias?.model_override ? "custom" : "none";
    selectedModel.value = "";
    models.value = [];
    modelsError.value = null;
    sortOrder.value = alias?.sort_order ?? 0;
    enabled.value = alias ? alias.enabled !== 0 : true;
    if (connectionId.value) void loadModels();
  },
);

watch(connectionId, (id) => {
  if (props.open && id) void loadModels();
});

/** Loads upstream models and upgrades a matching custom override to the picker. */
async function loadModels(): Promise<void> {
  loadingModels.value = true;
  modelsError.value = null;
  try {
    const result = await api.listUpstreamModels(connectionId.value);
    models.value = result.models;
    const current = modelOverride.value.trim();
    if (current && result.models.some((model) => model.id === current)) {
      overrideMode.value = "list";
      selectedModel.value = current;
    }
  } catch (caught) {
    models.value = [];
    modelsError.value =
      caught instanceof ApiError ? caught.message : "Failed to load models";
  } finally {
    loadingModels.value = false;
  }
}

function effectiveOverride(): string | null {
  if (overrideMode.value === "list") return selectedModel.value || null;
  if (overrideMode.value === "custom")
    return modelOverride.value.trim() || null;
  return null;
}

async function save(): Promise<void> {
  const trimmedPrefix = prefix.value.trim();
  if (!trimmedPrefix) {
    toast.error("Prefix is required");
    return;
  }
  if (!connectionId.value) {
    toast.error("Select a connection first");
    return;
  }
  if (overrideMode.value === "list" && !selectedModel.value) {
    toast.error("Pick a model, or switch to Any model");
    return;
  }

  const body: AliasInput = {
    prefix: trimmedPrefix,
    connection_id: connectionId.value,
    model_override: effectiveOverride(),
    sort_order: sortOrder.value,
    enabled: enabled.value,
  };

  saving.value = true;
  try {
    if (props.alias) {
      await api.updateAlias(props.alias.id, body);
      toast.success(`Alias “${trimmedPrefix}” updated`);
    } else {
      await api.createAlias(body);
      toast.success(`Alias “${trimmedPrefix}” created`);
    }
    emit("saved");
    emit("update:open", false);
  } catch (error) {
    toast.error(
      error instanceof ApiError ? error.message : "Failed to save alias",
    );
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent>
      <DialogHeader>
        <DialogTitle>{{ isEdit ? "Edit alias" : "New alias" }}</DialogTitle>
        <DialogDescription>
          Maps a model prefix like <code>glm</code> to a connection, so
          <code>glm/glm-4.6</code>
          routes there.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="alias-prefix">Prefix</Label>
          <Input
            id="alias-prefix"
            v-model="prefix"
            placeholder="glm"
            autocapitalize="off"
          />
          <p class="text-xs text-muted-foreground">
            Trimmed and lowercased; no slashes or whitespace.
          </p>
        </div>

        <div class="grid gap-2">
          <Label>Connection</Label>
          <Select v-model="connectionId" :disabled="!connections.length">
            <SelectTrigger class="w-full">
              <SelectValue placeholder="Select a connection" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="connection in connections"
                :key="connection.id"
                :value="connection.id"
              >
                {{ connection.name }}
              </SelectItem>
            </SelectContent>
          </Select>
          <p v-if="!connections.length" class="text-xs text-destructive">
            Create a connection before adding aliases.
          </p>
        </div>

        <div class="grid gap-2">
          <div class="flex items-center justify-between">
            <Label>Model override</Label>
            <Button
              variant="ghost"
              size="sm"
              type="button"
              :disabled="loadingModels || !connectionId"
              @click="loadModels"
            >
              <RefreshCw :class="loadingModels ? 'animate-spin' : ''" />
              {{ models.length ? "Reload models" : "Load models" }}
            </Button>
          </div>
          <Select v-model="overrideMode">
            <SelectTrigger class="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none"
                >Any model — the prefix keeps the caller's model</SelectItem
              >
              <SelectItem v-if="models.length" value="list"
                >Pick from upstream models</SelectItem
              >
              <SelectItem value="custom">Custom model id…</SelectItem>
            </SelectContent>
          </Select>

          <Select v-if="overrideMode === 'list'" v-model="selectedModel">
            <SelectTrigger class="w-full">
              <SelectValue placeholder="Select an upstream model" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="model in models"
                :key="model.id"
                :value="model.id"
              >
                {{ model.id }}
              </SelectItem>
            </SelectContent>
          </Select>

          <Input
            v-if="overrideMode === 'custom'"
            id="alias-model-override"
            v-model="modelOverride"
            placeholder="Optional, e.g. glm-4.6"
          />

          <p v-if="loadingModels" class="text-xs text-muted-foreground">
            Loading models…
          </p>
          <p v-else-if="modelsError" class="text-xs text-destructive">
            {{ modelsError }} — enter the model manually instead.
          </p>
          <p v-else-if="models.length" class="text-xs text-muted-foreground">
            {{ models.length }} models loaded from the connection.
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="alias-sort-order">Sort order</Label>
            <Input
              id="alias-sort-order"
              v-model.number="sortOrder"
              type="number"
            />
          </div>
        </div>

        <div
          class="flex items-center justify-between gap-4 rounded-md border p-3"
        >
          <div>
            <Label for="alias-enabled">Enabled</Label>
            <p class="text-xs text-muted-foreground">
              Disabled aliases do not resolve.
            </p>
          </div>
          <Switch id="alias-enabled" v-model="enabled" />
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)"
          >Cancel</Button
        >
        <Button :disabled="saving || !connections.length" @click="save">
          {{ saving ? "Saving…" : isEdit ? "Save changes" : "Create alias" }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
