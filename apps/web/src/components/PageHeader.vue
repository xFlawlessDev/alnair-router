<script setup lang="ts">
defineProps<{
  title: string;
  description?: string;
}>();
</script>

<template>
  <header class="editorial-masthead">
    <div class="masthead-copy">
      <span class="masthead-rubric" aria-hidden="true"
        >Alnair / Router desk</span
      >
      <h1>{{ title }}</h1>
      <p v-if="description">{{ description }}</p>
    </div>
    <div v-if="$slots.actions" class="masthead-actions">
      <slot name="actions" />
    </div>
  </header>
</template>

<style scoped>
.editorial-masthead {
  position: relative;
  isolation: isolate;
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: flex-end;
  justify-content: space-between;
  gap: 1.5rem;
  margin-bottom: 0.5rem;
  border-top: 6px solid #2448a6;
  padding: clamp(1.25rem, 3vw, 2rem);
  padding-bottom: 2rem;
  background: var(--editorial-navy);
  color: var(--editorial-paper);
}
.editorial-masthead::before {
  position: absolute;
  z-index: -1;
  top: 0;
  right: 0;
  width: min(35%, 14rem);
  height: 100%;
  background-image: radial-gradient(#fffaf0 0.7px, transparent 0.7px);
  background-size: 5px 5px;
  opacity: 0.08;
  content: "";
  pointer-events: none;
}
.editorial-masthead::after {
  position: absolute;
  right: 0;
  bottom: -10px;
  left: 0;
  height: 12px;
  background: var(--editorial-navy);
  clip-path: polygon(
    0 0,
    100% 0,
    100% 45%,
    97% 70%,
    93% 40%,
    89% 90%,
    84% 55%,
    80% 100%,
    74% 60%,
    69% 75%,
    64% 40%,
    59% 80%,
    54% 50%,
    49% 100%,
    44% 60%,
    39% 80%,
    34% 45%,
    29% 95%,
    24% 55%,
    19% 80%,
    14% 50%,
    9% 90%,
    4% 55%,
    0 80%
  );
  content: "";
  pointer-events: none;
}
.masthead-copy {
  min-width: 0;
  flex: 1 1 18rem;
}
.masthead-rubric {
  display: block;
  width: fit-content;
  margin-bottom: 1rem;
  border-bottom: 1px solid #8294a7;
  padding-bottom: 0.375rem;
  color: var(--editorial-mustard);
  font-size: 0.75rem;
  font-weight: 700;
}
h1 {
  overflow-wrap: anywhere;
  font-family: var(--editorial-display);
  font-size: clamp(2.25rem, 5vw, 4rem);
  font-style: italic;
  font-weight: 900;
  letter-spacing: -0.055em;
  line-height: 1.05;
}
p {
  max-width: 42rem;
  margin-top: 0.875rem;
  color: #d7e0e8;
  font-size: 0.875rem;
  line-height: 1.6;
}
.masthead-actions {
  display: flex;
  max-width: 100%;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  border: 3px solid var(--editorial-paper);
  padding: 0.5rem;
  background: var(--background);
  color: var(--foreground);
  box-shadow: 4px 4px 0 #071524;
}
</style>
