<script setup lang="ts">
import { computed, ref } from "vue";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";

const props = defineProps<{ label: string; options: string[] }>();
const model = defineModel<string[] | null>({ required: true });
const custom = ref("");
const choices = computed(() =>
  [...new Set([...props.options, ...(model.value ?? [])])].sort((a, b) => a.localeCompare(b)),
);

function toggle(value: string, checked: boolean) {
  const selected = model.value ?? [];
  model.value = checked
    ? [...selected, value]
    : selected.filter((item) => item !== value);
}

function addCustom() {
  const value = custom.value.trim();
  if (!value) return;
  if (!model.value?.includes(value)) model.value = [...(model.value ?? []), value];
  custom.value = "";
}
</script>

<template>
  <fieldset class="grid gap-2">
    <legend class="text-sm font-medium">{{ label }}</legend>
    <select
      :aria-label="`${label} source`"
      :value="model === null ? 'catalog' : 'override'"
      class="border-input bg-background h-9 rounded-md border px-3 text-sm"
      @change="model = ($event.target as HTMLSelectElement).value === 'catalog' ? null : []"
    >
      <option value="catalog">Use catalog</option>
      <option value="override">Choose values</option>
    </select>
    <template v-if="model !== null">
      <div class="flex flex-wrap gap-x-4 gap-y-2">
        <label v-for="option in choices" :key="option" class="flex items-center gap-2 text-sm">
          <Checkbox
            :model-value="model.includes(option)"
            @update:model-value="toggle(option, $event === true)"
          />
          {{ option }}
        </label>
      </div>
      <p class="text-xs text-muted-foreground">No selections means none.</p>
      <details class="text-xs text-muted-foreground">
        <summary class="cursor-pointer">Add a custom value</summary>
        <div class="mt-2 flex gap-2">
          <Input v-model="custom" :aria-label="`Custom ${label}`" @keydown.enter.prevent="addCustom" />
          <Button type="button" variant="outline" size="sm" @click="addCustom">Add</Button>
        </div>
      </details>
    </template>
  </fieldset>
</template>
