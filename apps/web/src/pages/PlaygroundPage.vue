<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import PageHeader from "@/components/PageHeader.vue";
import ChatTab from "@/components/playground/ChatTab.vue";
import TokenSaverTab from "@/components/playground/TokenSaverTab.vue";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/api";
import { buildToggles, toggleSaver } from "@/lib/playground";
import type { SettingsResponse, TokenSaverSettings } from "@/types/api";

/**
 * The playground hub: one page for exercising the router end to end. The tab is
 * the only thing that changes; the per-run saver overrides below are shared, so
 * comparing configurations feels the same in either mode and a chat message
 * runs through whatever the token saver tab is currently showing.
 */
const tab = ref("chat");
const settings = ref<SettingsResponse | null>(null);
const overrides = ref<Partial<TokenSaverSettings>>({});

const live = computed<TokenSaverSettings | null>(
  () => settings.value?.token_saver ?? null,
);

/** The settings a run will use: live values with any per-run override applied. */
const effective = computed<Partial<TokenSaverSettings> | null>(() => {
  if (live.value) return { ...live.value, ...overrides.value };
  return Object.keys(overrides.value).length ? overrides.value : null;
});

const toggles = computed(() => buildToggles(effective.value));

const description = computed(() =>
  tab.value === "chat"
    ? "Streams real completions through the resolver, executor and token-saving pipeline — the same code every client request goes through. Nothing here is simulated."
    : "Runs the real pipeline and shows exactly what it changed to the prompt. Nothing here is simulated.",
);

/** Switches a per-run saver on or off without touching saved settings. */
function toggle(key: string): void {
  overrides.value = toggleSaver(overrides.value, effective.value, key);
}

function resetOverrides(): void {
  overrides.value = {};
}

onMounted(async () => {
  try {
    settings.value = await api.settings();
  } catch {
    // Both tabs still run against the live configuration without this: the
    // settings load only powers the toggles' initial state.
    settings.value = null;
  }
});
</script>

<template>
  <div class="flex flex-col gap-8">
    <PageHeader title="Playground" :description="description" />

    <Tabs v-model="tab">
      <TabsList>
        <TabsTrigger value="chat">Chat</TabsTrigger>
        <TabsTrigger value="token-saver">Token Saver</TabsTrigger>
      </TabsList>

      <TabsContent value="chat" class="pt-4">
        <ChatTab
          :toggles="toggles"
          :overrides="overrides"
          @toggle="toggle"
          @reset="resetOverrides"
        />
      </TabsContent>

      <TabsContent value="token-saver" class="pt-4">
        <TokenSaverTab
          :toggles="toggles"
          :overrides="overrides"
          @toggle="toggle"
          @reset="resetOverrides"
        />
      </TabsContent>
    </Tabs>
  </div>
</template>
