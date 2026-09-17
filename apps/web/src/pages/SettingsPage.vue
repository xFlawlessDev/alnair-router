<script setup lang="ts">
import {
  CircleDollarSign,
  Gauge,
  RefreshCw,
  RotateCcw,
  Route,
  Save,
  ServerCog,
  ShieldCheck,
  TriangleAlert,
} from "@lucide/vue";
import { computed, onMounted, ref, type Component } from "vue";
import { toast } from "vue-sonner";

import ConfirmDialog from "@/components/ConfirmDialog.vue";
import PageHeader from "@/components/PageHeader.vue";
import DataSection from "@/components/settings/DataSection.vue";
import LimitsSection from "@/components/settings/LimitsSection.vue";
import PricingSection from "@/components/settings/PricingSection.vue";
import RoutingSection from "@/components/settings/RoutingSection.vue";
import SecuritySection from "@/components/settings/SecuritySection.vue";
import SettingsNav, {
  type NavEntry,
} from "@/components/settings/SettingsNav.vue";
import { Button } from "@/components/ui/button";
import { useSettingsController } from "@/lib/settings/controller";
import {
  SETTINGS_SECTIONS,
  sectionIsCustomized,
  sectionIsDirty,
  type SettingsSectionId,
} from "@/lib/settings/sections";

const ICONS: Record<SettingsSectionId, Component> = {
  security: ShieldCheck,
  routing: Route,
  limits: Gauge,
  pricing: CircleDollarSign,
  data: ServerCog,
};

/** One section is on screen at a time; the nav replaces the old card wall. */
const SECTIONS: Record<SettingsSectionId, Component> = {
  security: SecuritySection,
  routing: RoutingSection,
  limits: LimitsSection,
  pricing: PricingSection,
  data: DataSection,
};

const {
  form,
  settings,
  loading,
  saving,
  resetting,
  patch,
  customizedCount,
  failed,
  error,
  load,
  save,
  reset,
} = useSettingsController();

const resetOpen = ref(false);
const active = ref<SettingsSectionId>("security");

const dirty = computed(() => Object.keys(patch.value).length > 0);

const entries = computed<NavEntry[]>(() =>
  SETTINGS_SECTIONS.map((section) => ({
    id: section.id,
    label: section.label,
    icon: ICONS[section.id],
    dirty: sectionIsDirty(patch.value, section),
    customized: sectionIsCustomized(settings.value?.overrides ?? [], section),
  })),
);

const activeSection = computed(() => SECTIONS[active.value]);

/** Names the sections with unsaved edits, for the sticky bar. */
const dirtyLabels = computed(() =>
  SETTINGS_SECTIONS.filter((section) =>
    sectionIsDirty(patch.value, section),
  ).map((section) => section.label),
);

/** The server rejects a set-but-blank admin token; point at the field instead. */
async function saveAll(): Promise<void> {
  if (
    form.admin_token_enabled &&
    !settings.value?.server.admin_token_set &&
    !form.admin_token.trim()
  ) {
    active.value = "security";
    toast.error("Enter an admin token or turn the requirement off");
    return;
  }
  await save();
}

async function resetAll(): Promise<void> {
  if (await reset()) resetOpen.value = false;
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6 pb-20">
    <PageHeader
      title="Settings"
      description="Runtime configuration stored in the router database. Changes apply immediately and survive restarts."
    >
      <template #actions>
        <Button
          variant="outline"
          size="sm"
          :disabled="loading || saving"
          @click="load"
        >
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button
          variant="outline"
          size="sm"
          :disabled="loading || saving || !settings || customizedCount === 0"
          @click="resetOpen = true"
        >
          <RotateCcw /> Reset overrides
        </Button>
      </template>
    </PageHeader>

    <div
      v-if="failed"
      class="flex items-start gap-3 rounded-lg border border-destructive/40 bg-destructive/5 px-4 py-3.5"
    >
      <TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
      <div class="grid gap-1">
        <p class="text-sm font-medium text-destructive">Cannot load settings</p>
        <p class="text-xs text-pretty text-muted-foreground">{{ error }}</p>
        <Button
          variant="outline"
          size="sm"
          class="mt-1 w-fit"
          :disabled="loading"
          @click="load"
        >
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Try again
        </Button>
      </div>
    </div>

    <div v-else class="grid gap-6 lg:grid-cols-[13rem_minmax(0,1fr)] lg:gap-8">
      <SettingsNav
        :entries="entries"
        :active="active"
        :disabled="loading"
        @select="active = $event as SettingsSectionId"
      />

      <div class="min-w-0">
        <component :is="activeSection" />
      </div>
    </div>

    <!-- The header's Save scrolls away, so unsaved work gets its own bar. -->
    <Transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="translate-y-2 opacity-0"
      leave-active-class="transition duration-100 ease-in"
      leave-to-class="translate-y-2 opacity-0"
    >
      <div
        v-if="dirty && !failed"
        class="pointer-events-none sticky bottom-4 z-20 flex justify-center"
      >
        <div
          class="pointer-events-auto flex w-full max-w-3xl flex-wrap items-center gap-3 rounded-xl border bg-background/95 px-4 py-3 shadow-lg backdrop-blur supports-[backdrop-filter]:bg-background/80"
        >
          <div class="min-w-0 flex-1">
            <p class="text-sm font-medium">
              Unsaved changes
              <span class="font-normal text-muted-foreground">
                · {{ dirtyLabels.join(", ") }}
              </span>
            </p>
            <p class="text-xs text-muted-foreground">
              Applies immediately to new requests.
            </p>
          </div>
          <div class="flex items-center gap-2">
            <Button variant="ghost" size="sm" :disabled="saving" @click="load">
              Discard
            </Button>
            <Button size="sm" :disabled="saving || loading" @click="saveAll">
              <Save /> {{ saving ? "Saving…" : "Save changes" }}
            </Button>
          </div>
        </div>
      </div>
    </Transition>

    <ConfirmDialog
      v-model:open="resetOpen"
      title="Reset all overrides?"
      description="Every dashboard-managed setting is removed and the file/env configuration applies again. If your config.toml defines an admin token, re-enter it from the key icon in the header."
      confirm-label="Reset overrides"
      pending-label="Resetting…"
      :pending="resetting"
      @confirm="resetAll"
    />
  </div>
</template>
