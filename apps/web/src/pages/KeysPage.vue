<script setup lang="ts">
import {
  Check,
  Copy,
  Eye,
  EyeOff,
  KeyRound,
  Pencil,
  Plus,
  RefreshCw,
  RotateCw,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "@lucide/vue";
import { computed, onMounted, ref } from "vue";
import { toast } from "vue-sonner";

import ConfirmDialog from "@/components/ConfirmDialog.vue";
import EmptyState from "@/components/EmptyState.vue";
import PageHeader from "@/components/PageHeader.vue";
import StatusBadge from "@/components/StatusBadge.vue";
import BudgetUsage from "@/components/keys/BudgetUsage.vue";
import KeyFormDialog from "@/components/keys/KeyFormDialog.vue";
import PlanFormDialog from "@/components/keys/PlanFormDialog.vue";
import PlansCard from "@/components/keys/PlansCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ApiError, api } from "@/lib/api";
import { formatDateTime, formatCost, isEnabled } from "@/lib/format";
import {
  configuredCaps,
  effectiveCaps,
  effectiveTokenCaps,
  isExpired,
} from "@/lib/limits";
import type {
  Alias,
  ApiKey,
  ComboWithEntries,
  KeyPlan,
  KeySpend,
} from "@/types/api";

/** Sentinel because Select values cannot be empty strings. */
const NO_PLAN = "__no_plan__";

const keys = ref<ApiKey[]>([]);
const plans = ref<KeyPlan[]>([]);
const aliases = ref<Alias[]>([]);
const combos = ref<ComboWithEntries[]>([]);
const spends = ref<KeySpend[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const formOpen = ref(false);
const editing = ref<ApiKey | null>(null);
const secret = ref<string | null>(null);
const copied = ref(false);
/** Decrypted secret per key id; absent until the row is revealed. */
const revealed = ref<Map<string, string>>(new Map());
/** Key ids whose secret was not stored, so reveal cannot be offered. */
const unrevealable = ref<Set<string>>(new Set());
const revealing = ref<string | null>(null);
const rotating = ref<ApiKey | null>(null);
const rotatingBusy = ref(false);
const deleting = ref<ApiKey | null>(null);
const deletingBusy = ref(false);
const planFormOpen = ref(false);
const editingPlan = ref<KeyPlan | null>(null);
const deletingPlan = ref<KeyPlan | null>(null);
const deletingPlanBusy = ref(false);

const plansById = computed(
  () => new Map(plans.value.map((plan) => [plan.id, plan])),
);
const spendsById = computed(
  () => new Map(spends.value.map((spend) => [spend.api_key_id, spend])),
);

/** The plan supplying caps a key leaves empty, if any. */
function planFor(key: ApiKey): KeyPlan | undefined {
  return key.plan_id ? plansById.value.get(key.plan_id) : undefined;
}

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const [keyList, planList, aliasList, comboList, spendList] =
      await Promise.all([
        api.listKeys(),
        api.listPlans(),
        api.listAliases(),
        api.listCombos(),
        api.usageByKey(),
      ]);
    keys.value = keyList;
    plans.value = planList;
    aliases.value = aliasList;
    combos.value = comboList;
    spends.value = spendList;
  } catch (caught) {
    error.value =
      caught instanceof ApiError ? caught.message : "Failed to load API keys";
  } finally {
    loading.value = false;
  }
}

function openCreate(): void {
  editing.value = null;
  formOpen.value = true;
}

function openEdit(key: ApiKey): void {
  editing.value = key;
  formOpen.value = true;
}

function openCreatePlan(): void {
  editingPlan.value = null;
  planFormOpen.value = true;
}

function openEditPlan(plan: KeyPlan): void {
  editingPlan.value = plan;
  planFormOpen.value = true;
}

function handleSaved(createdSecret?: string): void {
  if (createdSecret) {
    secret.value = createdSecret;
    copied.value = false;
  }
  load();
}

/** Applies (or detaches) a plan straight from the key row. */
async function applyPlanValue(key: ApiKey, value: unknown): Promise<void> {
  const planId = typeof value === "string" && value !== NO_PLAN ? value : null;
  if (planId === key.plan_id) return;

  const plan = planId ? plansById.value.get(planId) : null;
  try {
    await api.updateKey(key.id, { plan_id: planId });
    toast.success(
      plan
        ? `Plan “${plan.name}” applied to “${key.name}”`
        : `Plan detached from “${key.name}”`,
    );
    await load();
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to update the key",
    );
  }
}

async function confirmDeletePlan(): Promise<void> {
  if (!deletingPlan.value) return;
  deletingPlanBusy.value = true;
  const name = deletingPlan.value.name;
  try {
    await api.deletePlan(deletingPlan.value.id);
    toast.success(`Plan “${name}” deleted`);
    deletingPlan.value = null;
    await load();
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to delete the plan",
    );
  } finally {
    deletingPlanBusy.value = false;
  }
}

async function copySecret(value: string | null): Promise<void> {
  if (!value) return;
  try {
    await navigator.clipboard.writeText(value);
    copied.value = true;
    toast.success("Key copied to clipboard");
  } catch {
    toast.error("Clipboard is unavailable");
  }
}

/** Shows the row's secret, fetching it once and then toggling locally. */
async function revealSecret(key: ApiKey): Promise<void> {
  if (revealed.value.has(key.id)) {
    const next = new Map(revealed.value);
    next.delete(key.id);
    revealed.value = next;
    return;
  }

  revealing.value = key.id;
  try {
    const { secret: value } = await api.revealKey(key.id);
    revealed.value = new Map(revealed.value).set(key.id, value);
  } catch (caught) {
    if (caught instanceof ApiError && caught.status === 404) {
      unrevealable.value = new Set(unrevealable.value).add(key.id);
      toast.error(
        "This key's secret was not stored — rotate it to get a new one",
      );
    } else {
      toast.error(
        caught instanceof ApiError
          ? caught.message
          : "Failed to reveal the key",
      );
    }
  } finally {
    revealing.value = null;
  }
}

async function confirmRotate(): Promise<void> {
  if (!rotating.value) return;
  rotatingBusy.value = true;
  const { id, name } = rotating.value;
  try {
    const created = await api.rotateKey(id);
    secret.value = created.secret;
    copied.value = false;
    // The stored copy is now stale, and the key can be revealed again.
    const nextRevealed = new Map(revealed.value);
    nextRevealed.delete(id);
    revealed.value = nextRevealed;
    const nextUnrevealable = new Set(unrevealable.value);
    nextUnrevealable.delete(id);
    unrevealable.value = nextUnrevealable;
    toast.success(`Key “${name}” rotated`);
    rotating.value = null;
    await load();
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to rotate the key",
    );
  } finally {
    rotatingBusy.value = false;
  }
}

async function confirmDelete(): Promise<void> {
  if (!deleting.value) return;
  deletingBusy.value = true;
  const name = deleting.value.name;
  try {
    await api.deleteKey(deleting.value.id);
    toast.success(`Key “${name}” deleted`);
    deleting.value = null;
    await load();
  } catch (caught) {
    toast.error(
      caught instanceof ApiError ? caught.message : "Failed to delete key",
    );
  } finally {
    deletingBusy.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-6">
    <PageHeader
      title="API Keys"
      description="Router-issued client keys for /v1/*. Stored hashed, with an encrypted copy for reveal."
    >
      <template #actions>
        <Button variant="outline" :disabled="loading" @click="load">
          <RefreshCw :class="loading ? 'animate-spin' : ''" /> Refresh
        </Button>
        <Button @click="openCreate"><Plus /> Create key</Button>
      </template>
    </PageHeader>

    <Card v-if="error">
      <CardContent class="p-6 text-sm text-destructive">{{
        error
      }}</CardContent>
    </Card>

    <p v-else-if="loading" class="text-sm text-muted-foreground">
      Loading API keys…
    </p>

    <EmptyState
      v-else-if="!keys.length"
      title="No API keys yet"
      description="Keys authenticate /v1 requests when server.require_api_key is enabled. Reveal a key to copy it, or rotate it to mint a new secret."
    >
      <template #icon><KeyRound class="size-5" /></template>
      <template #action>
        <Button @click="openCreate"><Plus /> Create key</Button>
      </template>
    </EmptyState>

    <Card v-else>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Key</TableHead>
            <TableHead>Rules</TableHead>
            <TableHead>Limits</TableHead>
            <TableHead>Usage</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Expires</TableHead>
            <TableHead>Created</TableHead>
            <TableHead>Last used</TableHead>
            <TableHead class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="key in keys" :key="key.id">
            <TableCell class="font-medium">{{ key.name }}</TableCell>
            <TableCell>
              <div class="flex items-center gap-1">
                <code class="rounded bg-muted px-1.5 py-0.5 text-xs">
                  {{ revealed.get(key.id) ?? `${key.prefix}…` }}
                </code>
                <template v-if="!unrevealable.has(key.id)">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    :disabled="revealing === key.id"
                    :aria-label="
                      revealed.has(key.id)
                        ? `Hide key for ${key.name}`
                        : `Reveal key for ${key.name}`
                    "
                    @click="revealSecret(key)"
                  >
                    <EyeOff v-if="revealed.has(key.id)" class="size-3" />
                    <Eye v-else class="size-3" />
                  </Button>
                  <Button
                    v-if="revealed.has(key.id)"
                    variant="ghost"
                    size="icon-sm"
                    :aria-label="`Copy key for ${key.name}`"
                    @click="copySecret(revealed.get(key.id) ?? null)"
                  >
                    <Copy class="size-3" />
                  </Button>
                </template>
                <Button
                  v-else
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="`Rotate key for ${key.name}`"
                  @click="rotating = key"
                >
                  <RotateCw class="size-3" />
                </Button>
              </div>
            </TableCell>
            <TableCell>
              <div class="grid gap-1">
                <Select
                  :model-value="key.plan_id ?? NO_PLAN"
                  @update:model-value="applyPlanValue(key, $event)"
                >
                  <SelectTrigger
                    class="h-7 w-36 text-xs"
                    :aria-label="`Plan for ${key.name}`"
                  >
                    <SelectValue placeholder="No plan" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem :value="NO_PLAN">No plan</SelectItem>
                    <SelectItem
                      v-for="plan in plans"
                      :key="plan.id"
                      :value="plan.id"
                    >
                      {{ plan.name }}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <Badge
                  v-if="key.allowed_models?.length"
                  variant="secondary"
                  class="w-fit gap-1"
                >
                  <ShieldCheck class="size-3" />
                  {{ key.allowed_models.length }}
                  model{{ key.allowed_models.length > 1 ? "s" : "" }}
                </Badge>
                <Badge
                  v-if="key.plan_id && isExpired(planFor(key)?.expires_at)"
                  variant="destructive"
                  class="w-fit"
                >
                  Plan expired — key rejected
                </Badge>
                <span
                  v-else-if="!key.plan_id"
                  class="text-xs text-muted-foreground"
                >
                  Any model
                </span>
              </div>
            </TableCell>
            <TableCell>
              <div class="flex flex-wrap items-center gap-1">
                <Badge v-if="key.rate_limit_per_minute" variant="secondary">
                  {{ key.rate_limit_per_minute }}/min
                </Badge>
                <Badge
                  v-for="{ cap, amount } in configuredCaps(key)"
                  :key="cap.field"
                  :variant="
                    key.budget_mode === 'block' ? 'destructive' : 'outline'
                  "
                >
                  {{ formatCost(amount) }}/{{ cap.suffix }}
                </Badge>
                <Badge v-if="configuredCaps(key).length" variant="secondary">
                  {{ key.budget_mode }}
                </Badge>
                <span
                  v-if="
                    !key.rate_limit_per_minute && !configuredCaps(key).length
                  "
                  class="text-muted-foreground"
                >
                  Default
                </span>
              </div>
            </TableCell>
            <TableCell>
              <BudgetUsage
                :spend="spendsById.get(key.id) ?? null"
                :caps="effectiveCaps(key, planFor(key))"
                :token-caps="effectiveTokenCaps(key, planFor(key))"
              />
            </TableCell>
            <TableCell
              ><StatusBadge :enabled="isEnabled(key.enabled)"
            /></TableCell>
            <TableCell class="text-xs">
              <Badge v-if="isExpired(key.expires_at)" variant="destructive"
                >Expired</Badge
              >
              <span v-else class="text-muted-foreground">
                {{ formatDateTime(key.expires_at) }}
              </span>
            </TableCell>
            <TableCell class="text-xs text-muted-foreground">
              {{ formatDateTime(key.created_at) }}
            </TableCell>
            <TableCell class="text-xs text-muted-foreground">
              {{ formatDateTime(key.last_used_at) }}
            </TableCell>
            <TableCell class="text-right">
              <div class="flex justify-end gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  :aria-label="`Edit ${key.name}`"
                  @click="openEdit(key)"
                >
                  <Pencil />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="text-destructive hover:text-destructive"
                  :aria-label="`Delete ${key.name}`"
                  @click="deleting = key"
                >
                  <Trash2 />
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </Card>

    <PlansCard
      :plans="plans"
      @create="openCreatePlan"
      @edit="openEditPlan"
      @remove="deletingPlan = $event"
    />

    <KeyFormDialog
      v-model:open="formOpen"
      :api-key="editing"
      :plans="plans"
      :aliases="aliases"
      :combos="combos"
      @saved="handleSaved"
    />

    <PlanFormDialog
      v-model:open="planFormOpen"
      :plan="editingPlan"
      :aliases="aliases"
      :combos="combos"
      @saved="load"
    />

    <Dialog :open="secret !== null" @update:open="secret = null">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Copy your key now</DialogTitle>
          <DialogDescription class="flex items-start gap-2">
            <TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
            Store it somewhere safe. You can reveal it again from the key row
            while server.store_key_secrets is on.
          </DialogDescription>
        </DialogHeader>
        <div class="flex items-center gap-2">
          <code
            class="min-w-0 flex-1 overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs"
          >
            {{ secret }}
          </code>
          <Button
            variant="outline"
            size="icon"
            aria-label="Copy key"
            @click="copySecret(secret)"
          >
            <Check v-if="copied" class="text-primary" />
            <Copy v-else />
          </Button>
        </div>
        <DialogFooter>
          <Button @click="secret = null">Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <ConfirmDialog
      :open="rotating !== null"
      title="Rotate API key?"
      :description="`“${rotating?.name}” gets a new secret and clients using the old one immediately receive 401 responses.`"
      :pending="rotatingBusy"
      @update:open="rotating = $event ? rotating : null"
      @confirm="confirmRotate"
    />

    <ConfirmDialog
      :open="deleting !== null"
      title="Delete API key?"
      :description="`Clients using “${deleting?.name}” will immediately receive 401 responses.`"
      :pending="deletingBusy"
      @update:open="deleting = $event ? deleting : null"
      @confirm="confirmDelete"
    />

    <ConfirmDialog
      :open="deletingPlan !== null"
      title="Delete plan?"
      :description="`Keys using “${deletingPlan?.name}” keep their own rules and fall back to the server defaults.`"
      :pending="deletingPlanBusy"
      @update:open="deletingPlan = $event ? deletingPlan : null"
      @confirm="confirmDeletePlan"
    />
  </div>
</template>
