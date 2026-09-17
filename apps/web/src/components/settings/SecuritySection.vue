<script setup lang="ts">
import { ShieldCheck } from "@lucide/vue";

import SettingToggle from "@/components/settings/SettingToggle.vue";
import SettingsField from "@/components/settings/SettingsField.vue";
import SettingsSection from "@/components/settings/SettingsSection.vue";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { useSettingsForm } from "@/lib/settings/context";
import { SETTINGS_SECTIONS } from "@/lib/settings/sections";

const { form, loading, adminTokenLocked, adminTokenSet } = useSettingsForm();

const section = SETTINGS_SECTIONS.find((entry) => entry.id === "security")!;
</script>

<template>
  <SettingsSection :section="section">
    <template #title>
      <ShieldCheck class="size-4 text-muted-foreground" />
      {{ section.label }}
    </template>

    <div class="grid gap-3">
      <SettingToggle
        control-id="setting-require-api-key"
        title="Require client API key"
      >
        <template #description>
          Every <code class="text-xs">/v1</code> request must carry
          <code class="text-xs">Authorization: Bearer &lt;key&gt;</code>, using
          a key from the API Keys page.
        </template>
        <template #control>
          <Switch
            id="setting-require-api-key"
            v-model="form.require_api_key"
            :disabled="loading"
          />
        </template>
      </SettingToggle>

      <SettingToggle
        control-id="setting-admin-token-enabled"
        title="Require admin token"
      >
        <template #description>
          Guards <code class="text-xs">/api/*</code> with a bearer token.
          Enforced everywhere once set, even on loopback.
          <span v-if="adminTokenLocked" class="text-destructive">
            Cannot be disabled on a non-loopback bind without
            <code class="text-xs">allow_unauthenticated_admin</code>.
          </span>
        </template>
        <template #control>
          <Switch
            id="setting-admin-token-enabled"
            v-model="form.admin_token_enabled"
            :disabled="loading || adminTokenLocked"
          />
        </template>

        <SettingsField
          v-if="form.admin_token_enabled"
          id="setting-admin-token"
          label="Admin token"
          :hint="
            adminTokenSet
              ? 'Leave blank to keep the current token. A new one is stored in this browser too.'
              : 'No token stored yet — enter one to turn the requirement on. It is stored in this browser too.'
          "
        >
          <Input
            id="setting-admin-token"
            v-model="form.admin_token"
            type="password"
            autocomplete="off"
            :placeholder="
              adminTokenSet ? 'Keep current token' : 'Enter a token'
            "
            :disabled="loading"
          />
        </SettingsField>
      </SettingToggle>

      <SettingToggle
        control-id="setting-readiness-checks"
        title="Check upstreams on /api/ready"
        description="Adds best-effort TCP reachability of each enabled connection to the readiness probe."
      >
        <template #control>
          <Switch
            id="setting-readiness-checks"
            v-model="form.readiness_upstream_checks"
            :disabled="loading"
          />
        </template>
      </SettingToggle>

      <SettingToggle
        control-id="setting-public-usage"
        title="Self-service usage page"
      >
        <template #description>
          Exposes <code class="text-xs">/me</code> and
          <code class="text-xs">/api/public/usage</code>, where a client reads
          its own rollup with a router-issued API key. No key, no data.
        </template>
        <template #control>
          <Switch
            id="setting-public-usage"
            v-model="form.public_usage"
            :disabled="loading"
          />
        </template>
      </SettingToggle>

      <SettingToggle
        control-id="setting-store-key-secrets"
        title="Store key secrets"
      >
        <template #description>
          Keeps an encrypted copy of each minted key so the API Keys page can
          reveal it. Off means a key is readable only at creation time — turn it
          off if the router should not hold client keys in recoverable form.
          Existing keys keep whatever they were minted with.
        </template>
        <template #control>
          <Switch
            id="setting-store-key-secrets"
            v-model="form.store_key_secrets"
            :disabled="loading"
          />
        </template>
      </SettingToggle>

      <SettingToggle
        control-id="setting-lan-access"
        title="LAN access"
        description="Bind every interface so other devices on the network can reach the router; the listener re-binds immediately. Admin routes then require the dashboard password."
      >
        <template #control>
          <Switch
            id="setting-lan-access"
            v-model="form.lan_access"
            :disabled="loading"
          />
        </template>
      </SettingToggle>

      <SettingsField
        id="setting-cors"
        label="CORS origins"
        hint="Browser origins allowed to call the API cross-origin, one per line. Empty emits no CORS headers; * allows any origin."
      >
        <Textarea
          id="setting-cors"
          v-model="form.cors_origins"
          rows="3"
          class="font-mono text-xs"
          placeholder="https://app.example.com"
          :disabled="loading"
        />
      </SettingsField>
    </div>
  </SettingsSection>
</template>
