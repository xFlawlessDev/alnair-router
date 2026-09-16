<script setup lang="ts">
import { computed } from "vue";
import {
  Handle,
  Position,
  VueFlow,
  type Edge,
  type Node,
} from "@vue-flow/core";
import { Background } from "@vue-flow/background";
import { Controls } from "@vue-flow/controls";

import "@vue-flow/core/dist/style.css";
import "@vue-flow/core/dist/theme-default.css";
import "@vue-flow/controls/dist/style.css";

import Logo from "@/components/Logo.vue";
import { formatLatency, formatNumber } from "@/lib/format";
import { buildTopology, type TopologyNodeData } from "@/lib/topology";
import type { ConnectionActivity } from "@/types/api";

const props = defineProps<{ connections: ConnectionActivity[] }>();

const topology = computed(() => buildTopology(props.connections));
const nodes = computed(
  () => topology.value.nodes as unknown as Node<TopologyNodeData | null>[],
);
const edges = computed(() => topology.value.edges as unknown as Edge[]);

function averageLatency(connection: ConnectionActivity): number {
  return connection.requests
    ? connection.total_latency_ms / connection.requests
    : 0;
}
</script>

<template>
  <div class="h-72 w-full">
    <VueFlow
      :nodes="nodes"
      :edges="edges"
      :fit-view-on-init="true"
      :nodes-draggable="false"
      :nodes-connectable="false"
      :elements-selectable="false"
      :zoom-on-scroll="false"
      :prevent-scrolling="false"
      :min-zoom="0.4"
      :max-zoom="1.5"
    >
      <Background variant="dots" :gap="18" :size="1" color="var(--border)" />
      <Controls position="bottom-right" :show-interactive="false" />

      <template #node-router>
        <div
          class="relative flex size-24 flex-col items-center justify-center gap-1 rounded-full border-2 bg-background shadow-sm"
        >
          <Logo class="size-6" />
          <span class="text-[10px] font-medium">Router</span>
          <Handle type="target" :position="Position.Left" id="left" />
          <Handle type="target" :position="Position.Right" id="right" />
        </div>
      </template>

      <template #node-connection="{ data }">
        <div
          class="w-44 rounded-md border bg-background px-3 py-2 text-xs shadow-sm transition-colors"
          :class="data.connection.in_flight ? 'border-emerald-500/70' : ''"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="truncate font-medium">{{ data.connection.name }}</span>
            <span class="relative flex size-2 shrink-0">
              <span
                v-if="data.connection.in_flight"
                class="absolute inline-flex size-full animate-ping rounded-full bg-emerald-500 opacity-75"
              />
              <span
                class="relative inline-flex size-2 rounded-full"
                :class="
                  data.connection.in_flight
                    ? 'bg-emerald-500'
                    : 'bg-muted-foreground/40'
                "
              />
            </span>
          </div>
          <p class="mt-1 text-muted-foreground">
            {{ formatNumber(data.connection.requests) }} req ·
            {{ formatNumber(data.connection.failures) }} failed
          </p>
          <p class="text-muted-foreground">
            avg {{ formatLatency(averageLatency(data.connection)) }}
          </p>
          <Handle
            v-if="data.side === 'left'"
            type="source"
            :position="Position.Right"
            id="source"
          />
          <Handle v-else type="source" :position="Position.Left" id="source" />
        </div>
      </template>
    </VueFlow>
  </div>
</template>

<style scoped>
:deep(.vue-flow__node) {
  background: transparent;
  border: none;
  padding: 0;
  box-shadow: none;
  font-size: inherit;
}

:deep(.vue-flow__handle) {
  opacity: 0;
  width: 2px;
  height: 2px;
  min-width: 2px;
  min-height: 2px;
}

:deep(.vue-flow__controls) {
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  box-shadow: none;
}

:deep(.vue-flow__controls-button) {
  background: var(--background);
  border-bottom: 1px solid var(--border);
  fill: var(--foreground);
}

:deep(.vue-flow__controls-button:hover) {
  background: var(--accent);
}
</style>
