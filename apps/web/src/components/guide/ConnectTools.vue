<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { toast } from "vue-sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { api } from "@/lib/api";
import { codingToolInstructions, type CodingTool } from "@/lib/codingTools";

const tools: { id: CodingTool; label: string }[] = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "omp", label: "OMP" },
  { id: "pi", label: "Pi agent" },
  { id: "cursor", label: "Cursor" },
  { id: "cline", label: "Cline" },
  { id: "opencode", label: "OpenCode" },
];
const tool = ref<CodingTool>("claude");
const routerUrl = ref(window.location.origin);
const CUSTOM_MODEL = "__custom_model__";
const modelSelection = ref("");
const customModel = ref("");
const models = ref<string[]>([]);
const model = computed(() =>
  modelSelection.value === CUSTOM_MODEL
    ? customModel.value
    : modelSelection.value,
);
const instructions = computed(() =>
  codingToolInstructions(tool.value, routerUrl.value, model.value),
);

onMounted(async () => {
  try {
    const catalog = await api.modelCatalog();
    models.value = [...new Set(catalog.data.map((entry) => entry.id))];
    if (!modelSelection.value) modelSelection.value = models.value[0] ?? "";
  } catch {
    toast.error("Could not load the model catalog; choose a custom model.");
  }
});

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(instructions.value.content);
    toast.success("Configuration copied");
  } catch {
    toast.error("Could not copy configuration");
  }
}
</script>

<template>
  <section class="grid gap-4" aria-labelledby="connect-tools-title">
    <div>
      <h2 id="connect-tools-title" class="text-lg font-semibold">
        Connect your tools
      </h2>
      <p class="text-sm text-muted-foreground">
        Choose a tool and model to prepare its configuration.
      </p>
    </div>
    <div class="grid gap-4 sm:grid-cols-2">
      <div class="grid gap-2">
        <Label for="coding-tool">Tool</Label>
        <Select v-model="tool">
          <SelectTrigger id="coding-tool" class="w-full">
            <SelectValue placeholder="Select a tool" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="entry in tools"
              :key="entry.id"
              :value="entry.id"
            >
              {{ entry.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="grid gap-2">
        <Label for="coding-model">Model</Label>
        <Select v-model="modelSelection">
          <SelectTrigger id="coding-model" class="w-full">
            <SelectValue placeholder="Select a model" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="id in models" :key="id" :value="id">
              {{ id }}
            </SelectItem>
            <SelectItem :value="CUSTOM_MODEL">Custom model…</SelectItem>
          </SelectContent>
        </Select>
        <template v-if="modelSelection === CUSTOM_MODEL">
          <Label for="coding-custom-model">Custom model ID</Label>
          <Input
            id="coding-custom-model"
            v-model="customModel"
            placeholder="alias or model ID"
          />
        </template>
      </div>
    </div>
    <div class="grid gap-2">
      <Label for="coding-router-url">Router URL</Label>
      <Input
        id="coding-router-url"
        v-model="routerUrl"
        placeholder="http://127.0.0.1:7878"
      />
      <p class="text-xs text-muted-foreground">
        Use an address reachable from the machine running the tool.
      </p>
    </div>
    <p class="text-sm">
      <strong>{{ instructions.location }}</strong
      ><br />{{ instructions.detail }}
    </p>
    <pre class="editorial-code overflow-x-auto p-4 text-xs">{{
      instructions.content
    }}</pre>
    <div>
      <Button
        type="button"
        :disabled="!model.trim() || !routerUrl.trim()"
        @click="copy"
        >Copy configuration</Button
      >
    </div>
  </section>
</template>
