<script setup lang="ts">
import {
  DatabaseBackup,
  Download,
  RefreshCw,
  RotateCcw,
  Save,
  Upload,
} from "@lucide/vue";
import { computed, onMounted, reactive, ref } from "vue";
import { toast } from "vue-sonner";

import ConfirmDialog from "@/components/ConfirmDialog.vue";
import PageHeader from "@/components/PageHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { ApiError, api } from "@/lib/api";
import { setAdminToken } from "@/lib/adminToken";
import type { SettingsPatch, SettingsResponse } from "@/types/api";

const settings = ref<SettingsResponse | null>(null);
const loading = ref(true);
const saving = ref(false);
const resetOpen = ref(false);
const resetting = ref(false);
const backingUp = ref(false);
const restoreOpen = ref(false);
const restoring = ref(false);
const restoreFile = ref<File | null>(null);
const fileInput = ref<HTMLInputElement | null>(null);
const error = ref<string | null>(null);

/**
 * Editable mirror of `GET /api/settings`. `admin_token` always starts blank:
 * the server never returns the stored secret, and blank means "keep it".
 */
const form = reactive({
  require_api_key: false,
  admin_token_enabled: false,
  admin_token: "",
  readiness_upstream_checks: false,
  public_usage: true,
  lan_access: false,
  cors_origins: "",
  default_connection: "",
  max_attempts: 5,
  max_retries_per_tier: 2,
  max_retry_delay_ms: 30000,
  catalog_ttl_ms: 1000,
  connect_timeout_ms: 10000,
  idle_timeout_ms: 60000,
  max_concurrent: 0,
  max_concurrent_per_connection: 0,
  acquire_timeout_ms: 30000,
  requests_per_minute: 0,
  burst: 0,
  pricing_sync_enabled: false,
  pricing_sync_interval_secs: 86400,
  pricing_source_url: "",
});

const deployment = computed(() => settings.value?.deployment ?? null);

/** Clearing the admin token on a public bind would reopen the admin API. */
const adminTokenLocked = computed(() => {
  const info = deployment.value;
  return (
    info !== null && !info.binds_loopback && !info.allow_unauthenticated_admin
  );
});

function hydrate(response: SettingsResponse): void {
  settings.value = response;
  form.require_api_key = response.server.require_api_key;
  form.admin_token_enabled = response.server.admin_token_set;
  form.admin_token = "";
  form.readiness_upstream_checks = response.server.readiness_upstream_checks;
  form.public_usage = response.server.public_usage;
  form.lan_access = response.server.lan_access;
  form.cors_origins = response.server.cors_origins.join("\n");
  form.default_connection = response.router.default_connection ?? "";
  form.max_attempts = response.router.max_attempts;
  form.max_retries_per_tier = response.router.max_retries_per_tier;
  form.max_retry_delay_ms = response.router.max_retry_delay_ms;
  form.catalog_ttl_ms = response.router.catalog_ttl_ms;
  form.connect_timeout_ms = response.router.connect_timeout_ms;
  form.idle_timeout_ms = response.router.idle_timeout_ms;
  form.max_concurrent = response.limits.max_concurrent;
  form.max_concurrent_per_connection =
    response.limits.max_concurrent_per_connection;
  form.acquire_timeout_ms = response.limits.acquire_timeout_ms;
  form.requests_per_minute = response.rate_limit.requests_per_minute;
  form.burst = response.rate_limit.burst;
  form.pricing_sync_enabled = response.pricing.sync_enabled;
  form.pricing_sync_interval_secs = response.pricing.sync_interval_secs;
  form.pricing_source_url = response.pricing.source_url;
}

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    hydrate(await api.settings());
  } catch (caught) {
    error.value =
      caught instanceof ApiError ? caught.message : "Failed to load settings";
  } finally {
    loading.value = false;
  }
}

function toInt(value: unknown, minimum = 0): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return minimum;
  return Math.max(minimum, Math.trunc(parsed));
}

/** Sends only the fields that actually changed. */
function buildPatch(): SettingsPatch {
  const current = settings.value;
  if (!current) return {};
  const patch: SettingsPatch = {};

  if (form.require_api_key !== current.server.require_api_key) {
    patch.require_api_key = form.require_api_key;
  }
  if (!form.admin_token_enabled) {
    if (current.server.admin_token_set) patch.admin_token = null;
  } else if (form.admin_token.trim()) {
    patch.admin_token = form.admin_token.trim();
  }
  if (
    form.readiness_upstream_checks !== current.server.readiness_upstream_checks
  ) {
    patch.readiness_upstream_checks = form.readiness_upstream_checks;
  }
  if (form.public_usage !== current.server.public_usage) {
    patch.public_usage = form.public_usage;
  }
  if (form.lan_access !== current.server.lan_access) {
    patch.lan_access = form.lan_access;
  }

  const origins = form.cors_origins
    .split(/[\n,]/)
    .map((value) => value.trim())
    .filter((value) => value !== "");
  if (origins.join("\n") !== current.server.cors_origins.join("\n")) {
    patch.cors_origins = origins;
  }

  const connection = form.default_connection.trim();
  if (connection !== (current.router.default_connection ?? "")) {
    patch.default_connection = connection === "" ? null : connection;
  }

  const numbers: Array<[keyof SettingsPatch, number, number]> = [
    ["max_attempts", toInt(form.max_attempts, 1), current.router.max_attempts],
    [
      "max_retries_per_tier",
      toInt(form.max_retries_per_tier),
      current.router.max_retries_per_tier,
    ],
    [
      "max_retry_delay_ms",
      toInt(form.max_retry_delay_ms),
      current.router.max_retry_delay_ms,
    ],
    [
      "catalog_ttl_ms",
      toInt(form.catalog_ttl_ms),
      current.router.catalog_ttl_ms,
    ],
    [
      "connect_timeout_ms",
      toInt(form.connect_timeout_ms),
      current.router.connect_timeout_ms,
    ],
    [
      "idle_timeout_ms",
      toInt(form.idle_timeout_ms),
      current.router.idle_timeout_ms,
    ],
    [
      "max_concurrent",
      toInt(form.max_concurrent),
      current.limits.max_concurrent,
    ],
    [
      "max_concurrent_per_connection",
      toInt(form.max_concurrent_per_connection),
      current.limits.max_concurrent_per_connection,
    ],
    [
      "acquire_timeout_ms",
      toInt(form.acquire_timeout_ms),
      current.limits.acquire_timeout_ms,
    ],
    [
      "requests_per_minute",
      toInt(form.requests_per_minute),
      current.rate_limit.requests_per_minute,
    ],
    ["burst", toInt(form.burst), current.rate_limit.burst],
    [
      "pricing_sync_interval_secs",
      toInt(form.pricing_sync_interval_secs),
      current.pricing.sync_interval_secs,
    ],
  ];
  for (const [key, value, original] of numbers) {
    if (value !== original) {
      (patch as Record<string, number>)[key] = value;
    }
  }

  if (form.pricing_sync_enabled !== current.pricing.sync_enabled) {
    patch.pricing_sync_enabled = form.pricing_sync_enabled;
  }
  if (form.pricing_source_url.trim() !== current.pricing.source_url) {
    patch.pricing_source_url = form.pricing_source_url.trim();
  }

  return patch;
}

async function save(): Promise<void> {
  const current = settings.value;
  if (!current) return;
  if (
    form.admin_token_enabled &&
    !current.server.admin_token_set &&
    !form.admin_token.trim()
  ) {
    toast.error("Enter an admin token or turn the requirement off");
    return;
  }

  const patch = buildPatch();
  if (Object.keys(patch).length === 0) {
    toast.info("No changes to save");
    return;
  }

  saving.value = true;
  try {
    const response = await api.updateSettings(patch);
    if (typeof patch.admin_token === "string") setAdminToken(patch.admin_token);
    if (patch.admin_token === null) setAdminToken("");
    hydrate(response);
    toast.success("Settings saved");
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to save settings",
    );
  } finally {
    saving.value = false;
  }
}

async function reset(): Promise<void> {
  resetting.value = true;
  try {
    hydrate(await api.resetSettings());
    resetOpen.value = false;
    toast.success("Overrides cleared; file configuration restored");
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to reset settings",
    );
  } finally {
    resetting.value = false;
  }
}

function sectionOverridden(prefix: string): boolean {
  return (
    settings.value?.overrides.some((key) => key.startsWith(`${prefix}.`)) ??
    false
  );
}

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

function openRestorePicker(): void {
  fileInput.value?.click();
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
    await load();
    toast.success(
      `Restored ${summary.total_rows} rows across ${summary.tables.length} tables`,
    );
  } catch (caught) {
    toast.error(caught instanceof ApiError ? caught.message : "Restore failed");
  } finally {
    restoring.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-8">
    <PageHeader
      title="Settings"
      description="Runtime configuration stored in the router database. Changes apply immediately and survive restarts."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading || saving" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button
          variant="outline"
          :disabled="
            loading || saving || !settings || settings.overrides.length === 0
          "
          @click="resetOpen = true"
        >
          <RotateCcw /> Reset overrides
        </Button>
        <Button :disabled="loading || saving || !settings" @click="save">
          <Save /> {{ saving ? "Saving…" : "Save" }}
        </Button>
      </template>
    </PageHeader>

    <Card v-if="error" class="border-destructive/40">
      <CardHeader>
        <CardTitle class="text-destructive">Cannot load settings</CardTitle>
        <CardDescription>{{ error }}</CardDescription>
      </CardHeader>
    </Card>

    <template v-else>
      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            Security
            <Badge v-if="sectionOverridden('server')" variant="secondary"
              >Customized</Badge
            >
          </CardTitle>
          <CardDescription>
            Client keys and admin access. The admin token never leaves the
            router once saved.
          </CardDescription>
        </CardHeader>
        <CardContent class="grid gap-5">
          <div
            class="flex items-start justify-between gap-4 rounded-md border p-4"
          >
            <div class="space-y-1">
              <Label for="setting-require-api-key"
                >Require client API key</Label
              >
              <p class="text-xs text-muted-foreground">
                Every <code>/v1</code> request must carry
                <code>Authorization: Bearer &lt;key&gt;</code>, using a key from
                the API Keys page.
              </p>
            </div>
            <Switch
              id="setting-require-api-key"
              v-model="form.require_api_key"
            />
          </div>

          <div class="grid gap-2 rounded-md border p-4">
            <div class="flex items-start justify-between gap-4">
              <div class="space-y-1">
                <Label for="setting-admin-token-enabled"
                  >Require admin token</Label
                >
                <p class="text-xs text-muted-foreground">
                  Guards <code>/api/*</code> with a bearer token. Enforced
                  everywhere once set, even on loopback.
                  <span v-if="adminTokenLocked" class="text-destructive">
                    Cannot be disabled on a non-loopback bind without
                    <code>allow_unauthenticated_admin</code>.
                  </span>
                </p>
              </div>
              <Switch
                id="setting-admin-token-enabled"
                v-model="form.admin_token_enabled"
                :disabled="adminTokenLocked"
              />
            </div>
            <div v-if="form.admin_token_enabled" class="grid gap-2 pt-2">
              <Label for="setting-admin-token">New admin token</Label>
              <Input
                id="setting-admin-token"
                v-model="form.admin_token"
                type="password"
                autocomplete="off"
                :placeholder="
                  settings?.server.admin_token_set
                    ? 'Leave blank to keep the current token'
                    : 'Enter a token'
                "
              />
              <p class="text-xs text-muted-foreground">
                Saving a new token stores it in this browser too.
              </p>
            </div>
          </div>

          <div
            class="flex items-start justify-between gap-4 rounded-md border p-4"
          >
            <div class="space-y-1">
              <Label for="setting-readiness-checks"
                >Check upstreams on /api/ready</Label
              >
              <p class="text-xs text-muted-foreground">
                Adds best-effort TCP reachability of each enabled connection to
                the readiness probe.
              </p>
            </div>
            <Switch
              id="setting-readiness-checks"
              v-model="form.readiness_upstream_checks"
            />
          </div>

          <div
            class="flex items-start justify-between gap-4 rounded-md border p-4"
          >
            <div class="space-y-1">
              <Label for="setting-public-usage">Self-service usage page</Label>
              <p class="text-xs text-muted-foreground">
                Exposes <code>/me</code> and <code>/api/public/usage</code>,
                where a client reads its own rollup with a router-issued API
                key. No key, no data.
              </p>
            </div>
            <Switch id="setting-public-usage" v-model="form.public_usage" />
          </div>

          <div class="grid gap-2 rounded-md border p-4">
            <div class="space-y-1">
              <Label for="setting-cors">CORS origins</Label>
              <p class="text-xs text-muted-foreground">
                Browser origins allowed to call the API cross-origin, one per
                line. Empty emits no CORS headers; <code>*</code> allows any
                origin.
              </p>
            </div>
            <Textarea
              id="setting-cors"
              v-model="form.cors_origins"
              rows="3"
              class="font-mono text-xs"
              placeholder="https://app.example.com"
            />
          </div>

          <div
            class="flex items-start justify-between gap-4 rounded-md border p-4"
          >
            <div class="space-y-1">
              <Label for="setting-lan-access">LAN access</Label>
              <p class="text-xs text-muted-foreground">
                Bind every interface so other devices on the network can reach
                the router; the listener re-binds immediately. Admin routes then
                require the dashboard password.
              </p>
            </div>
            <Switch id="setting-lan-access" v-model="form.lan_access" />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            Routing
            <Badge v-if="sectionOverridden('router')" variant="secondary"
              >Customized</Badge
            >
          </CardTitle>
          <CardDescription
            >Fallback behaviour and upstream timeouts.</CardDescription
          >
        </CardHeader>
        <CardContent class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <div class="grid gap-2 sm:col-span-2 lg:col-span-3">
            <Label for="setting-default-connection">Default connection</Label>
            <Input
              id="setting-default-connection"
              v-model="form.default_connection"
              placeholder="Connection name for bare model ids; blank means none"
            />
          </div>
          <div class="grid gap-2">
            <Label for="setting-max-attempts">Max fallback tiers</Label>
            <Input
              id="setting-max-attempts"
              v-model.number="form.max_attempts"
              type="number"
              min="1"
            />
          </div>
          <div class="grid gap-2">
            <Label for="setting-max-retries">Retries per tier</Label>
            <Input
              id="setting-max-retries"
              v-model.number="form.max_retries_per_tier"
              type="number"
              min="0"
            />
          </div>
          <div class="grid gap-2">
            <Label for="setting-retry-delay">Max retry delay (ms)</Label>
            <Input
              id="setting-retry-delay"
              v-model.number="form.max_retry_delay_ms"
              type="number"
              min="0"
            />
          </div>
          <div class="grid gap-2">
            <Label for="setting-catalog-ttl">Catalog cache TTL (ms)</Label>
            <Input
              id="setting-catalog-ttl"
              v-model.number="form.catalog_ttl_ms"
              type="number"
              min="0"
            />
          </div>
          <div class="grid gap-2">
            <Label for="setting-connect-timeout">Connect timeout (ms)</Label>
            <Input
              id="setting-connect-timeout"
              v-model.number="form.connect_timeout_ms"
              type="number"
              min="0"
            />
          </div>
          <div class="grid gap-2">
            <Label for="setting-idle-timeout">Stream idle timeout (ms)</Label>
            <Input
              id="setting-idle-timeout"
              v-model.number="form.idle_timeout_ms"
              type="number"
              min="0"
            />
          </div>
        </CardContent>
      </Card>

      <div class="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle class="flex items-center gap-2">
              Concurrency limits
              <Badge v-if="sectionOverridden('limits')" variant="secondary"
                >Customized</Badge
              >
            </CardTitle>
            <CardDescription>Zero disables the matching cap.</CardDescription>
          </CardHeader>
          <CardContent class="grid gap-4">
            <div class="grid gap-2">
              <Label for="setting-max-concurrent"
                >Max concurrent upstream calls</Label
              >
              <Input
                id="setting-max-concurrent"
                v-model.number="form.max_concurrent"
                type="number"
                min="0"
              />
            </div>
            <div class="grid gap-2">
              <Label for="setting-max-per-connection"
                >Max concurrent per connection</Label
              >
              <Input
                id="setting-max-per-connection"
                v-model.number="form.max_concurrent_per_connection"
                type="number"
                min="0"
              />
            </div>
            <div class="grid gap-2">
              <Label for="setting-acquire-timeout"
                >Slot wait timeout (ms)</Label
              >
              <Input
                id="setting-acquire-timeout"
                v-model.number="form.acquire_timeout_ms"
                type="number"
                min="0"
              />
              <p class="text-xs text-muted-foreground">0 waits forever.</p>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle class="flex items-center gap-2">
              Rate limits
              <Badge v-if="sectionOverridden('rate_limit')" variant="secondary"
                >Customized</Badge
              >
            </CardTitle>
            <CardDescription>
              Defaults for client keys; individual keys and plans can override
              them.
            </CardDescription>
          </CardHeader>
          <CardContent class="grid gap-4">
            <div class="grid gap-2">
              <Label for="setting-rpm">Requests per minute</Label>
              <Input
                id="setting-rpm"
                v-model.number="form.requests_per_minute"
                type="number"
                min="0"
              />
              <p class="text-xs text-muted-foreground">0 is unlimited.</p>
            </div>
            <div class="grid gap-2">
              <Label for="setting-burst">Burst capacity</Label>
              <Input
                id="setting-burst"
                v-model.number="form.burst"
                type="number"
                min="0"
              />
              <p class="text-xs text-muted-foreground">
                0 uses one minute's worth of tokens (the requests-per-minute
                value).
              </p>
            </div>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            Pricing catalog
            <Badge v-if="sectionOverridden('pricing')" variant="secondary"
              >Customized</Badge
            >
          </CardTitle>
          <CardDescription>
            Background crawl of a LiteLLM or models.dev catalog; dashboard
            overrides always win.
          </CardDescription>
        </CardHeader>
        <CardContent class="grid gap-4">
          <div
            class="flex items-start justify-between gap-4 rounded-md border p-4"
          >
            <div class="space-y-1">
              <Label for="setting-pricing-sync">Sync pricing catalog</Label>
              <p class="text-xs text-muted-foreground">
                Crawl the source URL in the background on the configured
                interval.
              </p>
            </div>
            <Switch
              id="setting-pricing-sync"
              v-model="form.pricing_sync_enabled"
            />
          </div>
          <div class="grid gap-4 sm:grid-cols-2">
            <div class="grid gap-2">
              <Label for="setting-sync-interval">Sync interval (seconds)</Label>
              <Input
                id="setting-sync-interval"
                v-model.number="form.pricing_sync_interval_secs"
                type="number"
                min="60"
              />
              <p class="text-xs text-muted-foreground">
                Clamped to at least 60 seconds.
              </p>
            </div>
            <div class="grid gap-2 sm:col-span-2">
              <Label for="setting-source-url">Source URL</Label>
              <Input
                id="setting-source-url"
                v-model="form.pricing_source_url"
                type="url"
              />
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="flex items-center gap-2">
            <DatabaseBackup class="size-4" /> Backup
          </CardTitle>
          <CardDescription>
            Download every connection, alias, combo, key, plan, usage record and
            price rate as a SQLite snapshot, or import one to replace them.
            Runtime settings and the admin token are not part of a backup.
          </CardDescription>
        </CardHeader>
        <CardContent class="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            :disabled="backingUp || restoring"
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
            :disabled="backingUp || restoring"
            @click="openRestorePicker"
          >
            <Upload /> {{ restoring ? "Importing…" : "Import backup" }}
          </Button>
          <p class="w-full text-xs text-muted-foreground">
            Imports replace all data tables in one transaction. An interrupted
            import leaves the database unchanged.
          </p>
        </CardContent>
      </Card>

      <Card v-if="deployment">
        <CardHeader>
          <CardTitle>Deployment</CardTitle>
          <CardDescription>
            Read-only: these come from <code>config.toml</code> or environment
            variables and need a restart.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <dl class="grid gap-x-8 gap-y-3 text-sm sm:grid-cols-2">
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">Listen address</dt>
              <dd class="font-mono">
                {{ deployment.host }}:{{ deployment.port }}
              </dd>
            </div>
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">Loopback only</dt>
              <dd>{{ deployment.binds_loopback ? "Yes" : "No" }}</dd>
            </div>
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">Serve dashboard</dt>
              <dd>{{ deployment.serve_dashboard ? "Yes" : "No" }}</dd>
            </div>
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">System tray</dt>
              <dd>{{ deployment.tray ? "Yes" : "No" }}</dd>
            </div>
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">Allow unauthenticated admin</dt>
              <dd>
                {{ deployment.allow_unauthenticated_admin ? "Yes" : "No" }}
              </dd>
            </div>
            <div class="flex items-center justify-between gap-4">
              <dt class="text-muted-foreground">Encryption key</dt>
              <dd>
                {{ deployment.secrets_key_set ? "Configured" : "Missing" }}
              </dd>
            </div>
            <div class="flex items-center justify-between gap-4 sm:col-span-2">
              <dt class="text-muted-foreground">Database</dt>
              <dd class="truncate font-mono" :title="deployment.database_url">
                {{ deployment.database_url }}
              </dd>
            </div>
          </dl>
        </CardContent>
      </Card>
    </template>

    <ConfirmDialog
      v-model:open="resetOpen"
      title="Reset all overrides?"
      description="Every dashboard-managed setting is removed and the file/env configuration applies again. If your config.toml defines an admin token, re-enter it from the key icon in the header."
      confirm-label="Reset overrides"
      pending-label="Resetting…"
      :pending="resetting"
      @confirm="reset"
    />

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
