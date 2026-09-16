<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

import ProviderIcon from "@/components/ProviderIcon.vue";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogDescription,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ApiError, api } from "@/lib/api";
import { PROVIDER_CATEGORIES, type ProviderPreset } from "@/types/api";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{
  "update:open": [boolean];
  select: [preset: ProviderPreset];
}>();

const presets = ref<ProviderPreset[]>([]);
const loading = ref(false);
const search = ref("");

const filtered = computed(() => {
  const term = search.value.trim().toLowerCase();
  if (!term) return presets.value;
  return presets.value.filter((preset) =>
    [preset.id, preset.label, preset.base_url, preset.provider_type].some(
      (value) => value.toLowerCase().includes(term),
    ),
  );
});

/** Presets grouped into tiers; a tier with no match drops out entirely. */
const sections = computed(() =>
  PROVIDER_CATEGORIES.map((category) => ({
    ...category,
    items: filtered.value.filter((preset) => preset.category === category.id),
  })).filter((section) => section.items.length),
);

watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    search.value = "";
    loading.value = true;
    try {
      presets.value = (await api.listProviders()).data;
    } catch (caught) {
      toast.error(
        caught instanceof ApiError
          ? caught.message
          : "Failed to load providers",
      );
    } finally {
      loading.value = false;
    }
  },
);
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent class="sm:max-w-3xl">
      <DialogHeader>
        <DialogTitle>Add a provider</DialogTitle>
        <DialogDescription>
          Pick a preset to prefill the endpoint and wire format. Every field
          stays editable afterwards.
        </DialogDescription>
      </DialogHeader>

      <Input
        v-model="search"
        placeholder="Search providers…"
        aria-label="Search providers"
      />

      <p v-if="loading" class="text-sm text-muted-foreground">
        Loading providers…
      </p>

      <p v-else-if="!sections.length" class="text-sm text-muted-foreground">
        Nothing matches “{{ search.trim() }}”.
      </p>

      <div
        v-else
        class="flex max-h-[26rem] flex-col gap-4 overflow-y-auto pr-1"
      >
        <section
          v-for="section in sections"
          :key="section.id"
          class="flex flex-col gap-2"
        >
          <h3
            class="text-xs font-semibold tracking-wide text-muted-foreground uppercase"
          >
            {{ section.label }} · {{ section.items.length }}
          </h3>
          <div class="grid gap-2 sm:grid-cols-2">
            <button
              v-for="preset in section.items"
              :key="preset.id"
              type="button"
              class="flex flex-col gap-1.5 rounded-md border p-3 text-left transition-colors hover:bg-accent"
              @click="emit('select', preset)"
            >
              <span class="flex flex-wrap items-center gap-2">
                <ProviderIcon :id="preset.id" :label="preset.label" />
                <span class="font-medium">{{ preset.label }}</span>
                <Badge variant="outline">{{ preset.provider_type }}</Badge>
                <Badge v-if="preset.configured" variant="secondary">
                  {{ preset.configured }} added
                </Badge>
                <Badge v-else-if="preset.auth === 'none'" variant="secondary"
                  >no key</Badge
                >
              </span>
              <code
                class="truncate text-xs text-muted-foreground"
                :title="preset.base_url"
              >
                {{ preset.base_url }}
              </code>
              <span v-if="preset.note" class="text-xs text-muted-foreground">{{
                preset.note
              }}</span>
            </button>
          </div>
        </section>
      </div>

      <p class="text-xs text-muted-foreground">
        Need something else? Close this and use
        <strong>Add connection</strong> for a fully custom endpoint.
      </p>
    </DialogScrollContent>
  </Dialog>
</template>
