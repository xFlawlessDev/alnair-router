<script setup lang="ts">
import {
  Activity,
  Copy,
  Eye,
  EyeOff,
  Loader2,
  Pencil,
  Trash2,
} from "@lucide/vue";

import ProviderIcon from "@/components/ProviderIcon.vue";
import StatusBadge from "@/components/StatusBadge.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { formatDateTime, isEnabled, maskSecret } from "@/lib/format";
import type { Connection } from "@/types/api";

const props = defineProps<{
  connection: Connection;
  /** Preset label, empty for connections added by hand. */
  providerLabel: string;
  /** Custom header count, `0` when none. */
  headers: number;
  testing: boolean;
  revealed: boolean;
}>();

const emit = defineEmits<{
  test: [];
  edit: [];
  delete: [];
  toggle: [];
  reveal: [];
  copy: [];
}>();
</script>

<template>
  <Card class="gap-0">
    <CardContent class="flex flex-col gap-3 p-4">
      <div class="flex items-start gap-2">
        <ProviderIcon
          :id="connection.provider_id"
          :type="connection.provider_type"
          :label="providerLabel || connection.provider_type"
          class="mt-0.5 text-muted-foreground"
        />
        <span
          class="min-w-0 flex-1 truncate font-medium"
          :title="connection.name"
        >
          {{ connection.name }}
        </span>
        <StatusBadge :enabled="isEnabled(connection.enabled)" />
        <Switch
          :model-value="isEnabled(connection.enabled)"
          :aria-label="`Toggle ${connection.name}`"
          @update:model-value="emit('toggle')"
        />
      </div>

      <div class="flex flex-wrap items-center gap-1">
        <Badge v-if="connection.provider_id" variant="secondary">{{
          providerLabel
        }}</Badge>
        <Badge variant="outline">{{ connection.provider_type }}</Badge>
        <Badge
          v-if="connection.account_count"
          variant="secondary"
          :title="`${connection.account_count} extra key(s) rotate behind this connection`"
        >
          +{{ connection.account_count }} keys
        </Badge>
      </div>

      <code
        class="truncate text-xs text-muted-foreground"
        :title="connection.base_url"
      >
        {{ connection.base_url }}
      </code>

      <div
        class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground"
      >
        <span>{{
          headers ? `${headers} header(s)` : "No custom headers"
        }}</span>
        <span>Updated {{ formatDateTime(connection.updated_at) }}</span>
      </div>

      <div class="flex items-center gap-1">
        <template v-if="connection.api_key">
          <code class="truncate text-xs">
            {{ revealed ? connection.api_key : maskSecret(connection.api_key) }}
          </code>
          <Button
            variant="ghost"
            size="icon-sm"
            :aria-label="revealed ? 'Hide API key' : 'Reveal API key'"
            @click="emit('reveal')"
          >
            <EyeOff v-if="revealed" />
            <Eye v-else />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Copy API key"
            @click="emit('copy')"
          >
            <Copy />
          </Button>
        </template>
        <span v-else class="text-xs text-muted-foreground">No API key</span>
      </div>

      <div class="flex justify-end gap-1 border-t pt-3">
        <Button
          variant="ghost"
          size="icon-sm"
          :disabled="testing"
          :aria-label="`Test ${connection.name}`"
          :title="`Test ${connection.name} against its /models endpoint`"
          @click="emit('test')"
        >
          <Loader2 v-if="testing" class="animate-spin" />
          <Activity v-else />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          :aria-label="`Edit ${connection.name}`"
          @click="emit('edit')"
        >
          <Pencil />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          class="text-destructive hover:text-destructive"
          :aria-label="`Delete ${connection.name}`"
          @click="emit('delete')"
        >
          <Trash2 />
        </Button>
      </div>
    </CardContent>
  </Card>
</template>
