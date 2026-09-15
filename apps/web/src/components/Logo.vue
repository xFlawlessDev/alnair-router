<script setup lang="ts">
import raw from '@assets/alnair-white.svg?raw';

/**
 * Alnair brand mark, rendered from the shared SVG asset. The artwork ships
 * white; it is repainted with `currentColor` here so it follows the theme:
 * near-black on the light theme, near-white on the dark one.
 *
 * Decorative by default (the surrounding UI carries the name); pass `title`
 * to expose it as an image with an accessible label instead.
 */
withDefaults(defineProps<{ title?: string }>(), { title: '' });

const artwork = raw.replace('#fff', 'currentColor');
</script>

<template>
  <span
    class="inline-flex shrink-0 [&>svg]:size-full"
    :role="title ? 'img' : undefined"
    :aria-label="title || undefined"
    :aria-hidden="title ? undefined : true"
    v-html="artwork"
  />
</template>
