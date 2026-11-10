<script setup lang="ts">
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatNumber } from "@/lib/format";
import type { HealthDisplayRow } from "./types";

defineProps<{ rows: HealthDisplayRow[]; label: string; testing: string[] }>();
const emit = defineEmits<{ test: [id: string] }>();
</script>

<template>
  <Table>
    <TableHeader
      ><TableRow>
        <TableHead>{{ label }}</TableHead
        ><TableHead>Status</TableHead><TableHead>Uptime</TableHead>
        <TableHead>Attempts</TableHead><TableHead>Success</TableHead
        ><TableHead>p95</TableHead><TableHead>Main issue</TableHead
        ><TableHead><span class="sr-only">Actions</span></TableHead>
      </TableRow></TableHeader
    >
    <TableBody>
      <TableRow v-for="row in rows" :key="row.name">
        <TableCell class="font-semibold"
          >{{ row.name }}
          <p
            v-if="row.detail"
            class="text-xs font-normal text-muted-foreground"
          >
            {{ row.detail }}
          </p></TableCell
        >
        <TableCell
          ><Badge
            :variant="
              row.status === 'unhealthy'
                ? 'destructive'
                : row.status === 'healthy'
                  ? 'default'
                  : 'outline'
            "
            >{{ row.status }}</Badge
          ></TableCell
        >
        <TableCell class="text-muted-foreground">—</TableCell>
        <TableCell
          >{{ formatNumber(row.attempts) }}
          <p v-if="row.laterAttempts" class="text-xs text-muted-foreground">
            {{ formatNumber(row.laterAttempts) }} later attempts
          </p></TableCell
        >
        <TableCell>{{
          row.attempts
            ? `${(((row.attempts - row.failures) / row.attempts) * 100).toFixed(1)}%`
            : "—"
        }}</TableCell>
        <TableCell>{{ row.attempts ? `${row.p95} ms` : "—" }}</TableCell>
        <TableCell class="max-w-xs whitespace-normal">{{
          row.issue || "—"
        }}</TableCell>
        <TableCell
          ><Button
            v-if="row.connectionId"
            size="sm"
            variant="outline"
            :disabled="
              row.status === 'disabled' || testing.includes(row.connectionId)
            "
            :aria-label="`Check ${row.name}`"
            @click="emit('test', row.connectionId)"
            >{{
              testing.includes(row.connectionId) ? "Checking…" : "Check"
            }}</Button
          ></TableCell
        >
      </TableRow>
    </TableBody>
  </Table>
</template>
