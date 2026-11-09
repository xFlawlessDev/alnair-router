<script setup lang="ts">
import { Download, RefreshCw, Share2 } from "@lucide/vue";
import { computed, shallowRef, watch } from "vue";
import { toast } from "vue-sonner";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  buildUsageShareSvg,
  rankUsageModels,
  USAGE_CARD_DIMENSIONS,
  type UsageShareFormat,
  usageShareFile,
} from "@/lib/usageShareCard";
import type { MyUsageResponse } from "@/types/api";

const props = withDefaults(
  defineProps<{ usage: MyUsageResponse; disabled?: boolean }>(),
  { disabled: false },
);
const imageSource = shallowRef("");
const file = shallowRef<File | null>(null);
const preparing = shallowRef(true);
const sharing = shallowRef(false);
const error = shallowRef("");
const revision = shallowRef(0);
const format = shallowRef<UsageShareFormat>("landscape");
const dimensions = computed(() => USAGE_CARD_DIMENSIONS[format.value]);
const totalTokens = computed(
  () =>
    props.usage.summary.prompt_tokens + props.usage.summary.completion_tokens,
);
const topModels = computed(() =>
  rankUsageModels(props.usage.models ?? [], totalTokens.value).slice(0, 4),
);
const unavailable = computed(
  () => props.disabled || preparing.value || sharing.value || !file.value,
);

watch(
  [() => props.usage, revision, format],
  async ([usage, , selectedFormat], _, onCleanup) => {
    let current = true;
    onCleanup(() => {
      current = false;
    });
    preparing.value = true;
    error.value = "";
    file.value = null;
    const svg = buildUsageShareSvg(
      {
        name: usage.key.name,
        since: usage.since,
        until: usage.until,
        capturedAt: new Date(),
        summary: usage.summary,
        models: usage.models,
      },
      selectedFormat,
    );
    imageSource.value = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
    try {
      const prepared = await usageShareFile(svg, selectedFormat);
      if (current) file.value = prepared;
    } catch (caught) {
      if (current)
        error.value =
          caught instanceof Error
            ? caught.message
            : "Could not prepare the image";
    } finally {
      if (current) preparing.value = false;
    }
  },
  { immediate: true },
);

function downloadImage(): void {
  if (!file.value || unavailable.value) return;
  saveFile(file.value);
}

function saveFile(image: File): void {
  const url = URL.createObjectURL(image);
  const link = document.createElement("a");
  link.href = url;
  link.download = image.name;
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

async function shareImage(): Promise<void> {
  const image = file.value;
  if (!image || unavailable.value) return;
  const data: ShareData = { files: [image], title: "My Alnair API usage" };
  let canShare = false;
  try {
    canShare = !!navigator.share && !!navigator.canShare?.(data);
  } catch {
    canShare = false;
  }
  if (!canShare) {
    saveFile(image);
    toast.info("PNG downloaded. Attach it to your social post.");
    return;
  }
  sharing.value = true;
  try {
    await navigator.share(data);
  } catch (caught) {
    if (!(
      caught &&
      typeof caught === "object" &&
      "name" in caught &&
      caught.name === "AbortError"
    )) {
      saveFile(image);
      toast.info("Device sharing is unavailable. PNG downloaded instead.");
    }
  } finally {
    sharing.value = false;
  }
}
</script>

<template>
  <Card class="editorial-panel share-panel">
    <CardHeader>
      <CardTitle class="editorial-section-title"
        >Your usage, as a sticker</CardTitle
      >
      <CardDescription
        >A snapshot of this key's usage in the selected period, ready to
        share.</CardDescription
      >
    </CardHeader>
    <CardContent
      class="grid items-center gap-6 md:grid-cols-[minmax(0,1fr)_minmax(0,.7fr)]"
    >
      <figure class="min-w-0">
        <img
          :src="imageSource"
          :width="dimensions.width"
          :height="dimensions.height"
          class="share-preview"
          :class="{ 'share-preview--square': format === 'square' }"
          :alt="`Usage snapshot for ${usage.key.name}: ${usage.summary.requests} requests and ${usage.summary.prompt_tokens + usage.summary.completion_tokens} tokens`"
        />
      </figure>
      <div class="grid min-w-0 gap-4">
        <div>
          <h3 class="text-lg font-extrabold">Take your stats with you.</h3>
          <p class="mt-2 text-sm leading-relaxed text-muted-foreground">
            Includes the key name, period, tokens, requests, recorded spend,
            average tokens per request, and top models. The image contains no
            API key or router address.
          </p>
        </div>
        <div class="border-y border-border py-3">
          <p class="text-xs font-bold text-muted-foreground">
            Top models / by tokens
          </p>
          <ol v-if="topModels.length" class="mt-2 grid gap-2">
            <li
              v-for="model in topModels"
              :key="model.model"
              class="flex items-start justify-between gap-3 text-sm"
            >
              <span class="min-w-0 break-all font-mono">{{ model.model }}</span>
              <span class="shrink-0 font-semibold tabular-nums"
                >{{ model.share.toFixed(1) }}%</span
              >
            </li>
          </ol>
          <p v-else class="mt-1 text-sm text-muted-foreground">
            No model usage in this period
          </p>
        </div>
        <div
          role="group"
          aria-label="Image format"
          class="flex flex-wrap gap-2"
        >
          <Button
            v-for="option in [
              { value: 'landscape', label: 'Landscape' },
              { value: 'square', label: 'Square' },
            ] as const"
            :key="option.value"
            type="button"
            :variant="format === option.value ? 'default' : 'outline'"
            :aria-pressed="format === option.value"
            @click="format = option.value"
            >{{ option.label }}</Button
          >
        </div>
        <div class="flex flex-wrap gap-2">
          <Button :disabled="unavailable" @click="downloadImage"
            ><Download /> Download PNG</Button
          >
          <Button variant="outline" :disabled="unavailable" @click="shareImage"
            ><Share2 /> {{ sharing ? "Sharing…" : "Share image" }}</Button
          >
        </div>
        <p v-if="preparing" class="text-sm text-muted-foreground" role="status">
          Preparing your image…
        </p>
        <div v-else-if="error" class="grid gap-2" role="alert">
          <p class="text-sm text-destructive">{{ error }}</p>
          <Button variant="outline" class="w-fit" @click="revision++"
            ><RefreshCw /> Try again</Button
          >
        </div>
        <p v-else class="text-xs leading-relaxed text-muted-foreground">
          {{ dimensions.width }} x {{ dimensions.height }} PNG. Share opens your
          device's share menu when available; otherwise it downloads the image.
        </p>
      </div>
    </CardContent>
  </Card>
</template>

<style scoped>
.share-panel {
  border-top: 4px solid var(--primary);
}
.share-preview {
  display: block;
  width: 100%;
  height: auto;
  margin-inline: auto;
  border: 5px solid #fff;
  box-shadow: 5px 5px 0 var(--editorial-navy);
}
.share-preview--square {
  max-width: 28rem;
}
</style>
