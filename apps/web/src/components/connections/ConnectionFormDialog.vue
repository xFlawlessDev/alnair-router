<script setup lang="ts">
import { Plus, RefreshCw, Trash2, X } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { toast } from "vue-sonner";

import ProviderIcon from "@/components/ProviderIcon.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ApiError, api } from "@/lib/api";
import { formatDateTime, parseHeaders } from "@/lib/format";
import {
  PROVIDER_TYPES,
  type Connection,
  type ConnectionAccount,
  type ConnectionInput,
  type ProviderPreset,
  type ProviderType,
} from "@/types/api";

const props = defineProps<{
  open: boolean;
  connection: Connection | null;
  /** Preset to prefill from when creating. */
  preset?: ProviderPreset | null;
  /** Suggested name for a new connection (kept unique by the page). */
  defaultName?: string;
}>();
const emit = defineEmits<{
  "update:open": [boolean];
  saved: [];
  /** Reopen the picker so the user can switch the preset behind this form. */
  "change-provider": [];
}>();

interface HeaderRow {
  key: string;
  value: string;
}

const name = ref("");
const providerType = ref<ProviderType>("openai-compatible");
const baseUrl = ref("");
const apiKey = ref("");
const clearApiKey = ref(false);
const headers = ref<HeaderRow[]>([]);
const enabled = ref(true);
const connectTimeout = ref("");
const idleTimeout = ref("");
const pricingModel = ref("");
const cacheRetention = ref("none");
const saving = ref(false);

/** Extra keys (only meaningful once the connection exists). */
const accounts = ref<ConnectionAccount[]>([]);
const accountsLoading = ref(false);
const newAccountLabel = ref("");
const newAccountKey = ref("");
const addingAccount = ref(false);

const isEdit = computed(() => props.connection !== null);
const hasExistingKey = computed(() => Boolean(props.connection?.api_key));

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    const connection = props.connection;
    const preset = props.preset ?? null;

    name.value =
      connection?.name || props.defaultName?.trim() || presetName(preset) || "";
    providerType.value =
      connection?.provider_type ?? preset?.provider_type ?? "openai-compatible";
    baseUrl.value = connection?.base_url ?? preset?.base_url ?? "";
    apiKey.value = "";
    clearApiKey.value = false;
    headers.value = connection
      ? Object.entries(parseHeaders(connection.custom_headers)).map(
          ([key, value]) => ({ key, value }),
        )
      : Object.entries(preset?.default_headers ?? {}).map(([key, value]) => ({
          key,
          value,
        }));
    enabled.value = connection ? connection.enabled !== 0 : true;
    connectTimeout.value =
      connection?.connect_timeout_ms != null
        ? String(connection.connect_timeout_ms)
        : "";
    idleTimeout.value =
      connection?.idle_timeout_ms != null
        ? String(connection.idle_timeout_ms)
        : "";
    pricingModel.value = connection?.pricing_model ?? "";
    cacheRetention.value = connection?.cache_retention ?? "none";
    newAccountLabel.value = "";
    newAccountKey.value = "";
    accounts.value = [];
    if (connection) void loadAccounts();
  },
);

async function loadAccounts(): Promise<void> {
  if (!props.connection) return;
  accountsLoading.value = true;
  try {
    const list = await api.listConnectionAccounts(props.connection.id);
    accounts.value = Array.isArray(list) ? list : [];
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to load extra keys",
    );
  } finally {
    accountsLoading.value = false;
  }
}

async function addAccount(): Promise<void> {
  if (!props.connection) return;
  const label = newAccountLabel.value.trim();
  const key = newAccountKey.value.trim();
  if (!label || !key) {
    toast.error("A label and an API key are required");
    return;
  }

  addingAccount.value = true;
  try {
    await api.createConnectionAccount(props.connection.id, {
      label,
      api_key: key,
    });
    newAccountLabel.value = "";
    newAccountKey.value = "";
    await loadAccounts();
    emit("saved");
    toast.success(
      `Key “${label}” added — requests now rotate across enabled keys`,
    );
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to add the key",
    );
  } finally {
    addingAccount.value = false;
  }
}

async function toggleAccount(
  account: ConnectionAccount,
  enabled: boolean,
): Promise<void> {
  if (!props.connection) return;
  try {
    await api.updateConnectionAccount(props.connection.id, account.id, {
      enabled,
    });
    await loadAccounts();
    emit("saved");
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to update the key",
    );
  }
}

async function removeAccount(account: ConnectionAccount): Promise<void> {
  if (!props.connection) return;
  try {
    await api.deleteConnectionAccount(props.connection.id, account.id);
    await loadAccounts();
    emit("saved");
    toast.success(`Key “${account.label}” removed`);
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to delete the key",
    );
  }
}

/** A preset label turned into a usable connection name. */
function presetName(preset: ProviderPreset | null): string | null {
  if (!preset) return null;
  return preset.label
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

function parseTimeout(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

function addHeader(): void {
  headers.value.push({ key: "", value: "" });
}

function removeHeader(index: number): void {
  headers.value.splice(index, 1);
}

async function save(): Promise<void> {
  if (!name.value.trim() || !baseUrl.value.trim()) {
    toast.error("Name and base URL are required");
    return;
  }

  const customHeaders: Record<string, string> = {};
  for (const row of headers.value) {
    const key = row.key.trim();
    if (key) customHeaders[key] = row.value.trim();
  }

  if (
    connectTimeout.value.trim() &&
    parseTimeout(connectTimeout.value) === null
  ) {
    toast.error("Connect timeout must be zero or more milliseconds");
    return;
  }
  if (idleTimeout.value.trim() && parseTimeout(idleTimeout.value) === null) {
    toast.error("Idle timeout must be zero or more milliseconds");
    return;
  }

  const body = {
    name: name.value.trim(),
    provider_type: providerType.value,
    base_url: baseUrl.value.trim(),
    custom_headers: customHeaders,
    enabled: enabled.value,
    connect_timeout_ms: parseTimeout(connectTimeout.value),
    idle_timeout_ms: parseTimeout(idleTimeout.value),
    pricing_model: pricingModel.value.trim() || null,
    cache_retention: cacheRetention.value,
  };

  saving.value = true;
  try {
    if (props.connection) {
      const patch: Partial<ConnectionInput> = { ...body };
      if (apiKey.value.trim()) patch.api_key = apiKey.value.trim();
      else if (clearApiKey.value) patch.api_key = null;
      await api.updateConnection(props.connection.id, patch);
      toast.success(`Connection “${body.name}” updated`);
    } else {
      const createBody: ConnectionInput = {
        ...body,
        ...(props.preset ? { provider_id: props.preset.id } : {}),
        ...(apiKey.value.trim() ? { api_key: apiKey.value.trim() } : {}),
      };
      await api.createConnection(createBody);
      toast.success(`Connection “${body.name}” created`);
    }
    emit("saved");
    emit("update:open", false);
  } catch (error) {
    toast.error(
      error instanceof ApiError ? error.message : "Failed to save connection",
    );
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent class="sm:max-w-2xl">
      <DialogHeader>
        <DialogTitle>{{
          isEdit ? "Edit connection" : "New connection"
        }}</DialogTitle>
        <DialogDescription>
          An upstream endpoint. Credentials are encrypted at rest (AES-256-GCM)
          with
          <code>secrets.key</code>.
        </DialogDescription>
      </DialogHeader>

      <div
        v-if="!isEdit && preset"
        class="flex flex-wrap items-start justify-between gap-3 rounded-md border bg-muted/40 p-3 text-xs"
      >
        <div class="min-w-0">
          <p class="flex items-center gap-2 font-medium">
            <ProviderIcon :id="preset.id" :label="preset.label" />
            {{ preset.label }}
            <Badge variant="outline">{{ preset.provider_type }}</Badge>
          </p>
          <p v-if="preset.note" class="mt-1 text-muted-foreground">
            {{ preset.note }}
          </p>
          <a
            v-if="preset.api_key_url"
            :href="preset.api_key_url"
            target="_blank"
            rel="noreferrer"
            class="mt-1 inline-block underline underline-offset-4"
          >
            Get an API key
          </a>
        </div>
        <Button
          variant="outline"
          size="sm"
          type="button"
          @click="emit('change-provider')"
        >
          <RefreshCw /> Change provider
        </Button>
      </div>

      <div class="grid gap-4">
        <div class="grid gap-2">
          <Label for="connection-name">Name</Label>
          <Input
            id="connection-name"
            v-model="name"
            placeholder="openai-main"
          />
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label>Provider type</Label>
            <Select v-model="providerType">
              <SelectTrigger class="w-full">
                <SelectValue placeholder="Select provider" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="type in PROVIDER_TYPES"
                  :key="type"
                  :value="type"
                >
                  {{ type }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div
            class="flex items-end justify-between gap-4 rounded-md border p-3"
          >
            <div>
              <Label for="connection-enabled">Enabled</Label>
              <p class="text-xs text-muted-foreground">
                Disabled connections are skipped.
              </p>
            </div>
            <Switch id="connection-enabled" v-model="enabled" />
          </div>
        </div>

        <div class="grid gap-2">
          <Label for="connection-base-url">Base URL</Label>
          <Input
            id="connection-base-url"
            v-model="baseUrl"
            placeholder="https://api.openai.com/v1"
          />
          <p class="text-xs text-muted-foreground">
            Must include the API version path: the router appends
            <code>/chat/completions</code>, so this usually ends with
            <code>/v1</code>.
          </p>
        </div>

        <div class="grid gap-2">
          <Label for="connection-api-key">
            API key{{ preset?.auth === "none" ? " (optional)" : "" }}
          </Label>
          <Input
            id="connection-api-key"
            v-model="apiKey"
            type="password"
            autocomplete="off"
            :placeholder="
              isEdit && hasExistingKey && !clearApiKey
                ? 'Leave blank to keep the configured key'
                : 'sk-…'
            "
          />
          <div
            v-if="isEdit && hasExistingKey"
            class="text-xs text-muted-foreground"
          >
            <template v-if="clearApiKey">
              The stored key will be removed on save.
              <button
                class="underline"
                type="button"
                @click="clearApiKey = false"
              >
                Undo
              </button>
            </template>
            <template v-else>
              A key is configured.
              <button
                class="underline"
                type="button"
                @click="clearApiKey = true"
              >
                Clear it
              </button>
            </template>
          </div>
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="connection-connect-timeout">Connect timeout (ms)</Label>
            <Input
              id="connection-connect-timeout"
              v-model="connectTimeout"
              type="number"
              min="0"
              placeholder="Inherit server default"
            />
            <p class="text-xs text-muted-foreground">0 disables the timeout.</p>
          </div>
          <div class="grid gap-2">
            <Label for="connection-idle-timeout">Idle timeout (ms)</Label>
            <Input
              id="connection-idle-timeout"
              v-model="idleTimeout"
              type="number"
              min="0"
              placeholder="Inherit server default"
            />
            <p class="text-xs text-muted-foreground">
              Max silence between stream chunks.
            </p>
          </div>
        </div>

        <div class="grid gap-2">
          <Label for="connection-pricing-model">Pricing model (optional)</Label>
          <Input
            id="connection-pricing-model"
            v-model="pricingModel"
            placeholder="gpt-5.6-luna"
            autocapitalize="off"
          />
          <p class="text-xs text-muted-foreground">
            Catalog id used to price this connection's requests. Set it when the
            upstream model id differs from the catalog (e.g. relay paths like
            <code>ocg/openai/gpt-5.6-luna</code>).
          </p>
        </div>

        <div class="grid gap-2">
          <Label>Prompt caching</Label>
          <Select v-model="cacheRetention">
            <SelectTrigger id="connection-cache-retention" class="w-full">
              <SelectValue placeholder="Off" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">Off</SelectItem>
              <SelectItem value="short">5 minutes (short)</SelectItem>
              <SelectItem value="long">1 hour (long)</SelectItem>
            </SelectContent>
          </Select>
          <p class="text-xs text-muted-foreground">
            Marks stable prompt prefixes (system prompt, tools, history) with
            Anthropic <code>cache_control</code> breakpoints. Leave off for
            OpenAI-compatible endpoints that reject the extra field. The 1-hour
            option sends the extended-cache-ttl beta header.
          </p>
        </div>

        <div class="grid gap-2">
          <div class="flex items-center justify-between">
            <Label>Custom headers</Label>
            <Button
              variant="outline"
              size="sm"
              type="button"
              @click="addHeader"
            >
              <Plus /> Add header
            </Button>
          </div>
          <p v-if="!headers.length" class="text-xs text-muted-foreground">
            No custom headers. Values are sent verbatim to this upstream.
          </p>
          <div
            v-for="(header, index) in headers"
            :key="index"
            class="flex items-center gap-2"
          >
            <Input
              v-model="header.key"
              placeholder="Header-Name"
              class="flex-1"
            />
            <Input v-model="header.value" placeholder="value" class="flex-1" />
            <Button
              variant="ghost"
              size="icon"
              type="button"
              aria-label="Remove header"
              @click="removeHeader(index)"
            >
              <X />
            </Button>
          </div>
        </div>

        <div v-if="isEdit" class="grid gap-3 rounded-md border p-3">
          <div class="space-y-1">
            <Label>Extra API keys</Label>
            <p class="text-xs text-muted-foreground">
              The primary key and these enabled keys rotate round-robin per
              request; a failing key falls through to the next before the tier
              is abandoned.
            </p>
          </div>

          <p v-if="accountsLoading" class="text-xs text-muted-foreground">
            Loading keys…
          </p>

          <div v-else-if="accounts.length" class="grid gap-2">
            <div
              v-for="account in accounts"
              :key="account.id"
              class="flex items-center justify-between gap-3 rounded border px-3 py-2"
            >
              <div class="min-w-0">
                <p class="truncate text-sm font-medium">{{ account.label }}</p>
                <p class="text-xs text-muted-foreground">
                  Added {{ formatDateTime(account.created_at) }}
                </p>
              </div>
              <div class="flex shrink-0 items-center gap-2">
                <Switch
                  :model-value="account.enabled !== 0"
                  :aria-label="`Toggle ${account.label}`"
                  @update:model-value="(value) => toggleAccount(account, value)"
                />
                <Button
                  variant="ghost"
                  size="icon"
                  type="button"
                  class="text-destructive hover:text-destructive"
                  :aria-label="`Delete ${account.label}`"
                  @click="removeAccount(account)"
                >
                  <Trash2 />
                </Button>
              </div>
            </div>
          </div>

          <p v-else class="text-xs text-muted-foreground">
            No extra keys yet — the primary key above is used alone.
          </p>

          <div class="grid gap-2 sm:grid-cols-[1fr_1fr_auto]">
            <Input
              v-model="newAccountLabel"
              placeholder="Label (e.g. backup)"
            />
            <Input
              v-model="newAccountKey"
              type="password"
              autocomplete="off"
              placeholder="sk-…"
            />
            <Button
              type="button"
              variant="outline"
              :disabled="addingAccount"
              @click="addAccount"
            >
              <Plus /> Add
            </Button>
          </div>
        </div>

        <p v-else class="text-xs text-muted-foreground">
          Extra API keys can be added once the connection is saved.
        </p>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)"
          >Cancel</Button
        >
        <Button :disabled="saving" @click="save">
          {{
            saving ? "Saving…" : isEdit ? "Save changes" : "Create connection"
          }}
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
