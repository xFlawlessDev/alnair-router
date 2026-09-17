<script setup lang="ts">
import { Gauge } from "@lucide/vue";

import SettingsField from "@/components/settings/SettingsField.vue";
import SettingsSection from "@/components/settings/SettingsSection.vue";
import { Input } from "@/components/ui/input";
import { useSettingsForm } from "@/lib/settings/context";
import { SETTINGS_SECTIONS } from "@/lib/settings/sections";

const { form, loading } = useSettingsForm();

const section = SETTINGS_SECTIONS.find((entry) => entry.id === "limits")!;
</script>

<template>
  <SettingsSection :section="section">
    <template #title>
      <Gauge class="size-4 text-muted-foreground" />
      {{ section.label }}
    </template>

    <div class="grid gap-6">
      <section class="grid gap-3">
        <div>
          <h4 class="text-sm font-medium">Concurrency</h4>
          <p class="mt-0.5 text-xs text-muted-foreground">
            Caps how many requests the router holds in flight. Zero disables the
            matching cap.
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-3">
          <SettingsField
            id="setting-max-concurrent"
            label="Max concurrent upstream calls"
            hint="Across every connection."
          >
            <Input
              id="setting-max-concurrent"
              v-model.number="form.max_concurrent"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-max-per-connection"
            label="Max concurrent per connection"
            hint="One connection's own share."
          >
            <Input
              id="setting-max-per-connection"
              v-model.number="form.max_concurrent_per_connection"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-acquire-timeout"
            label="Slot wait timeout (ms)"
            hint="How long a request waits for a free slot. 0 waits forever."
          >
            <Input
              id="setting-acquire-timeout"
              v-model.number="form.acquire_timeout_ms"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>
        </div>
      </section>

      <section class="grid gap-3 border-t pt-6">
        <div>
          <h4 class="text-sm font-medium">Rate limiting</h4>
          <p class="mt-0.5 text-xs text-muted-foreground">
            Defaults for client keys; individual keys and plans can override
            them. Zero is unlimited.
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <SettingsField
            id="setting-rpm"
            label="Requests per minute"
            hint="Steady-state allowance per key."
          >
            <Input
              id="setting-rpm"
              v-model.number="form.requests_per_minute"
              type="number"
              min="0"
              :disabled="loading"
            />
          </SettingsField>

          <SettingsField
            id="setting-burst"
            label="Burst capacity"
            hint="0 allows one minute's worth of tokens (the value above)."
          >
            <Input
              id="setting-burst"
              v-model.number="form.burst"
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
