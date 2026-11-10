<script setup lang="ts">
import PageHeader from "@/components/PageHeader.vue";
import ConnectTools from "@/components/guide/ConnectTools.vue";
import { Card, CardContent } from "@/components/ui/card";
</script>

<template>
  <div class="editorial-page max-w-4xl">
    <PageHeader
      title="CLI Tools"
      description="Connect your coding tools to Alnair Router."
    />
    <Card class="editorial-panel">
      <CardContent class="pt-6"><ConnectTools /></CardContent>
    </Card>
  </div>
</template>
