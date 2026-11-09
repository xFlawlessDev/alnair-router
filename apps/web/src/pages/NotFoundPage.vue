<script setup lang="ts">
import { RouterLink } from "vue-router";
import { Button } from "@/components/ui/button";
</script>

<template>
  <div class="editorial-page mx-auto max-w-2xl py-10 sm:py-20">
    <p class="editorial-sticker editorial-sticker--accent w-fit">404</p>
    <h1 class="text-4xl font-black italic tracking-tight sm:text-6xl">
      Page not found
    </h1>
    <p class="max-w-md text-muted-foreground">
      The route you requested does not exist in this dashboard.
    </p>
    <Button as-child class="w-fit">
      <RouterLink to="/">Back to overview</RouterLink>
    </Button>
  </div>
</template>
