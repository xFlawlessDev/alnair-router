<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import {
  Combobox,
  ComboboxAnchor,
  ComboboxEmpty,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxSeparator,
} from '@/components/ui/combobox';
import { buildSuggestions, type Suggestion } from '@/lib/suggestions';
import type { Alias, ComboWithEntries, ID } from '@/types/api';

const props = withDefaults(
  defineProps<{
    modelValue: string;
    aliases: Alias[];
    combos: ComboWithEntries[];
    /** References used by the other tiers, hidden from the suggestions. */
    taken?: string[];
    /** The combo being edited: a tier must not reference its own chain. */
    excludeComboId?: ID | null;
  }>(),
  { taken: () => [], excludeComboId: null },
);
const emit = defineEmits<{ 'update:modelValue': [string] }>();

const picked = ref<Suggestion | null>(null);

const suggestions = computed(() =>
  buildSuggestions({
    aliases: props.aliases,
    combos: props.combos,
    taken: props.taken,
    excludeComboId: props.excludeComboId,
  }),
);

watch(picked, (suggestion) => {
  if (!suggestion) return;
  emit('update:modelValue', suggestion.pattern);
  picked.value = null;
});

/** The input doubles as the value, so arbitrary references still work. */
function updateText(value: unknown): void {
  emit('update:modelValue', typeof value === 'string' ? value : '');
}
</script>

<template>
  <div class="w-full">
    <Combobox
      v-model="picked"
      by="id"
      :reset-search-term-on-select="false"
      :reset-search-term-on-blur="false"
      open-on-focus
    >
      <ComboboxAnchor class="w-full">
        <ComboboxInput
          :model-value="modelValue"
          placeholder="oa/gpt-4o-mini or another combo name"
          class="h-9 w-full"
          @update:model-value="updateText"
        />
      </ComboboxAnchor>

      <ComboboxList class="w-[var(--reka-combobox-trigger-width)]">
        <ComboboxEmpty>No alias or combo matches — free text is kept as-is</ComboboxEmpty>

        <ComboboxGroup v-if="suggestions.aliases.length">
          <div class="px-2 py-1.5 text-xs font-medium text-muted-foreground">Aliases</div>
          <ComboboxItem
            v-for="item in suggestions.aliases"
            :key="item.id"
            :value="item"
            class="text-xs"
          >
            <span class="truncate font-mono">{{ item.pattern }}</span>
            <span v-if="item.detail" class="truncate text-muted-foreground">{{ item.detail }}</span>
          </ComboboxItem>
        </ComboboxGroup>

        <ComboboxSeparator v-if="suggestions.aliases.length && suggestions.combos.length" />

        <ComboboxGroup v-if="suggestions.combos.length">
          <div class="px-2 py-1.5 text-xs font-medium text-muted-foreground">Combos</div>
          <ComboboxItem
            v-for="item in suggestions.combos"
            :key="item.id"
            :value="item"
            class="text-xs"
          >
            <span class="truncate font-mono">{{ item.pattern }}</span>
            <span v-if="item.detail" class="truncate text-muted-foreground">{{ item.detail }}</span>
          </ComboboxItem>
        </ComboboxGroup>
      </ComboboxList>
    </Combobox>
  </div>
</template>
