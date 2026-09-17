<script setup lang="ts">
import { CircleAlert, CircleCheck, RefreshCw } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { RouterLink } from "vue-router";

import SegmentedControl from "@/components/settings/SegmentedControl.vue";
import SettingToggle from "@/components/settings/SettingToggle.vue";
import SettingsField from "@/components/settings/SettingsField.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ApiError, api } from "@/lib/api";
import { useSettingsForm } from "@/lib/settings/context";
import {
  CAVEMAN_LEVELS,
  OUTPUT_DIRECTIVES,
  PONYTAIL_LEVELS,
  SLIMMER_LEVELS,
  applyOutputDirective,
  readOutputDirective,
  type OutputDirective,
} from "@/lib/settings/form";
import type { HeadroomTestResult } from "@/types/api";

const { form, loading } = useSettingsForm();

const headroomTesting = ref(false);
const headroomResult = ref<HeadroomTestResult | null>(null);

const directive = computed({
  get: () => readOutputDirective(form),
  set: (value: string) => applyOutputDirective(form, value as OutputDirective),
});

const PIPELINE = [
  "normalize",
  "RTK",
  "Headroom",
  "Terse / Caveman",
  "Ponytail",
];

/**
 * Probes the URL currently typed, so an unsaved value can be checked before it
 * is committed.
 */
async function testHeadroom(): Promise<void> {
  headroomTesting.value = true;
  headroomResult.value = null;
  try {
    headroomResult.value = await api.testHeadroom(
      form.headroom_url.trim() || undefined,
    );
  } catch (caught) {
    headroomResult.value = {
      ok: false,
      message:
        caught instanceof ApiError ? caught.message : "Test request failed",
      latency_ms: 0,
    };
  } finally {
    headroomTesting.value = false;
  }
}

// A result for a URL that has since been edited is misleading, so drop it.
watch(
  () => [form.headroom_url, form.headroom_enabled],
  () => {
    headroomResult.value = null;
  },
);
</script>

<template>
  <div class="grid gap-3">
    <SettingToggle control-id="setting-slimmer" title="RTK / Slimmer">
      <template #description>
        Locally shrinks diffs, grep results, listings and build logs. It never
        rewrites ordinary user prose or error traces.
      </template>
      <template #control>
        <div class="flex items-center gap-2">
          <Badge variant="outline" class="font-normal">Measured</Badge>
          <Switch
            id="setting-slimmer"
            v-model="form.slimmer_enabled"
            :disabled="loading"
          />
        </div>
      </template>

      <SettingsField
        v-if="form.slimmer_enabled"
        label="Compression level"
        as-group
      >
        <Select v-model="form.slimmer_level" :disabled="loading">
          <SelectTrigger id="setting-slimmer-level" class="w-full sm:max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="level in SLIMMER_LEVELS"
              :key="level.value"
              :value="level.value"
            >
              {{ level.label }} — {{ level.hint }}
            </SelectItem>
          </SelectContent>
        </Select>
      </SettingsField>
    </SettingToggle>

    <SettingToggle control-id="setting-headroom" title="Headroom proxy">
      <template #description>
        Deeper compression through <code class="text-xs">/v1/compress</code>.
        Fail-open: if it is unavailable, the original request continues.
      </template>
      <template #control>
        <div class="flex items-center gap-2">
          <Badge variant="outline" class="font-normal">Measured</Badge>
          <Switch
            id="setting-headroom"
            v-model="form.headroom_enabled"
            :disabled="loading"
          />
        </div>
      </template>

      <div
        v-if="form.headroom_enabled"
        class="grid gap-4 sm:grid-cols-[1fr_10rem]"
      >
        <SettingsField id="setting-headroom-url" label="Proxy URL">
          <Input
            id="setting-headroom-url"
            v-model="form.headroom_url"
            placeholder="http://localhost:8787"
            :disabled="loading"
          />
        </SettingsField>

        <SettingsField id="setting-headroom-timeout" label="Timeout (ms)">
          <Input
            id="setting-headroom-timeout"
            v-model.number="form.headroom_timeout_ms"
            type="number"
            min="1"
            :disabled="loading"
          />
        </SettingsField>

        <div class="flex flex-wrap items-center gap-3 sm:col-span-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            :disabled="headroomTesting || loading"
            @click="testHeadroom"
          >
            <RefreshCw :class="headroomTesting ? 'animate-spin' : ''" />
            Test connection
          </Button>
          <span
            v-if="headroomResult"
            class="flex items-center gap-1.5 text-xs"
            :class="headroomResult.ok ? 'text-emerald-500' : 'text-amber-500'"
          >
            <CircleCheck v-if="headroomResult.ok" class="size-3.5" />
            <CircleAlert v-else class="size-3.5" />
            {{ headroomResult.message }}
            <span class="text-muted-foreground">
              ({{ headroomResult.latency_ms }} ms)
            </span>
          </span>
        </div>

        <p class="text-xs leading-relaxed text-muted-foreground sm:col-span-2">
          Needs the Python CLI running; see <strong>Running Headroom</strong>{"
          "} below for the install and start commands. The npm package is a
          library, not the CLI.
        </p>
      </div>
    </SettingToggle>

    <SettingToggle
      title="Output directive"
      description="Rewrites the system prompt to ask for a smaller answer. Pick one; Terse and Caveman both write a system directive, so they cannot stack. Savings are estimates."
    >
      <template #control>
        <Badge variant="outline" class="font-normal">Estimated</Badge>
      </template>

      <SegmentedControl
        v-model="directive"
        :options="OUTPUT_DIRECTIVES"
        :disabled="loading"
        group-label="Output directive"
        class="mb-3.5"
      />

      <SettingsField
        v-if="directive === 'caveman'"
        id="setting-caveman-level"
        label="Caveman level"
      >
        <Select v-model="form.caveman_level" :disabled="loading">
          <SelectTrigger id="setting-caveman-level" class="w-full sm:max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="level in CAVEMAN_LEVELS"
              :key="level.value"
              :value="level.value"
            >
              {{ level.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </SettingsField>
    </SettingToggle>

    <SettingToggle
      control-id="setting-ponytail"
      title="Ponytail"
      description="Layers a lazy-senior-dev prompt on the directive above. It pushes toward the smallest useful diff without dropping validation, security or requested work."
    >
      <template #control>
        <div class="flex items-center gap-2">
          <Badge variant="outline" class="font-normal">Estimated</Badge>
          <Switch
            id="setting-ponytail"
            v-model="form.ponytail_enabled"
            :disabled="loading"
          />
        </div>
      </template>

      <SettingsField
        v-if="form.ponytail_enabled"
        id="setting-ponytail-level"
        label="Ponytail level"
      >
        <Select v-model="form.ponytail_level" :disabled="loading">
          <SelectTrigger id="setting-ponytail-level" class="w-full sm:max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="level in PONYTAIL_LEVELS"
              :key="level.value"
              :value="level.value"
            >
              {{ level.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </SettingsField>
    </SettingToggle>

    <div
      class="flex flex-wrap items-center gap-x-2 gap-y-2 rounded-lg bg-muted/50 px-4 py-3 text-xs text-muted-foreground"
    >
      <span class="font-medium text-foreground">Order</span>
      <template v-for="(step, index) in PIPELINE" :key="step">
        <span v-if="index > 0" aria-hidden="true">→</span>
        <code>{{ step }}</code>
      </template>
      <span class="ms-auto text-pretty"
        >Saved once — the same pipeline applies to every provider.</span
      >
    </div>

    <p class="text-xs text-muted-foreground">
      Try any pipeline on one request in the
      <RouterLink
        to="/token-saver/playground"
        class="font-medium text-foreground underline underline-offset-4"
        >playground</RouterLink
      >.
    </p>
  </div>
</template>
