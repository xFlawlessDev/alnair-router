<script setup lang="ts">
import { LayoutGrid, List, Search, X } from '@lucide/vue';
import { computed } from 'vue';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import {
  hasActiveFilters,
  type ConnectionFilters,
  type ConnectionsGroup,
  type ConnectionsView,
} from '@/lib/connectionsView';
import { PROVIDER_TYPES, type ProviderType } from '@/types/api';

const props = defineProps<{
  filters: ConnectionFilters;
  view: ConnectionsView;
  group: ConnectionsGroup;
  /** Rows left after filtering, and the total before it. */
  shown: number;
  total: number;
}>();

const emit = defineEmits<{
  'update:search': [string];
  'update:type': [ProviderType | 'all'];
  'update:status': [ConnectionFilters['status']];
  'update:view': [ConnectionsView];
  'update:group': [ConnectionsGroup];
  clear: [];
}>();

const filtering = computed(() => hasActiveFilters(props.filters));

/** Clicking the active toggle would deselect it; keep the current view instead. */
function pickView(value: unknown): void {
  if (value === 'list' || value === 'grid') emit('update:view', value);
}

function pickType(value: unknown): void {
  emit('update:type', (value === 'all' ? 'all' : value) as ProviderType | 'all');
}
</script>

<template>
  <div class="flex flex-wrap items-end gap-3 rounded-lg border p-3">
    <div class="grid min-w-56 flex-1 gap-1.5">
      <Label for="connection-search" class="text-xs text-muted-foreground">Search</Label>
      <div class="relative">
        <Search
          class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <Input
          id="connection-search"
          :model-value="filters.search"
          placeholder="Name, URL or provider…"
          class="h-9 pl-9"
          @update:model-value="emit('update:search', String($event))"
        />
      </div>
    </div>

    <div class="grid gap-1.5">
      <Label for="connection-type-filter" class="text-xs text-muted-foreground">
        Provider type
      </Label>
      <Select :model-value="filters.type" @update:model-value="pickType">
        <SelectTrigger id="connection-type-filter" class="h-9 w-44">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All types</SelectItem>
          <SelectItem v-for="type in PROVIDER_TYPES" :key="type" :value="type">
            {{ type }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div class="grid gap-1.5">
      <Label for="connection-status-filter" class="text-xs text-muted-foreground">Status</Label>
      <Select
        :model-value="filters.status"
        @update:model-value="emit('update:status', $event as ConnectionFilters['status'])"
      >
        <SelectTrigger id="connection-status-filter" class="h-9 w-36">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All statuses</SelectItem>
          <SelectItem value="enabled">Enabled</SelectItem>
          <SelectItem value="disabled">Disabled</SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div class="ml-auto flex items-end gap-3">
      <span class="pb-2 text-xs text-muted-foreground">{{ shown }} of {{ total }}</span>

      <div class="grid gap-1.5">
        <Label for="connection-group" class="text-xs text-muted-foreground">Group by</Label>
        <Select
          :model-value="group"
          @update:model-value="emit('update:group', $event as ConnectionsGroup)"
        >
          <SelectTrigger id="connection-group" class="h-9 w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">No grouping</SelectItem>
            <SelectItem value="provider">Provider</SelectItem>
            <SelectItem value="type">Provider type</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <ToggleGroup
        type="single"
        variant="outline"
        size="default"
        :model-value="view"
        aria-label="Layout"
        @update:model-value="pickView"
      >
        <ToggleGroupItem value="list" aria-label="List view">
          <List />
        </ToggleGroupItem>
        <ToggleGroupItem value="grid" aria-label="Grid view">
          <LayoutGrid />
        </ToggleGroupItem>
      </ToggleGroup>

      <Button v-if="filtering" variant="ghost" size="sm" class="mb-0.5" @click="emit('clear')">
        <X /> Clear filters
      </Button>
    </div>
  </div>
</template>
