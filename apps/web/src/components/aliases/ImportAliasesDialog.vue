<script setup lang="ts">
import { RefreshCw, Search } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { ApiError, api } from "@/lib/api";
import { fallbackPrefix, modelPrefixSlug, uniquePrefix } from "@/lib/aliases";
import type { Connection, UpstreamModel } from "@/types/api";

const props = defineProps<{
  open: boolean;
  connections: Connection[];
  existingPrefixes: string[];
}>();
const emit = defineEmits<{ "update:open": [boolean]; saved: [] }>();

const connectionId = ref("");
const models = ref<UpstreamModel[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const search = ref("");
const selected = ref<Set<string>>(new Set());
const importing = ref(false);

const existing = computed(
  () => new Set(props.existingPrefixes.map((p) => p.toLowerCase())),
);

const filtered = computed(() => {
  const term = search.value.trim().toLowerCase();
  if (!term) return models.value;
  return models.value.filter(
    (model) =>
      model.id.toLowerCase().includes(term) ||
      model.name.toLowerCase().includes(term),
  );
});

/** Selected models with their generated, conflict-free prefixes. */
const plans = computed(() => {
  const taken = new Set(existing.value);
  return [...selected.value]
    .map((id) => models.value.find((model) => model.id === id))
    .filter((model): model is UpstreamModel => model !== undefined)
    .map((model, index) => ({
      model,
      prefix: uniquePrefix(
        fallbackPrefix(modelPrefixSlug(model.id), index),
        taken,
      ),
    }));
});

const allFilteredSelected = computed(
  () =>
    filtered.value.length > 0 &&
    filtered.value.every((model) => selected.value.has(model.id)),
);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    connectionId.value = props.connections[0]?.id ?? "";
    models.value = [];
    error.value = null;
    search.value = "";
    selected.value = new Set();
    if (connectionId.value) void loadModels();
  },
);

watch(connectionId, (id) => {
  if (props.open && id) void loadModels();
});

function toggle(modelId: string, checked: boolean): void {
  const next = new Set(selected.value);
  if (checked) next.add(modelId);
  else next.delete(modelId);
  selected.value = next;
}

function toggleAll(): void {
  if (allFilteredSelected.value) {
    const next = new Set(selected.value);
    for (const model of filtered.value) next.delete(model.id);
    selected.value = next;
  } else {
    const next = new Set(selected.value);
    for (const model of filtered.value) next.add(model.id);
    selected.value = next;
  }
}

async function loadModels(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const result = await api.listUpstreamModels(connectionId.value);
    models.value = result.models;
  } catch (caught) {
    models.value = [];
    error.value =
      caught instanceof ApiError ? caught.message : "Failed to load models";
  } finally {
    loading.value = false;
  }
}

async function importSelected(): Promise<void> {
  const plan = plans.value;
  if (!plan.length || !connectionId.value) return;

  importing.value = true;
  const results = await Promise.allSettled(
    plan.map((entry) =>
      api.createAlias({
        prefix: entry.prefix,
        connection_id: connectionId.value,
        model_override: entry.model.id,
      }),
    ),
  );
  importing.value = false;

  const failed = results.filter(
    (result) => result.status === "rejected",
  ).length;
  if (failed === 0) {
    toast.success(`Imported ${plan.length} aliases`);
  } else {
    toast.warning(
      `Imported ${plan.length - failed} of ${plan.length} aliases; ${failed} failed`,
    );
  }

  emit("saved");
  emit("update:open", false);
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent class="sm:max-w-2xl">
      <DialogHeader>
        <DialogTitle>Import models as aliases</DialogTitle>
        <DialogDescription>
          Reads the connection's upstream <code>/models</code> and creates one
          alias per selected model. Prefixes are generated from the model id and
          de-duplicated against existing aliases.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
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
            Create a connection before importing models.
          </p>
        </div>

        <div
          v-if="loading"
          class="rounded-md border p-6 text-center text-sm text-muted-foreground"
        >
          Loading models…
        </div>

        <div
          v-else-if="error"
          class="grid gap-2 rounded-md border border-destructive/40 p-4"
        >
          <p class="text-sm text-destructive">{{ error }}</p>
          <Button variant="outline" size="sm" class="w-fit" @click="loadModels">
            <RefreshCw /> Retry
          </Button>
        </div>

        <template v-else-if="models.length">
          <div class="flex items-center gap-2">
            <div class="relative flex-1">
              <Search
                class="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
              />
              <Input
                v-model="search"
                placeholder="Filter models"
                class="pl-8"
              />
            </div>
            <Button variant="outline" size="sm" @click="toggleAll">
              {{ allFilteredSelected ? "Clear" : "Select all" }}
            </Button>
          </div>

          <div class="max-h-72 divide-y overflow-y-auto rounded-md border">
            <label
              v-for="model in filtered"
              :key="model.id"
              class="flex cursor-pointer items-center gap-3 px-3 py-2 hover:bg-accent"
            >
              <Checkbox
                :model-value="selected.has(model.id)"
                @update:model-value="toggle(model.id, $event === true)"
              />
              <span class="min-w-0 flex-1">
                <span class="block truncate font-mono text-xs">{{
                  model.id
                }}</span>
                <span
                  v-if="model.name !== model.id"
                  class="block truncate text-xs text-muted-foreground"
                >
                  {{ model.name }}
                </span>
              </span>
              <Badge
                v-if="existing.has(modelPrefixSlug(model.id))"
                variant="outline"
                class="text-xs"
              >
                prefix exists
              </Badge>
            </label>
            <p
              v-if="!filtered.length"
              class="p-4 text-center text-sm text-muted-foreground"
            >
              No models match "{{ search }}".
            </p>
          </div>

          <p class="text-xs text-muted-foreground">
            <template v-if="selected.size">
              {{ selected.size }} selected ·
              {{
                plans
                  .map((plan) => plan.prefix)
                  .slice(0, 4)
                  .join(", ")
              }}<template v-if="plans.length > 4">
                +{{ plans.length - 4 }} more</template
              >
            </template>
            <template v-else>Nothing selected yet.</template>
          </p>
        </template>

        <p v-else class="rounded-md border p-4 text-sm text-muted-foreground">
          This upstream returned no models.
        </p>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)"
          >Cancel</Button
        >
        <Button :disabled="importing || !selected.size" @click="importSelected">
          {{
            importing
              ? "Importing…"
              : `Import ${selected.size || ""} aliases`.trim()
          }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
