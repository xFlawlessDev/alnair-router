<script setup lang="ts">
import { computed, ref } from "vue";
import { toast } from "vue-sonner";
import CapabilityMultiSelect from "@/components/CapabilityMultiSelect.vue";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogScrollContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api } from "@/lib/api";
import {
  catalogMatch,
  combineCapabilities,
  type CapabilityTarget,
} from "@/lib/capabilities";
import type { CapabilityCatalog, ModelCapabilities } from "@/types/api";

const props = defineProps<{
  scope: "alias" | "combo";
  id: string;
  targets: CapabilityTarget[];
  catalog: CapabilityCatalog;
}>();
const emit = defineEmits<{ changed: [] }>();
const open = ref(false);
const busy = ref(false);
const context = ref("");
const reasoning = ref("");
const toolCall = ref("");
const levels = ref<string[] | null>(null);
const input = ref<string[] | null>(null);
const output = ref<string[] | null>(null);
const options = computed(() => {
  const result = {
    reasoning_levels: new Set(["none", "minimal", "low", "medium", "high", "xhigh", "max", "default"]),
    input_formats: new Set(["text", "image", "audio", "video", "pdf"]),
    output_formats: new Set(["text", "image", "audio", "video"]),
  };
  for (const entry of props.catalog.entries) {
    for (const key of ["reasoning_levels", "input_formats", "output_formats"] as const) {
      for (const value of entry.capabilities[key] ?? []) result[key].add(value);
    }
  }
  return {
    reasoning_levels: [...result.reasoning_levels],
    input_formats: [...result.input_formats],
    output_formats: [...result.output_formats],
  };
});
const manual = computed(
  () =>
    props.catalog.overrides.find(
      (row) => row.scope === props.scope && row.target_id === props.id,
    )?.capabilities ?? null,
);
const rows = computed(() =>
  props.targets.map((target) => {
    const aliasOverride =
      props.scope === "combo"
        ? (props.catalog.overrides.find(
            (row) => row.scope === "alias" && row.target_id === target.aliasId,
          )?.capabilities ?? null)
        : null;
    return {
      target,
      data: combineCapabilities(
        combineCapabilities(catalogMatch(props.catalog, target), aliasOverride),
        manual.value,
      ),
    };
  }),
);
const booleanText = (value: boolean | null) =>
  value == null ? "Unknown" : value ? "Yes" : "No";
const listText = (value: string[] | null) =>
  value == null ? "Unknown" : value.length ? value.join(", ") : "None";
const parseBool = (value: string) => (value === "" ? null : value === "yes");

function edit() {
  const value = manual.value;
  const tokens = value?.context;
  context.value = tokens == null ? "" : tokens % 1_000_000 === 0
    ? `${tokens / 1_000_000}M`
    : tokens % 1_000 === 0
      ? `${tokens / 1_000}K`
      : String(tokens);
  reasoning.value =
    value?.reasoning == null ? "" : value.reasoning ? "yes" : "no";
  toolCall.value =
    value?.tool_call == null ? "" : value.tool_call ? "yes" : "no";
  levels.value = value?.reasoning_levels?.slice() ?? null;
  input.value = value?.input_formats?.slice() ?? null;
  output.value = value?.output_formats?.slice() ?? null;
  open.value = true;
}

async function save() {
  const text = context.value.trim();
  const match = /^(\d+(?:\.\d+)?)([km])?$/i.exec(text);
  const multiplier = match?.[2]?.toUpperCase() === "M" ? 1_000_000
    : match?.[2] ? 1_000 : 1;
  const number = text ? (match ? Number(match[1]) * multiplier : NaN) : null;
  if (number != null && (!Number.isSafeInteger(number) || number <= 0)) {
    toast.error("Context must be a positive whole token count, e.g. 128K or 1M");
    return;
  }
  const value: ModelCapabilities = {
    context: number,
    reasoning: parseBool(reasoning.value),
    tool_call: parseBool(toolCall.value),
    reasoning_levels: levels.value,
    input_formats: input.value,
    output_formats: output.value,
  };
  busy.value = true;
  try {
    await api.saveCapabilities(props.scope, props.id, value);
    emit("changed");
    open.value = false;
    toast.success("Capabilities saved");
  } catch {
    toast.error("Could not save capabilities");
  } finally {
    busy.value = false;
  }
}
async function reset() {
  busy.value = true;
  try {
    await api.deleteCapabilities(props.scope, props.id);
    emit("changed");
    open.value = false;
    toast.success("Capability override removed");
  } catch {
    toast.error("Could not remove capability override");
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section
    class="grid gap-2 border-t pt-3"
    :aria-label="`Capabilities for ${scope}`"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="editorial-kicker">
        Model capabilities <span v-if="manual">· override</span>
      </p>
      <Button variant="outline" size="sm" @click="edit">{{
        manual ? "Edit capabilities" : "Override capabilities"
      }}</Button>
    </div>
    <p v-if="!rows.length" class="text-xs text-muted-foreground">
      No enabled tiers to inspect.
    </p>
    <div
      v-for="(row, index) in rows"
      :key="`${row.target.label}-${index}`"
      class="grid gap-2 text-xs"
    >
      <p v-if="scope === 'combo'" class="break-all font-medium">
        {{ row.target.label }}
      </p>
      <p v-if="!row.data" class="text-muted-foreground">
        No unambiguous catalog match. Add an override or select a pinned model.
      </p>
      <dl v-else class="grid grid-cols-2 gap-x-4 gap-y-2 sm:grid-cols-3">
        <div>
          <dt class="text-muted-foreground">Context</dt>
          <dd>{{ row.data.context?.toLocaleString() ?? "Unknown" }}</dd>
        </div>
        <div>
          <dt class="text-muted-foreground">Reasoning</dt>
          <dd>{{ booleanText(row.data.reasoning) }}</dd>
        </div>
        <div>
          <dt class="text-muted-foreground">Tool calling</dt>
          <dd>{{ booleanText(row.data.tool_call) }}</dd>
        </div>
        <div>
          <dt class="text-muted-foreground">Reasoning levels</dt>
          <dd class="break-words">{{ listText(row.data.reasoning_levels) }}</dd>
        </div>
        <div>
          <dt class="text-muted-foreground">Input</dt>
          <dd class="break-words">{{ listText(row.data.input_formats) }}</dd>
        </div>
        <div>
          <dt class="text-muted-foreground">Output</dt>
          <dd class="break-words">{{ listText(row.data.output_formats) }}</dd>
        </div>
      </dl>
    </div>
    <Dialog v-model:open="open">
      <DialogScrollContent>
        <DialogHeader
          ><DialogTitle>Override model capabilities</DialogTitle
          ><DialogDescription
            >Blank fields use catalog data. Select “Use catalog” to inherit
            list values. Overrides on a combo apply
            to each fallback tier.</DialogDescription
          ></DialogHeader
        >
        <div class="grid gap-4">
          <div class="grid gap-2">
            <Label :for="`cap-context-${id}`">Context tokens</Label
            ><Input
              :id="`cap-context-${id}`"
              v-model="context"
              type="text"
              :aria-describedby="`cap-context-guide-${id}`"
              placeholder="Use catalog (e.g. 128K or 1M)"
            />
            <p :id="`cap-context-guide-${id}`" class="text-xs text-muted-foreground">
              Enter a context window such as 128K or 1M (K = 1,000 tokens;
              M = 1,000,000). Exact token counts are also accepted. Check your
              provider's documentation for the limit, or leave blank to use the
              catalog. This is informational and does not enforce a limit.
            </p>
          </div>
          <div class="grid gap-4 sm:grid-cols-2">
            <div class="grid gap-2">
              <Label :for="`cap-reasoning-${id}`">Reasoning</Label
              ><select
                :id="`cap-reasoning-${id}`"
                v-model="reasoning"
                class="border-input bg-background h-9 rounded-md border px-3 text-sm"
              >
                <option value="">Use catalog</option>
                <option value="yes">Yes</option>
                <option value="no">No</option>
              </select>
            </div>
            <div class="grid gap-2">
              <Label :for="`cap-tools-${id}`">Tool calling</Label
              ><select
                :id="`cap-tools-${id}`"
                v-model="toolCall"
                class="border-input bg-background h-9 rounded-md border px-3 text-sm"
              >
                <option value="">Use catalog</option>
                <option value="yes">Yes</option>
                <option value="no">No</option>
              </select>
            </div>
          </div>
          <CapabilityMultiSelect
            v-model="levels"
            label="Reasoning levels"
            :options="options.reasoning_levels"
          />
          <CapabilityMultiSelect
            v-model="input"
            label="Input formats"
            :options="options.input_formats"
          />
          <CapabilityMultiSelect
            v-model="output"
            label="Output formats"
            :options="options.output_formats"
          />
          <p class="text-xs text-muted-foreground">
            This information does not change routing.
          </p>
        </div>
        <DialogFooter
          ><Button
            v-if="manual"
            variant="outline"
            :disabled="busy"
            @click="reset"
            >Remove override</Button
          ><Button :disabled="busy" @click="save"
            >Save capabilities</Button
          ></DialogFooter
        >
      </DialogScrollContent>
    </Dialog>
  </section>
</template>
