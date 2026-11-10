<script setup lang="ts">
import { connectionCheckHint } from "@/lib/connectionProbe";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatDateTime, formatNumber } from "@/lib/format";
import type { Connection, ConnectionActivity } from "@/types/api";
import type { ProbeResult } from "./types";

defineProps<{
  connections: Connection[];
  activities: Record<string, ConnectionActivity>;
  probes: Record<string, ProbeResult>;
  testing: string[];
  activityAvailable: boolean;
}>();
const emit = defineEmits<{ test: [connection: Connection] }>();
</script>

<template>
  <Table>
    <TableHeader>
      <TableRow>
        <TableHead>Connection</TableHead>
        <TableHead>Last check</TableHead>
        <TableHead>Attempts / failures</TableHead>
        <TableHead>Avg. attempt time</TableHead>
        <TableHead><span class="sr-only">Actions</span></TableHead>
      </TableRow>
    </TableHeader>
    <TableBody>
      <TableRow v-for="connection in connections" :key="connection.id">
        <TableCell>
          <div class="font-semibold">{{ connection.name }}</div>
          <div class="text-xs text-muted-foreground">
            {{ connection.provider_type }}
          </div>
        </TableCell>
        <TableCell>
          <Badge v-if="!connection.enabled" variant="outline">Disabled</Badge>
          <template v-else-if="probes[connection.id]">
            <Badge
              :variant="
                probes[connection.id]?.result?.ok ? 'default' : 'destructive'
              "
            >
              {{
                probes[connection.id]?.result?.ok ? "Reachable" : "Check failed"
              }}
            </Badge>
            <p
              class="mt-1 max-w-xs whitespace-normal text-xs text-muted-foreground"
            >
              {{
                probes[connection.id]?.error ??
                probes[connection.id]?.result?.message
              }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ formatDateTime(probes[connection.id]?.checkedAt) }}
            </p>
            <p
              v-if="probes[connection.id]?.result"
              class="mt-1 text-xs text-muted-foreground"
            >
              Probe: {{ probes[connection.id]?.result?.latency_ms }} ms
            </p>
            <p
              v-if="probes[connection.id]?.result?.notice"
              class="mt-1 max-w-xs whitespace-normal text-xs text-muted-foreground"
            >
              {{ probes[connection.id]?.result?.notice }}
            </p>
          </template>
          <span v-else class="text-muted-foreground">Not checked</span>
        </TableCell>
        <TableCell>
          <template v-if="activities[connection.id]">
            {{ formatNumber(activities[connection.id]!.requests) }} /
            {{ formatNumber(activities[connection.id]!.failures) }}
            <div class="text-xs text-muted-foreground">
              {{ formatNumber(activities[connection.id]!.in_flight) }} awaiting
              response
            </div>
          </template>
          <span v-else class="text-muted-foreground">{{
            activityAvailable ? "No activity" : "Unavailable"
          }}</span>
        </TableCell>
        <TableCell>
          <template v-if="activities[connection.id]?.requests">
            {{
              Math.round(
                activities[connection.id]!.total_latency_ms /
                  activities[connection.id]!.requests,
              )
            }}
            ms
          </template>
          <span v-else class="text-muted-foreground">—</span>
        </TableCell>
        <TableCell>
          <Button
            variant="outline"
            size="sm"
            :disabled="!connection.enabled || testing.includes(connection.id)"
            :aria-label="`Check ${connection.name}`"
            :title="connectionCheckHint(connection)"
            @click="emit('test', connection)"
          >
            {{ testing.includes(connection.id) ? "Checking…" : "Check" }}
          </Button>
        </TableCell>
      </TableRow>
    </TableBody>
  </Table>
</template>
