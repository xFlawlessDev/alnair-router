<script setup lang="ts">
import { CheckCircle2, Copy, ExternalLink, Loader2, XCircle } from "@lucide/vue";
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { toast } from "vue-sonner";

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
import { ApiError, api } from "@/lib/api";
import type { Connection, OAuthAccount, OAuthPreset } from "@/types/api";

const props = defineProps<{
  open: boolean;
  connection: Connection;
}>();
const emit = defineEmits<{
  "update:open": [boolean];
  /** An account was connected or changed; the page should refresh. */
  saved: [];
}>();

/** When the dialog is watching a browser login, poll this often. */
const POLL_INTERVAL_MS = 1500;

const presets = ref<OAuthPreset[]>([]);
const accounts = ref<OAuthAccount[]>([]);
const loading = ref(true);
const saving = ref(false);

const presetId = ref("gitlab-duo");
const label = ref("");
const clientId = ref("");
const clientSecret = ref("");
const authorizeUrl = ref("");
const tokenUrl = ref("");
const deviceCodeUrl = ref("");
const userInfoUrl = ref("");
const scopes = ref("");

/** The login being watched, if any. */
const loginId = ref<string | null>(null);
const loginUrl = ref<string | null>(null);
const redirectUri = ref<string | null>(null);
const deviceCode = ref<{ user_code: string; verification_uri: string | null } | null>(
  null,
);
const loginError = ref<string | null>(null);
let pollTimer: ReturnType<typeof setInterval> | null = null;

const selectedPreset = computed(
  () => presets.value.find((preset) => preset.id === presetId.value) ?? null,
);
const isGeneric = computed(() => presetId.value === "generic");
const hasDevice = computed(() => Boolean(deviceCodeUrl.value.trim()));
const busy = computed(() => loginId.value !== null);

watch(
  () => props.open,
  (open) => {
    if (!open) {
      stopPolling();
      return;
    }
    resetForm();
    void load();
  },
);

onBeforeUnmount(stopPolling);

function resetForm(): void {
  accounts.value = [];
  loginId.value = null;
  loginUrl.value = null;
  redirectUri.value = null;
  deviceCode.value = null;
  loginError.value = null;
  label.value = "";
  clientId.value = "";
  clientSecret.value = "";
  scopes.value = "";
  stopPolling();
}

async function load(): Promise<void> {
  loading.value = true;
  try {
    const [presetList, accountList] = await Promise.all([
      api.listOAuthPresets(),
      api.listOAuthAccounts(props.connection.id),
    ]);
    presets.value = presetList.data ?? [];
    accounts.value = Array.isArray(accountList) ? accountList : [];
    applyPreset(presetId.value);
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to load OAuth data",
    );
  } finally {
    loading.value = false;
  }
}

/** Fills the endpoint fields from a preset; the client id stays empty. */
function applyPreset(id: string): void {
  presetId.value = id;
  const preset = presets.value.find((entry) => entry.id === id);
  authorizeUrl.value = preset?.authorize_url ?? "";
  tokenUrl.value = preset?.token_url ?? "";
  deviceCodeUrl.value = preset?.device_code_url ?? "";
  userInfoUrl.value = preset?.user_info_url ?? "";
  scopes.value = preset?.scopes ?? "";
}

function stopPolling(): void {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function clientPayload() {
  return {
    connection_id: props.connection.id,
    label: label.value.trim(),
    provider_key: presetId.value,
    client_id: clientId.value.trim(),
    client_secret: clientSecret.value.trim() || null,
    authorize_url: authorizeUrl.value.trim(),
    token_url: tokenUrl.value.trim(),
    device_code_url: deviceCodeUrl.value.trim() || null,
    user_info_url: userInfoUrl.value.trim() || null,
    scopes: scopes.value.trim(),
  };
}

function validate(): boolean {
  if (!label.value.trim()) {
    toast.error("An account label is required");
    return false;
  }
  if (!clientId.value.trim()) {
    toast.error(
      "A client id is required — register an OAuth application with the provider first",
    );
    return false;
  }
  if (!tokenUrl.value.trim()) {
    toast.error("A token URL is required");
    return false;
  }
  return true;
}

/** Starts a PKCE login and opens the provider's consent page. */
async function connectInBrowser(): Promise<void> {
  if (!validate()) return;
  if (!authorizeUrl.value.trim()) {
    toast.error("An authorize URL is required for a browser login");
    return;
  }

  saving.value = true;
  try {
    const started = await api.startOAuthLogin(clientPayload());
    loginId.value = started.login_id;
    loginUrl.value = started.authorize_url;
    redirectUri.value = started.redirect_uri;
    loginError.value = null;
    window.open(started.authorize_url, "_blank", "noopener,noreferrer");
    startPolling();
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to start the login",
    );
  } finally {
    saving.value = false;
  }
}

/** Starts a device login; the router polls the provider on its own. */
async function connectWithDeviceCode(): Promise<void> {
  if (!validate()) return;

  saving.value = true;
  try {
    const started = await api.startOAuthDeviceLogin(clientPayload());
    loginId.value = started.login_id;
    deviceCode.value = {
      user_code: started.user_code,
      verification_uri: started.verification_uri,
    };
    loginUrl.value = started.verification_uri;
    loginError.value = null;
    if (started.verification_uri) {
      window.open(started.verification_uri, "_blank", "noopener,noreferrer");
    }
    startPolling();
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to start the login",
    );
  } finally {
    saving.value = false;
  }
}

function startPolling(): void {
  stopPolling();
  pollTimer = setInterval(() => void pollOnce(), POLL_INTERVAL_MS);
}

async function pollOnce(): Promise<void> {
  const id = loginId.value;
  if (!id) return;

  try {
    const view = await api.oauthLoginStatus(id);
    if (view.status === "completed") {
      stopPolling();
      loginId.value = null;
      loginUrl.value = null;
      deviceCode.value = null;
      toast.success(`Account “${view.label}” connected`);
      await refreshAccounts();
      emit("saved");
    } else if (view.status === "failed") {
      stopPolling();
      loginId.value = null;
      loginError.value = view.error ?? "The provider refused the login";
    }
  } catch (caught) {
    // A dropped poll is not fatal; the login keeps going server-side.
    if (caught instanceof ApiError && caught.status === 404) {
      stopPolling();
      loginId.value = null;
      loginError.value = "This login expired. Start it again.";
    }
  }
}

async function refreshAccounts(): Promise<void> {
  try {
    const list = await api.listOAuthAccounts(props.connection.id);
    accounts.value = Array.isArray(list) ? list : [];
  } catch {
    // The list is cosmetic here; the connect already succeeded.
  }
}

async function cancelLogin(): Promise<void> {
  const id = loginId.value;
  stopPolling();
  loginId.value = null;
  loginUrl.value = null;
  deviceCode.value = null;
  if (id) {
    try {
      await api.cancelOAuthLogin(id);
    } catch {
      // Already gone is fine.
    }
  }
}

async function toggleAccount(
  account: OAuthAccount,
  enabled: boolean,
): Promise<void> {
  try {
    await api.updateOAuthAccount(props.connection.id, account.id, { enabled });
    await refreshAccounts();
    emit("saved");
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to update account",
    );
  }
}

async function removeAccount(account: OAuthAccount): Promise<void> {
  try {
    await api.deleteOAuthAccount(props.connection.id, account.id);
    await refreshAccounts();
    emit("saved");
    toast.success(`Account “${account.label}” disconnected`);
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to remove account",
    );
  }
}

async function copy(value: string, what: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value);
    toast.success(`${what} copied`);
  } catch {
    toast.error("Could not copy to the clipboard");
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="emit('update:open', $event)">
    <DialogScrollContent class="max-w-2xl">
      <DialogHeader>
        <DialogTitle>OAuth accounts</DialogTitle>
        <DialogDescription>
          Sign in with your own OAuth application —
          <span class="font-medium">{{ connection.name }}</span> sends the token
          as <code>Authorization: Bearer</code>. The router ships no client id of
          its own.
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-5">
        <div class="grid gap-2">
          <p class="text-sm font-medium">Connected accounts</p>

          <p v-if="loading" class="text-xs text-muted-foreground">
            Loading accounts…
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
                  {{ account.provider_key }} · updated
                  {{ new Date(account.updated_at).toLocaleString() }}
                </p>
              </div>
              <div class="flex shrink-0 items-center gap-2">
                <Badge :variant="account.enabled !== 0 ? 'default' : 'secondary'">
                  {{ account.enabled !== 0 ? "active" : "disabled" }}
                </Badge>
                <Button
                  variant="outline"
                  size="sm"
                  type="button"
                  @click="toggleAccount(account, account.enabled === 0)"
                >
                  {{ account.enabled !== 0 ? "Disable" : "Enable" }}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  class="text-destructive hover:text-destructive"
                  @click="removeAccount(account)"
                >
                  Remove
                </Button>
              </div>
            </div>
          </div>

          <p v-else class="text-xs text-muted-foreground">
            No OAuth accounts yet. Connect one below, or keep using the static
            API key on the connection.
          </p>
        </div>

        <div class="grid gap-3 rounded border p-3">
          <div class="flex items-center justify-between gap-3">
            <p class="text-sm font-medium">Connect an account</p>
            <Badge variant="outline">bring your own client</Badge>
          </div>

          <div class="grid gap-2">
            <Label for="oauth-preset">Provider preset</Label>
            <Select
              :model-value="presetId"
              @update:model-value="(value) => applyPreset(String(value))"
            >
              <SelectTrigger id="oauth-preset">
                <SelectValue placeholder="Choose a preset" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="preset in presets"
                  :key="preset.id"
                  :value="preset.id"
                >
                  {{ preset.label }}
                </SelectItem>
              </SelectContent>
            </Select>
            <p
              v-if="selectedPreset?.note"
              class="text-xs text-muted-foreground"
            >
              {{ selectedPreset.note }}
            </p>
            <a
              v-if="selectedPreset?.docs_url"
              :href="selectedPreset.docs_url"
              target="_blank"
              rel="noopener noreferrer"
              class="inline-flex w-fit items-center gap-1 text-xs text-primary underline-offset-4 hover:underline"
            >
              Register an OAuth application
              <ExternalLink class="size-3" />
            </a>
          </div>

          <div class="grid gap-2 sm:grid-cols-2">
            <div class="grid gap-2">
              <Label for="oauth-label">Account label</Label>
              <Input
                id="oauth-label"
                v-model="label"
                placeholder="work"
                :disabled="busy"
              />
            </div>
            <div class="grid gap-2">
              <Label for="oauth-client-id">Client ID</Label>
              <Input
                id="oauth-client-id"
                v-model="clientId"
                autocomplete="off"
                placeholder="your own client id"
                :disabled="busy"
              />
            </div>
          </div>

          <div class="grid gap-2">
            <Label for="oauth-client-secret">
              Client secret
              <span class="text-muted-foreground">
                (leave blank for a public PKCE client)
              </span>
            </Label>
            <Input
              id="oauth-client-secret"
              v-model="clientSecret"
              type="password"
              autocomplete="off"
              placeholder="optional"
              :disabled="busy"
            />
          </div>

          <div class="grid gap-2">
            <Label for="oauth-authorize">Authorize URL</Label>
            <Input
              id="oauth-authorize"
              v-model="authorizeUrl"
              :disabled="busy"
              :placeholder="isGeneric ? 'https://…/oauth/authorize' : ''"
            />
          </div>

          <div class="grid gap-2">
            <Label for="oauth-token">Token URL</Label>
            <Input
              id="oauth-token"
              v-model="tokenUrl"
              :disabled="busy"
              :placeholder="isGeneric ? 'https://…/oauth/token' : ''"
            />
          </div>

          <div class="grid gap-2 sm:grid-cols-2">
            <div class="grid gap-2">
              <Label for="oauth-device">
                Device code URL
                <span class="text-muted-foreground">(optional)</span>
              </Label>
              <Input
                id="oauth-device"
                v-model="deviceCodeUrl"
                :disabled="busy"
                placeholder="https://…/device/code"
              />
            </div>
            <div class="grid gap-2">
              <Label for="oauth-userinfo">
                User info URL
                <span class="text-muted-foreground">(optional)</span>
              </Label>
              <Input
                id="oauth-userinfo"
                v-model="userInfoUrl"
                :disabled="busy"
                placeholder="https://…/userinfo"
              />
            </div>
          </div>

          <div class="grid gap-2">
            <Label for="oauth-scopes">Scopes</Label>
            <Input
              id="oauth-scopes"
              v-model="scopes"
              :disabled="busy"
              placeholder="space separated"
            />
          </div>

          <div
            v-if="redirectUri"
            class="grid gap-1 rounded bg-muted/50 px-3 py-2"
          >
            <p class="text-xs text-muted-foreground">
              Register this exact redirect URI with your OAuth application:
            </p>
            <div class="flex items-center gap-2">
              <code class="truncate text-xs">{{ redirectUri }}</code>
              <Button
                variant="ghost"
                size="icon"
                type="button"
                :aria-label="'Copy redirect URI'"
                @click="copy(redirectUri ?? '', 'Redirect URI')"
              >
                <Copy class="size-3.5" />
              </Button>
            </div>
          </div>

          <div
            v-if="busy"
            class="flex items-start gap-2 rounded border border-primary/40 bg-primary/5 px-3 py-2"
          >
            <Loader2 class="mt-0.5 size-4 shrink-0 animate-spin" />
            <div class="min-w-0 text-xs">
              <p class="font-medium">Waiting for authorization…</p>
              <p v-if="deviceCode" class="mt-1 text-muted-foreground">
                Enter the code
                <span class="font-mono font-medium text-foreground">
                  {{ deviceCode.user_code }}
                </span>
                on the provider's page.
              </p>
              <p v-else class="mt-1 text-muted-foreground">
                Finish signing in on the tab that just opened.
              </p>
              <a
                v-if="loginUrl"
                :href="loginUrl"
                target="_blank"
                rel="noopener noreferrer"
                class="mt-1 inline-flex items-center gap-1 text-primary underline-offset-4 hover:underline"
              >
                Reopen the authorization page
                <ExternalLink class="size-3" />
              </a>
            </div>
          </div>

          <div
            v-if="loginError"
            class="flex items-start gap-2 rounded border border-destructive/40 bg-destructive/5 px-3 py-2"
          >
            <XCircle class="mt-0.5 size-4 shrink-0 text-destructive" />
            <p class="text-xs">{{ loginError }}</p>
          </div>

          <div v-if="!busy && !loginError && accounts.length" class="flex items-center gap-2 text-xs text-muted-foreground">
            <CheckCircle2 class="size-3.5 text-emerald-500" />
            Rotate between accounts by adding more than one.
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              :disabled="saving || busy || !authorizeUrl.trim()"
              @click="connectInBrowser"
            >
              <ExternalLink />
              {{ saving ? "Starting…" : "Connect in browser" }}
            </Button>
            <Button
              v-if="hasDevice"
              type="button"
              variant="outline"
              :disabled="saving || busy"
              @click="connectWithDeviceCode"
            >
              Use a device code
            </Button>
            <Button
              v-if="busy"
              type="button"
              variant="ghost"
              @click="cancelLogin"
            >
              Cancel
            </Button>
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">
          Close
        </Button>
      </DialogFooter>
    </DialogScrollContent>
  </Dialog>
</template>
