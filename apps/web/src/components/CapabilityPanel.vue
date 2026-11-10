<script setup lang="ts">
import { computed, ref } from "vue";
import { toast } from "vue-sonner";
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
const levels = ref("");
const input = ref("");
const output = ref("");
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
const parseList = (value: string) =>
  value.trim() === "-"
    ? []
    : value.trim()
      ? value
          .split(",")
          .map((part) => part.trim())
          .filter(Boolean)
      : null;
const parseBool = (value: string) => (value === "" ? null : value === "yes");

function edit() {
  const value = manual.value;
  context.value = value?.context?.toString() ?? "";
  reasoning.value =
    value?.reasoning == null ? "" : value.reasoning ? "yes" : "no";
  toolCall.value =
    value?.tool_call == null ? "" : value.tool_call ? "yes" : "no";
  levels.value = value?.reasoning_levels
    ? value.reasoning_levels.join(", ") || "-"
    : "";
  input.value = value?.input_formats
    ? value.input_formats.join(", ") || "-"
    : "";
  output.value = value?.output_formats
    ? value.output_formats.join(", ") || "-"
    : "";
  open.value = true;
}

async function save() {
  const number = context.value.trim() ? Number(context.value) : null;
  if (number != null && (!Number.isSafeInteger(number) || number <= 0)) {
    toast.error("Context must be a positive whole number");
    return;
  }
  const value: ModelCapabilities = {
    context: number,
    reasoning: parseBool(reasoning.value),
    tool_call: parseBool(toolCall.value),
    reasoning_levels: parseList(levels.value),
    input_formats: parseList(input.value),
    output_formats: parseList(output.value),
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
            >Blank fields use catalog data. Overrides on a combo apply to each
            fallback tier.</DialogDescription
          ></DialogHeader
        >
        <div class="grid gap-4">
          <div class="grid gap-2">
            <Label :for="`cap-context-${id}`">Context tokens</Label
            ><Input
              :id="`cap-context-${id}`"
              v-model="context"
              type="number"
              min="1"
              step="1"
              placeholder="Use catalog"
            />
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
          <div class="grid gap-2">
            <Label :for="`cap-levels-${id}`">Reasoning levels</Label
            ><Input
              :id="`cap-levels-${id}`"
              v-model="levels"
              placeholder="low, medium, high"
            />
          </div>
          <div class="grid gap-2">
            <Label :for="`cap-input-${id}`">Input formats</Label
            ><Input
              :id="`cap-input-${id}`"
              v-model="input"
              placeholder="text, image, pdf"
            />
          </div>
          <div class="grid gap-2">
            <Label :for="`cap-output-${id}`">Output formats</Label
            ><Input
              :id="`cap-output-${id}`"
              v-model="output"
              placeholder="text, audio"
            />
          </div>
          <p class="text-xs text-muted-foreground">
            Separate values with commas; enter a dash for none. This information
            does not change routing.
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
