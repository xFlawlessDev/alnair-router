<script setup lang="ts">
import { computed } from 'vue';

import { providerIcon, providerInitials, providerTypeIcon } from '@/lib/providerIcons';
import type { ProviderType } from '@/types/api';

const props = defineProps<{
  /** Provider preset id. Connections added by hand have none. */
  id?: string | null;
  /** Wire family, used when there is no preset glyph to show. */
  type?: ProviderType | null;
  label: string;
  /** Accessible name. Omit to keep the icon decorative. */
  title?: string;
}>();

const glyph = computed(() => providerIcon(props.id) ?? providerTypeIcon(props.type));
const initials = computed(() => providerInitials(props.label));
</script>

<template>
  <span
    v-if="glyph"
    class="inline-flex size-4 shrink-0 items-center justify-center [&>svg]:size-full"
    :role="title ? 'img' : undefined"
    :aria-label="title || undefined"
    :aria-hidden="title ? undefined : true"
    v-html="glyph"
  />
  <span
    v-else
    class="inline-flex size-4 shrink-0 items-center justify-center rounded-sm bg-muted text-[0.5rem] font-semibold text-muted-foreground uppercase"
    :role="title ? 'img' : undefined"
    :aria-label="title || undefined"
    :aria-hidden="title ? undefined : true"
  >
    {{ initials }}
  </span>
</template>
