<script setup lang="ts">
import { computed } from 'vue';

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

const props = defineProps<{
  title?: string;
  data: { name?: string; color?: string; value: unknown }[];
}>();

/**
 * The chart datum also carries the raw `x`/`values` fields; only rows the legend
 * named (models and Total) belong in the tooltip, and empty buckets drop their
 * zero rows so the hover stays short.
 */
function isZero(value: unknown): boolean {
  if (typeof value === 'number') return value === 0;
  if (typeof value !== 'string') return false;
  return Number(value.replace(/[^0-9.-]/g, '')) === 0;
}

const rows = computed(() => props.data.filter((row) => row.name && !isZero(row.value)));
</script>

<template>
  <Card class="max-w-[18rem] text-sm">
    <CardHeader v-if="title" class="border-b p-3">
      <CardTitle>{{ title }}</CardTitle>
    </CardHeader>
    <CardContent class="flex min-w-[180px] flex-col gap-1 p-3">
      <div v-for="(item, key) in rows" :key="key" class="flex justify-between gap-3">
        <div class="flex min-w-0 items-center">
          <span class="mr-2 h-2.5 w-2.5 shrink-0">
            <svg width="100%" height="100%" viewBox="0 0 30 30">
              <path
                d=" M 15 15 m -14, 0 a 14,14 0 1,1 28,0 a 14,14 0 1,1 -28,0"
                :stroke="item.color"
                :fill="item.color"
                stroke-width="1"
              />
            </svg>
          </span>
          <span class="truncate" :title="item.name">{{ item.name }}</span>
        </div>
        <span class="shrink-0 font-semibold">{{ item.value }}</span>
      </div>
    </CardContent>
  </Card>
</template>
