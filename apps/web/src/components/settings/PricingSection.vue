<script setup lang="ts">
import { CircleDollarSign } from "@lucide/vue";
import { computed } from "vue";

import SettingToggle from "@/components/settings/SettingToggle.vue";
import SettingsField from "@/components/settings/SettingsField.vue";
import SettingsSection from "@/components/settings/SettingsSection.vue";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { useSettingsForm } from "@/lib/settings/context";
import { SETTINGS_SECTIONS } from "@/lib/settings/sections";

const { form, loading } = useSettingsForm();

const section = SETTINGS_SECTIONS.find((entry) => entry.id === "pricing")!;

/** The interval is clamped server-side to 60s; show it in friendlier terms. */
const intervalSummary = computed(() => {
  const seconds = Number(form.pricing_sync_interval_secs);
  if (!Number.isFinite(seconds) || seconds <= 0) return "Not scheduled";
  if (seconds % 3600 === 0) {
    const hours = seconds / 3600;
    return hours === 1 ? "Every hour" : `Every ${hours} hours`;
  }
  if (seconds % 60 === 0) {
    const minutes = seconds / 60;
    return minutes === 1 ? "Every minute" : `Every ${minutes} minutes`;
  }
  return `Every ${Math.trunc(seconds)} seconds`;
});
</script>

<template>
  <SettingsSection :section="section">
    <template #title>
      <CircleDollarSign class="size-4 text-muted-foreground" />
      {{ section.label }}
    </template>

    <div class="grid gap-3">
      <SettingToggle
        control-id="setting-pricing-sync"
        title="Sync pricing catalog"
        description="Crawl the source URL in the background on the configured interval."
      >
        <template #control>
          <Switch
            id="setting-pricing-sync"
            v-model="form.pricing_sync_enabled"
            :disabled="loading"
          />
        </template>

        <div class="grid gap-4 sm:grid-cols-2">
          <SettingsField
            id="setting-sync-interval"
            label="Sync interval (seconds)"
            :hint="`${intervalSummary}. Clamped to at least 60 seconds.`"
          >
            <Input
              id="setting-sync-interval"
              v-model.number="form.pricing_sync_interval_secs"
              type="number"
              min="60"
              step="60"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-source-url"
            label="Source URL"
            hint="A LiteLLM or models.dev catalog."
          >
            <Input
              id="setting-source-url"
              v-model="form.pricing_source_url"
              type="url"
              placeholder="https://…/model_prices_and_context_window.json"
              :disabled="loading"
            />
          </SettingsField>
        </div>
      </SettingToggle>

      <p class="text-xs text-muted-foreground">
        Rates entered on the Pricing page win over anything this sync writes.
      </p>
    </div>
  </SettingsSection>
</template>
