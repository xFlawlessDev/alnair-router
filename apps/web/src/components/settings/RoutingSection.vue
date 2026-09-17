<script setup lang="ts">
import { Route } from "@lucide/vue";

import SettingsField from "@/components/settings/SettingsField.vue";
import SettingsSection from "@/components/settings/SettingsSection.vue";
import { Input } from "@/components/ui/input";
import { useSettingsForm } from "@/lib/settings/context";
import { SETTINGS_SECTIONS } from "@/lib/settings/sections";

const { form, loading } = useSettingsForm();

const section = SETTINGS_SECTIONS.find((entry) => entry.id === "routing")!;
</script>

<template>
  <SettingsSection :section="section">
    <template #title>
      <Route class="size-4 text-muted-foreground" />
      {{ section.label }}
    </template>

    <div class="grid gap-6">
      <SettingsField
        id="setting-default-connection"
        label="Default connection"
        hint="Connection used for bare model ids that name no provider prefix. Blank means such a request fails instead of falling back."
      >
        <Input
          id="setting-default-connection"
          v-model="form.default_connection"
          placeholder="e.g. openai-main"
          :disabled="loading"
        />
      </SettingsField>

      <section class="grid gap-3 border-t pt-6">
        <div>
          <h4 class="text-sm font-medium">Fallback</h4>
          <p class="mt-0.5 text-xs text-muted-foreground">
            How a failing request walks through the candidate connections.
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-3">
          <SettingsField
            id="setting-max-attempts"
            label="Max fallback tiers"
            hint="Connections tried per request."
          >
            <Input
              id="setting-max-attempts"
              v-model.number="form.max_attempts"
              type="number"
              min="1"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-max-retries"
            label="Retries per tier"
            hint="Retries before moving on."
          >
            <Input
              id="setting-max-retries"
              v-model.number="form.max_retries_per_tier"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-retry-delay"
            label="Max retry delay (ms)"
            hint="Upper bound on backoff."
          >
            <Input
              id="setting-retry-delay"
              v-model.number="form.max_retry_delay_ms"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>
        </div>
      </section>

      <section class="grid gap-3 border-t pt-6">
        <div>
          <h4 class="text-sm font-medium">Timeouts</h4>
          <p class="mt-0.5 text-xs text-muted-foreground">
            Applied to every upstream call unless a connection overrides it.
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-3">
          <SettingsField
            id="setting-connect-timeout"
            label="Connect timeout (ms)"
            hint="Time to first byte."
          >
            <Input
              id="setting-connect-timeout"
              v-model.number="form.connect_timeout_ms"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-idle-timeout"
            label="Stream idle timeout (ms)"
            hint="Gap allowed mid-stream."
          >
            <Input
              id="setting-idle-timeout"
              v-model.number="form.idle_timeout_ms"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-catalog-ttl"
            label="Catalog cache TTL (ms)"
            hint="How long a fetched model list is reused before the next refresh."
          >
            <Input
              id="setting-catalog-ttl"
              v-model.number="form.catalog_ttl_ms"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>
        </div>
      </section>
    </div>
  </SettingsSection>
</template>
