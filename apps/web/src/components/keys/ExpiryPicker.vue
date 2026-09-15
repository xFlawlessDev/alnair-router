<script setup lang="ts">
import type { DateValue } from '@internationalized/date';
import { getLocalTimeZone, parseDate, today } from '@internationalized/date';
import { computed, ref } from 'vue';

import { Button } from '@/components/ui/button';
import { Calendar } from '@/components/ui/calendar';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { formatDateTime } from '@/lib/format';

const props = defineProps<{ id: string; modelValue: string }>();
const emit = defineEmits<{ 'update:modelValue': [value: string] }>();

const open = ref(false);

const parsed = computed(() => {
  const value = props.modelValue.trim();
  if (!value) return { date: null as DateValue | null, time: '' };
  const [datePart = '', timePart = ''] = value.split('T');
  try {
    return { date: parseDate(datePart), time: timePart.slice(0, 5) };
  } catch {
    return { date: null, time: '' };
  }
});

const label = computed(() => {
  if (!parsed.value.date) return 'Never expires';
  const time = parsed.value.time || '00:00';
  return formatDateTime(`${parsed.value.date.toString()}T${time}`);
});

function selectDate(date: DateValue | undefined): void {
  if (!date) return;
  emit('update:modelValue', `${date.toString()}T${parsed.value.time || '00:00'}`);
}

function updateTime(value: string | number): void {
  const time = String(value);
  const date = parsed.value.date ?? today(getLocalTimeZone());
  emit('update:modelValue', time ? `${date.toString()}T${time}` : '');
}

function clear(): void {
  emit('update:modelValue', '');
  open.value = false;
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <Button
        :id="id"
        type="button"
        variant="outline"
        class="w-full justify-start font-normal"
        :class="modelValue ? '' : 'text-muted-foreground'"
      >
        {{ label }}
      </Button>
    </PopoverTrigger>
    <PopoverContent class="w-auto p-0" align="start">
      <Calendar
        :model-value="parsed.date"
        @update:model-value="selectDate"
      />
      <div class="flex items-center gap-2 border-t p-3">
        <Input
          type="time"
          class="h-8 w-32"
          :model-value="parsed.time || '00:00'"
          :aria-label="`Expiry time (${id})`"
          @update:model-value="updateTime"
        />
        <Button type="button" variant="ghost" size="sm" @click="clear">Clear</Button>
      </div>
    </PopoverContent>
  </Popover>
</template>
