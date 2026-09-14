<script setup lang="ts">
import { Plus, X } from '@lucide/vue';
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { ApiError, api } from '@/lib/api';
import { parseHeaders } from '@/lib/format';
import { PROVIDER_TYPES, type Connection, type ConnectionInput, type ProviderType } from '@/types/api';

const props = defineProps<{ open: boolean; connection: Connection | null }>();
const emit = defineEmits<{ 'update:open': [boolean]; saved: [] }>();

interface HeaderRow {
  key: string;
  value: string;
}

const name = ref('');
const providerType = ref<ProviderType>('openai-compatible');
const baseUrl = ref('');
const apiKey = ref('');
const clearApiKey = ref(false);
const headers = ref<HeaderRow[]>([]);
const enabled = ref(true);
const saving = ref(false);

const isEdit = computed(() => props.connection !== null);
const hasExistingKey = computed(() => Boolean(props.connection?.api_key));

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const connection = props.connection;
    name.value = connection?.name ?? '';
    providerType.value = connection?.provider_type ?? 'openai-compatible';
    baseUrl.value = connection?.base_url ?? '';
    apiKey.value = '';
    clearApiKey.value = false;
    headers.value = connection
      ? Object.entries(parseHeaders(connection.custom_headers)).map(([key, value]) => ({ key, value }))
      : [];
    enabled.value = connection ? connection.enabled !== 0 : true;
  },
);

function addHeader(): void {
  headers.value.push({ key: '', value: '' });
}

function removeHeader(index: number): void {
  headers.value.splice(index, 1);
}

async function save(): Promise<void> {
  if (!name.value.trim() || !baseUrl.value.trim()) {
    toast.error('Name and base URL are required');
    return;
  }

  const customHeaders: Record<string, string> = {};
  for (const row of headers.value) {
    const key = row.key.trim();
    if (key) customHeaders[key] = row.value.trim();
  }

  const body = {
    name: name.value.trim(),
    provider_type: providerType.value,
    base_url: baseUrl.value.trim(),
    custom_headers: customHeaders,
    enabled: enabled.value,
  };

  saving.value = true;
  try {
    if (props.connection) {
      const patch: Partial<ConnectionInput> = { ...body };
      if (apiKey.value.trim()) patch.api_key = apiKey.value.trim();
      else if (clearApiKey.value) patch.api_key = null;
      await api.updateConnection(props.connection.id, patch);
      toast.success(`Connection “${body.name}” updated`);
    } else {
      await api.createConnection(
        apiKey.value.trim() ? { ...body, api_key: apiKey.value.trim() } : body,
      );
      toast.success(`Connection “${body.name}” created`);
    }
    emit('saved');
    emit('update:open', false);
  } catch (error) {
    toast.error(error instanceof ApiError ? error.message : 'Failed to save connection');
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent class="sm:max-w-2xl">
      <DialogHeader>
        <DialogTitle>{{ isEdit ? 'Edit connection' : 'New connection' }}</DialogTitle>
        <DialogDescription>
          An upstream endpoint. Credentials are stored by the router as configured — plaintext in
          SQLite today.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="connection-name">Name</Label>
          <Input id="connection-name" v-model="name" placeholder="openai-main" />
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label>Provider type</Label>
            <Select v-model="providerType">
              <SelectTrigger class="w-full">
                <SelectValue placeholder="Select provider" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="type in PROVIDER_TYPES" :key="type" :value="type">
                  {{ type }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="flex items-end justify-between gap-4 rounded-md border p-3">
            <div>
              <Label for="connection-enabled">Enabled</Label>
              <p class="text-xs text-muted-foreground">Disabled connections are skipped.</p>
            </div>
            <Switch id="connection-enabled" v-model="enabled" />
          </div>
        </div>

        <div class="grid gap-2">
          <Label for="connection-base-url">Base URL</Label>
          <Input
            id="connection-base-url"
            v-model="baseUrl"
            placeholder="https://api.openai.com/v1"
          />
        </div>

        <div class="grid gap-2">
          <Label for="connection-api-key">API key</Label>
          <Input
            id="connection-api-key"
            v-model="apiKey"
            type="password"
            autocomplete="off"
            :placeholder="
              isEdit && hasExistingKey && !clearApiKey
                ? 'Leave blank to keep the configured key'
                : 'sk-…'
            "
          />
          <div v-if="isEdit && hasExistingKey" class="text-xs text-muted-foreground">
            <template v-if="clearApiKey">
              The stored key will be removed on save.
              <button class="underline" type="button" @click="clearApiKey = false">Undo</button>
            </template>
            <template v-else>
              A key is configured.
              <button class="underline" type="button" @click="clearApiKey = true">Clear it</button>
            </template>
          </div>
        </div>

        <div class="grid gap-2">
          <div class="flex items-center justify-between">
            <Label>Custom headers</Label>
            <Button variant="outline" size="sm" type="button" @click="addHeader">
              <Plus /> Add header
            </Button>
          </div>
          <p v-if="!headers.length" class="text-xs text-muted-foreground">
            No custom headers. Values are sent verbatim to this upstream.
          </p>
          <div v-for="(header, index) in headers" :key="index" class="flex items-center gap-2">
            <Input v-model="header.key" placeholder="Header-Name" class="flex-1" />
            <Input v-model="header.value" placeholder="value" class="flex-1" />
            <Button
              variant="ghost"
              size="icon"
              type="button"
              aria-label="Remove header"
              @click="removeHeader(index)"
            >
              <X />
            </Button>
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">Cancel</Button>
        <Button :disabled="saving" @click="save">
          {{ saving ? 'Saving…' : isEdit ? 'Save changes' : 'Create connection' }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
