<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { X } from '@lucide/vue';

import { Button } from '@/components/ui/button';
import {
  Combobox,
  ComboboxAnchor,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from '@/components/ui/combobox';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { ApiKey, UsageFacets } from '@/types/api';

/** Sentinel because Select values cannot be empty strings. */
const ALL = '__all__';

const props = defineProps<{
  keys: ApiKey[];
  facets: UsageFacets | null;
  apiKeyId: string;
  model: string;
  provider: string;
  connection: string;
}>();
const emit = defineEmits<{
  'update:apiKeyId': [string];
  'update:model': [string];
  'update:provider': [string];
  'update:connection': [string];
  clear: [];
}>();

/** Suggestion row for the model combobox. */
interface ModelSuggestion {
  id: string;
  pattern: string;
}

const picked = ref<ModelSuggestion | null>(null);

const models = computed<ModelSuggestion[]>(() =>
  (props.facets?.models ?? []).map((model) => ({ id: model, pattern: model })),
);

const hasFilters = computed(
  () =>
    props.apiKeyId !== ALL ||
    props.model.trim() !== '' ||
    props.provider !== ALL ||
    props.connection !== ALL,
);

watch(picked, (suggestion) => {
  if (!suggestion) return;
  emit('update:model', suggestion.pattern);
  picked.value = null;
});

/** The input doubles as the filter value, so partial matches work too. */
function updateModel(value: unknown): void {
  emit('update:model', typeof value === 'string' ? value : '');
}
</script>

<template>
  <div class="flex flex-wrap items-end gap-3 rounded-lg border p-3">
    <div class="grid gap-1.5">
      <Label for="usage-key-filter" class="text-xs text-muted-foreground">API key</Label>
      <Select
        :model-value="apiKeyId"
        @update:model-value="emit('update:apiKeyId', String($event))"
      >
        <SelectTrigger id="usage-key-filter" class="h-9 w-44">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem :value="ALL">All keys</SelectItem>
          <SelectItem v-for="key in keys" :key="key.id" :value="key.id">
            {{ key.name }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div class="grid gap-1.5">
      <Label for="usage-model-filter" class="text-xs text-muted-foreground">Model</Label>
      <Combobox
        v-model="picked"
        by="id"
        :reset-search-term-on-select="false"
        :reset-search-term-on-blur="false"
        open-on-focus
      >
        <ComboboxAnchor class="w-56">
          <ComboboxInput
            id="usage-model-filter"
            :model-value="model"
            placeholder="Any model"
            class="h-9 w-full"
            @update:model-value="updateModel"
          />
        </ComboboxAnchor>
        <ComboboxList class="w-[var(--reka-combobox-trigger-width)]">
          <ComboboxEmpty>No recorded model matches</ComboboxEmpty>
          <ComboboxItem v-for="item in models" :key="item.id" :value="item" class="text-xs">
            <span class="truncate font-mono">{{ item.pattern }}</span>
          </ComboboxItem>
        </ComboboxList>
      </Combobox>
    </div>

    <div class="grid gap-1.5">
      <Label for="usage-connection-filter" class="text-xs text-muted-foreground">Connection</Label>
      <Select
        :model-value="connection"
        @update:model-value="emit('update:connection', String($event))"
      >
        <SelectTrigger id="usage-connection-filter" class="h-9 w-48">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem :value="ALL">All connections</SelectItem>
          <SelectItem v-for="item in facets?.connections ?? []" :key="item" :value="item">
            {{ item }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div class="grid gap-1.5">
      <Label for="usage-provider-filter" class="text-xs text-muted-foreground">Provider type</Label>
      <Select
        :model-value="provider"
        @update:model-value="emit('update:provider', String($event))"
      >
        <SelectTrigger id="usage-provider-filter" class="h-9 w-48">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem :value="ALL">All provider types</SelectItem>
          <SelectItem v-for="item in facets?.providers ?? []" :key="item" :value="item">
            {{ item }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <Button v-if="hasFilters" variant="ghost" size="sm" @click="emit('clear')">
      <X /> Clear filters
    </Button>
  </div>
</template>
