<script setup lang="ts">
import type { Connection } from "@/types/api";
import OAuthQuota from "./OAuthQuota.vue";
import TavilyQuota from "./TavilyQuota.vue";
defineProps<{ connection: Connection }>();
</script>

<template>
  <OAuthQuota
    v-if="connection.provider_type === 'openai-codex'"
    :connection-id="connection.id"
  />
  <TavilyQuota
    v-else-if="connection.provider_id === 'tavily'"
    :connection-id="connection.id"
  />
</template>
