<script setup lang="ts">
import { computed, ref, watch } from "vue";

import {
  Combobox,
  ComboboxAnchor,
  ComboboxEmpty,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxSeparator,
} from "@/components/ui/combobox";
import {
  TagsInput,
  TagsInputInput,
  TagsInputItem,
  TagsInputItemDelete,
  TagsInputItemText,
} from "@/components/ui/tags-input";
import { buildSuggestions, type Suggestion } from "@/lib/suggestions";
import type { Alias, ComboWithEntries } from "@/types/api";

const props = withDefaults(
  defineProps<{
    modelValue: string[];
    aliases: Alias[];
    combos: ComboWithEntries[];
    hint?: string;
  }>(),
  { hint: "" },
);
const emit = defineEmits<{ "update:modelValue": [string[]] }>();

/**
 * Reka's combobox drives the search; clearing the pick after each selection
 * turns it into a repeatable "add pattern" palette.
 */
const picked = ref<Suggestion | null>(null);

const suggestions = computed(() =>
  buildSuggestions({
    aliases: props.aliases,
    combos: props.combos,
    taken: props.modelValue,
  }),
);

const hasSuggestions = computed(
  () =>
    suggestions.value.aliases.length > 0 || suggestions.value.combos.length > 0,
);

watch(picked, (suggestion) => {
  if (!suggestion) return;
  add(suggestion.pattern);
  picked.value = null;
});

function add(value: string): void {
  const trimmed = value.trim();
  if (!trimmed || props.modelValue.includes(trimmed)) return;
  emit("update:modelValue", [...props.modelValue, trimmed]);
}

/** TagsInput may carry numbers, model patterns are always strings. */
function updateTags(values: unknown): void {
  if (!Array.isArray(values)) return;
  emit(
    "update:modelValue",
    values.map((value) => String(value)),
  );
}
</script>

<template>
  <div class="grid gap-2">
    <TagsInput
      class="min-h-10"
      :model-value="modelValue"
      @update:model-value="updateTags"
    >
      <TagsInputItem v-for="model in modelValue" :key="model" :value="model">
        <TagsInputItemText />
        <TagsInputItemDelete />
      </TagsInputItem>
      <TagsInputInput
        placeholder="openai/* or gpt-4o-mini"
        class="min-w-40 flex-1 bg-transparent shadow-none outline-none"
      />
    </TagsInput>

    <Combobox
      v-if="hasSuggestions"
      v-model="picked"
      by="id"
      reset-search-term-on-select
    >
      <ComboboxAnchor>
        <ComboboxInput
          placeholder="Search aliases and combos to add…"
          class="h-8 w-full text-xs shadow-none"
        />
      </ComboboxAnchor>
      <ComboboxList class="w-[var(--reka-combobox-trigger-width)]">
        <ComboboxEmpty>No alias or combo matches</ComboboxEmpty>

        <ComboboxGroup v-if="suggestions.aliases.length">
          <div class="px-2 py-1.5 text-xs font-medium text-muted-foreground">
            Aliases
          </div>
          <ComboboxItem
            v-for="item in suggestions.aliases"
            :key="item.id"
            :value="item"
            class="text-xs"
          >
            <span class="truncate font-mono">{{ item.pattern }}</span>
            <span v-if="item.detail" class="truncate text-muted-foreground">{{
              item.detail
            }}</span>
          </ComboboxItem>
        </ComboboxGroup>

        <ComboboxSeparator
          v-if="suggestions.aliases.length && suggestions.combos.length"
        />

        <ComboboxGroup v-if="suggestions.combos.length">
          <div class="px-2 py-1.5 text-xs font-medium text-muted-foreground">
            Combos
          </div>
          <ComboboxItem
            v-for="item in suggestions.combos"
            :key="item.id"
            :value="item"
            class="text-xs"
          >
            <span class="truncate font-mono">{{ item.pattern }}</span>
            <span v-if="item.detail" class="truncate text-muted-foreground">{{
              item.detail
            }}</span>
          </ComboboxItem>
        </ComboboxGroup>
      </ComboboxList>
    </Combobox>

    <p v-if="hint" class="text-xs text-muted-foreground">{{ hint }}</p>
  </div>
</template>
