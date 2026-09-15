<script setup lang="ts">
import { ArrowDown, ArrowUp, Plus, X } from '@lucide/vue';
import { computed, ref, watch } from 'vue';
import { toast } from 'vue-sonner';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import TierInput from '@/components/combos/TierInput.vue';
import { ApiError, api } from '@/lib/api';
import type { Alias, ComboWithEntries } from '@/types/api';

const props = defineProps<{
  open: boolean;
  combo: ComboWithEntries | null;
  aliases: Alias[];
  combos: ComboWithEntries[];
}>();
const emit = defineEmits<{ 'update:open': [boolean]; saved: [] }>();

const name = ref('');
const description = ref('');
const enabled = ref(true);
const entries = ref<string[]>([]);
const saving = ref(false);

const isEdit = computed(() => props.combo !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const combo = props.combo;
    name.value = combo?.combo.name ?? '';
    description.value = combo?.combo.description ?? '';
    enabled.value = combo ? combo.combo.enabled !== 0 : true;
    entries.value = combo ? combo.entries.map((entry) => entry.model_ref) : [''];
  },
);

function addEntry(): void {
  entries.value.push('');
}

function removeEntry(index: number): void {
  entries.value.splice(index, 1);
}

function move(index: number, delta: -1 | 1): void {
  const target = index + delta;
  if (target < 0 || target >= entries.value.length) return;
  const [entry] = entries.value.splice(index, 1);
  if (entry !== undefined) entries.value.splice(target, 0, entry);
}

async function save(): Promise<void> {
  const trimmedName = name.value.trim();
  if (!trimmedName) {
    toast.error('Name is required');
    return;
  }
  const modelRefs = entries.value.map((entry) => entry.trim()).filter(Boolean);
  if (!modelRefs.length) {
    toast.error('Add at least one tier to the fallback chain');
    return;
  }

  const body = {
    name: trimmedName,
    description: description.value.trim() || null,
    enabled: enabled.value,
    entries: modelRefs,
  };

  saving.value = true;
  try {
    if (props.combo) {
      await api.updateCombo(props.combo.combo.id, body);
      toast.success(`Combo “${trimmedName}” updated`);
    } else {
      await api.createCombo(body);
      toast.success(`Combo “${trimmedName}” created`);
    }
    emit('saved');
    emit('update:open', false);
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Failed to save combo');
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent class="sm:max-w-2xl">
      <DialogHeader>
        <DialogTitle>{{ isEdit ? 'Edit combo' : 'New combo' }}</DialogTitle>
        <DialogDescription>
          Tiers are tried in order. When a tier fails before emitting content, the router moves to
          the next one.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="combo-name">Name</Label>
            <Input id="combo-name" v-model="name" placeholder="free-forever" autocapitalize="off" />
            <p class="text-xs text-muted-foreground">Trimmed and lowercased by the router.</p>
          </div>
          <div class="flex items-end justify-between gap-4 rounded-md border p-3">
            <div>
              <Label for="combo-enabled">Enabled</Label>
              <p class="text-xs text-muted-foreground">Disabled combos error explicitly.</p>
            </div>
            <Switch id="combo-enabled" v-model="enabled" />
          </div>
        </div>

        <div class="grid gap-2">
          <Label for="combo-description">Description</Label>
          <Textarea id="combo-description" v-model="description" placeholder="Optional notes" />
        </div>

        <div class="grid gap-2">
          <div class="flex items-center justify-between">
            <Label>Fallback tiers</Label>
            <Button variant="outline" size="sm" type="button" @click="addEntry">
              <Plus /> Add tier
            </Button>
          </div>
          <div v-for="(entry, index) in entries" :key="index" class="flex items-center gap-2">
            <span class="w-6 shrink-0 text-center text-sm text-muted-foreground">{{ index + 1 }}</span>
            <TierInput
              v-model="entries[index]"
              :aliases="aliases"
              :combos="combos"
              :taken="entries.filter((_, other) => other !== index)"
              :exclude-combo-id="combo?.combo.id ?? null"
              class="flex-1"
            />
            <Button
              variant="ghost"
              size="icon"
              type="button"
              aria-label="Move tier up"
              :disabled="index === 0"
              @click="move(index, -1)"
            >
              <ArrowUp />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              type="button"
              aria-label="Move tier down"
              :disabled="index === entries.length - 1"
              @click="move(index, 1)"
            >
              <ArrowDown />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              type="button"
              aria-label="Remove tier"
              @click="removeEntry(index)"
            >
              <X />
            </Button>
          </div>
          <p class="text-xs text-muted-foreground">
            Search and pick an alias or another combo, or type any reference —
            <code>prefix/model</code>, a bare model, or a combo name.
          </p>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">Cancel</Button>
        <Button :disabled="saving" @click="save">
          {{ saving ? 'Saving…' : isEdit ? 'Save changes' : 'Create combo' }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
