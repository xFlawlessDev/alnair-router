<script setup lang="ts">
import { Moon, Sun } from '@lucide/vue';
import { computed, onMounted } from 'vue';

import { Button } from '@/components/ui/button';
import { useTheme } from '@/composables/useTheme';

const { isDark, toggle, initialize } = useTheme();
const label = computed(() => (isDark.value ? 'Switch to light mode' : 'Switch to dark mode'));

onMounted(initialize);
</script>

<template>
  <Button variant="ghost" size="icon" :aria-label="label" @click="toggle">
    <Sun v-if="isDark" />
    <Moon v-else />
  </Button>
</template>
