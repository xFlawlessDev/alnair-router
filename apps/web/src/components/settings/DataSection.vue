<script setup lang="ts">
import { DatabaseBackup, Download, ServerCog, Upload } from "@lucide/vue";
import { computed, ref } from "vue";
import { toast } from "vue-sonner";

import ConfirmDialog from "@/components/ConfirmDialog.vue";
import SettingsSection from "@/components/settings/SettingsSection.vue";
import { Button } from "@/components/ui/button";
import { ApiError, api } from "@/lib/api";
import { useSettingsForm } from "@/lib/settings/context";
import { SETTINGS_SECTIONS } from "@/lib/settings/sections";

const { settings, loading, reload } = useSettingsForm();

const section = SETTINGS_SECTIONS.find((entry) => entry.id === "data")!;
const deployment = computed(() => settings.value?.deployment ?? null);

const backingUp = ref(false);
const restoring = ref(false);
const restoreOpen = ref(false);
const restoreFile = ref<File | null>(null);
const fileInput = ref<HTMLInputElement | null>(null);

async function downloadBackup(): Promise<void> {
  backingUp.value = true;
  try {
    const blob = await api.downloadBackup();
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    link.href = url;
    link.download = `alnair-router-backup-${stamp}.sqlite`;
    link.click();
    URL.revokeObjectURL(url);
    toast.success("Backup downloaded");
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : "Backup failed");
  } finally {
    backingUp.value = false;
  }
}

function pickRestoreFile(event: Event): void {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0] ?? null;
  input.value = "";
  if (!file) return;
  restoreFile.value = file;
  restoreOpen.value = true;
}

async function restore(): Promise<void> {
  const file = restoreFile.value;
  if (!file) return;
  restoring.value = true;
  try {
    const summary = await api.restoreBackup(file);
    restoreOpen.value = false;
    restoreFile.value = null;
    await reload();
    toast.success(
      `Restored ${summary.total_rows} rows across ${summary.tables.length} tables`,
    );
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : "Restore failed");
  } finally {
    restoring.value = false;
  }
}

/** Read-only deployment facts, rendered as a definition list. */
const facts = computed(() => {
  const info = deployment.value;
  if (!info) return [];
  return [
    { label: "Listen address", value: `${info.host}:${info.port}`, mono: true },
    { label: "Loopback only", value: info.binds_loopback ? "Yes" : "No" },
    { label: "Serve dashboard", value: info.serve_dashboard ? "Yes" : "No" },
    { label: "System tray", value: info.tray ? "Yes" : "No" },
    {
      label: "Allow unauthenticated admin",
      value: info.allow_unauthenticated_admin ? "Yes" : "No",
    },
    {
      label: "Encryption key",
      value: info.secrets_key_set ? "Configured" : "Missing",
    },
    { label: "Database", value: info.database_url, mono: true, wide: true },
  ];
});
</script>

<template>
  <div class="grid gap-6">
    <SettingsSection :section="section">
      <template #title>
        <DatabaseBackup class="size-4 text-muted-foreground" />
        Backup &amp; restore
      </template>
      <template #description>
        Download every connection, alias, combo, key, plan, usage record and
        price rate as a SQLite snapshot, or import one to replace them. Runtime
        settings and the admin token are not part of a backup.
      </template>

      <div class="grid gap-3">
        <div class="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            :disabled="backingUp || restoring || loading"
            @click="downloadBackup"
          >
            <Download /> {{ backingUp ? "Preparing…" : "Download backup" }}
          </Button>
          <input
            ref="fileInput"
            type="file"
            accept=".sqlite,.db,application/octet-stream"
            class="hidden"
            @change="pickRestoreFile"
          />
          <Button
            variant="outline"
            :disabled="backingUp || restoring || loading"
            @click="fileInput?.click()"
          >
            <Upload /> {{ restoring ? "Importing…" : "Import backup" }}
          </Button>
        </div>
        <p class="text-xs leading-relaxed text-muted-foreground">
          Imports replace all data tables in one transaction. An interrupted
          import leaves the database unchanged.
        </p>
      </div>
    </SettingsSection>

    <SettingsSection v-if="deployment" :section="section">
      <template #title>
        <ServerCog class="size-4 text-muted-foreground" />
        Deployment
      </template>
      <template #description>
        Read-only: these come from <code>config.toml</code> or environment
        variables and need a restart.
      </template>

      <dl class="grid gap-x-8 gap-y-2.5 text-xs">
        <div
          v-for="fact in facts"
          :key="fact.label"
          class="flex items-baseline justify-between gap-4 border-b border-border/60 pb-2.5"
          :class="fact.wide && 'sm:col-span-2'"
        >
          <dt class="shrink-0 text-muted-foreground">{{ fact.label }}</dt>
          <dd
            class="min-w-0 truncate text-right"
            :class="fact.mono && 'font-mono'"
            :title="String(fact.value)"
          >
            {{ fact.value }}
          </dd>
        </div>
      </dl>
    </SettingsSection>

    <ConfirmDialog
      v-model:open="restoreOpen"
      title="Import this backup?"
      :description="`Every connection, alias, combo, key, plan, usage record and price rate is replaced by the contents of ${restoreFile?.name ?? 'the selected file'}. Runtime settings and the admin token stay unchanged. This cannot be undone.`"
      confirm-label="Import backup"
      pending-label="Importing…"
      :pending="restoring"
      @confirm="restore"
    />
  </div>
</template>
