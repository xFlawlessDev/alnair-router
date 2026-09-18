<script setup lang="ts">
import { Copy, Plus, Wrench } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

import ProviderIcon from "@/components/ProviderIcon.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ApiError, api } from "@/lib/api";
import {
  PROVIDER_CATEGORIES,
  type ProviderCategory,
  type ProviderPreset,
} from "@/types/api";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{
  "update:open": [boolean];
  select: [preset: ProviderPreset];
  "quick-add": [preset: ProviderPreset];
  custom: [];
}>();

type CategoryFilter = ProviderCategory | "all";

const CATEGORY_SHORT: Record<ProviderCategory, string> = {
  api_key: "API key",
  free_tier: "Free tier",
  local: "Local",
};

const presets = ref<ProviderPreset[]>([]);
const loading = ref(false);
const search = ref("");
const category = ref<CategoryFilter>("all");

/** Only offer a tier filter while more than one tier is present. */
const categoryOptions = computed(() =>
  PROVIDER_CATEGORIES.filter((option) =>
    presets.value.some((preset) => preset.category === option.id),
  ),
);

const filtered = computed(() => {
  const term = search.value.trim().toLowerCase();
  return presets.value.filter((preset) => {
    if (category.value !== "all" && preset.category !== category.value) {
      return false;
    }
    if (!term) return true;
    return [
      preset.id,
      preset.label,
      preset.base_url,
      preset.provider_type,
      preset.note ?? "",
    ].some((value) => value.toLowerCase().includes(term));
  });
});

/** Presets grouped into tiers; a tier with no match drops out entirely. */
const sections = computed(() =>
  PROVIDER_CATEGORIES.map((option) => ({
    ...option,
    items: filtered.value.filter((preset) => preset.category === option.id),
  })).filter((section) => section.items.length),
);

/** The custom-endpoint shortcut only competes for attention in an open list. */
const showCustom = computed(
  () => !search.value.trim() && category.value === "all",
);

/** A no-key provider can be added outright; a configured one can be cloned. */
function canQuickAdd(preset: ProviderPreset): boolean {
  return preset.auth === "none" || preset.configured > 0;
}

function quickLabel(preset: ProviderPreset): string {
  return preset.auth === "none" ? "Quick add" : "Duplicate";
}

/** Roving arrow-key navigation across the cards, mirroring the DOM order. */
function onGridKeydown(event: KeyboardEvent): void {
  const moves: Record<string, (index: number, last: number) => number> = {
    ArrowDown: (index, last) => (index < 0 ? 0 : Math.min(index + 1, last)),
    ArrowRight: (index, last) => (index < 0 ? 0 : Math.min(index + 1, last)),
    ArrowUp: (index, last) => (index < 0 ? last : Math.max(index - 1, 0)),
    ArrowLeft: (index, last) => (index < 0 ? last : Math.max(index - 1, 0)),
    Home: () => 0,
    End: (_index, last) => last,
  };
  const move = moves[event.key];
  if (!move) return;

  const cards = [
    ...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>(
      "[data-preset-card]",
    ),
  ];
  if (!cards.length) return;

  event.preventDefault();
  const current = cards.indexOf(document.activeElement as HTMLButtonElement);
  cards[move(current, cards.length - 1)]?.focus();
}

function pickCategory(value: unknown): void {
  if (value === "api_key" || value === "free_tier" || value === "local") {
    category.value = value;
  } else {
    category.value = "all";
  }
}

watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    search.value = "";
    category.value = "all";
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
          Pick a preset to prefill the endpoint and wire format, or start from a
          custom endpoint. Every field stays editable afterwards.
        </DialogDescription>
      </DialogHeader>

      <div class="flex flex-wrap items-center gap-2">
        <Input
          v-model="search"
          placeholder="Search providers…"
          aria-label="Search providers"
          class="min-w-56 flex-1"
        />

        <ToggleGroup
          v-if="categoryOptions.length > 1"
          type="single"
          variant="outline"
          size="default"
          :model-value="category"
          aria-label="Filter by provider tier"
          @update:model-value="pickCategory"
        >
          <ToggleGroupItem value="all" aria-label="All tiers">
            All
          </ToggleGroupItem>
          <ToggleGroupItem
            v-for="option in categoryOptions"
            :key="option.id"
            :value="option.id"
            :aria-label="option.label"
          >
            {{ CATEGORY_SHORT[option.id] }}
          </ToggleGroupItem>
        </ToggleGroup>
      </div>

      <p class="sr-only" aria-live="polite">
        {{ filtered.length }} providers shown
      </p>

      <p v-if="loading" class="text-sm text-muted-foreground">
        Loading providers…
      </p>

      <p
        v-else-if="!sections.length"
        class="text-sm text-muted-foreground"
        role="status"
      >
        {{
          search.trim()
            ? `Nothing matches “${search.trim()}”.`
            : "No providers in this tier."
        }}
      </p>

      <div
        v-else
        class="flex max-h-[26rem] flex-col gap-4 overflow-y-auto pr-1"
        @keydown="onGridKeydown"
      >
        <button
          v-if="showCustom"
          type="button"
          data-preset-card
          aria-label="Custom endpoint"
          class="flex flex-col gap-1 rounded-md border border-dashed p-3 text-left transition-colors outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
          @click="emit('custom')"
        >
          <span class="flex items-center gap-2 font-medium">
            <Wrench class="size-4 text-muted-foreground" />
            Custom endpoint
          </span>
          <span class="text-xs text-muted-foreground">
            Any OpenAI-compatible or Anthropic-native URL, configured by hand.
          </span>
        </button>

        <section
          v-for="section in sections"
          :key="section.id"
          class="flex flex-col gap-2"
          :aria-label="section.label"
        >
          <h3
            class="text-xs font-semibold tracking-wide text-muted-foreground uppercase"
          >
            {{ section.label }} · {{ section.items.length }}
          </h3>
          <div class="grid gap-2 sm:grid-cols-2">
            <div
              v-for="preset in section.items"
              :key="preset.id"
              class="flex flex-col gap-2 rounded-md border p-3 transition-colors focus-within:ring-2 focus-within:ring-ring hover:bg-accent"
            >
              <button
                type="button"
                data-preset-card
                class="flex flex-col gap-1.5 rounded-sm text-left outline-none"
                :aria-label="`Configure ${preset.label}`"
                @click="emit('select', preset)"
              >
                <span class="flex flex-wrap items-center gap-2">
                  <ProviderIcon :id="preset.id" :label="preset.label" />
                  <span class="font-medium">{{ preset.label }}</span>
                  <Badge variant="outline">{{ preset.provider_type }}</Badge>
                </span>

                <code
                  class="truncate text-xs text-muted-foreground"
                  :title="preset.base_url"
                >
                  {{ preset.base_url }}
                </code>

                <span
                  v-if="preset.note"
                  class="text-xs text-muted-foreground"
                  >{{ preset.note }}</span
                >

                <span class="flex flex-wrap items-center gap-1">
                  <Badge variant="secondary">{{
                    CATEGORY_SHORT[preset.category]
                  }}</Badge>
                  <Badge v-if="preset.configured" variant="secondary">
                    {{ preset.configured }} added
                  </Badge>
                  <Badge v-else-if="preset.auth === 'none'" variant="secondary"
                    >no key</Badge
                  >
                  <Badge v-else variant="outline">key required</Badge>
                </span>
              </button>

              <div class="flex items-center justify-between gap-2">
                <a
                  v-if="preset.api_key_url"
                  :href="preset.api_key_url"
                  target="_blank"
                  rel="noreferrer"
                  class="text-xs underline underline-offset-4 hover:text-foreground"
                  :aria-label="`Get an API key for ${preset.label}`"
                >
                  Get key
                </a>
                <span v-else />

                <Button
                  v-if="canQuickAdd(preset)"
                  variant="outline"
                  size="sm"
                  type="button"
                  :aria-label="`${quickLabel(preset)} ${preset.label}`"
                  @click="emit('quick-add', preset)"
                >
                  <Plus v-if="preset.auth === 'none'" />
                  <Copy v-else />
                  {{ quickLabel(preset) }}
                </Button>
              </div>
            </div>
          </div>
        </section>
      </div>
    </DialogScrollContent>
  </Dialog>
</template>
