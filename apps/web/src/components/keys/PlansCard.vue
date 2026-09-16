<script setup lang="ts">
import { Pencil, Plus, Sparkles, Trash2 } from "@lucide/vue";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { formatCost, formatDateTime } from "@/lib/format";
import { configuredCaps, isExpired } from "@/lib/limits";
import type { KeyPlan } from "@/types/api";

defineProps<{ plans: KeyPlan[] }>();
const emit = defineEmits<{
  create: [];
  edit: [plan: KeyPlan];
  remove: [plan: KeyPlan];
}>();
</script>

<template>
  <Card>
    <CardHeader class="flex flex-row items-start justify-between gap-4">
      <div class="grid gap-1">
        <CardTitle>Plans &amp; templates</CardTitle>
        <CardDescription
          >Set the rules once, slap them on any key.</CardDescription
        >
      </div>
      <Button variant="outline" @click="emit('create')"
        ><Plus /> New plan</Button
      >
    </CardHeader>

    <CardContent>
      <p v-if="!plans.length" class="text-sm text-muted-foreground">
        No plans yet. Bundle a model allowlist, a rate limit and a budget, then
        apply the plan to any key with one click.
      </p>

      <ul v-else class="grid gap-3">
        <li
          v-for="plan in plans"
          :key="plan.id"
          class="flex flex-wrap items-start justify-between gap-3 rounded-md border p-3"
        >
          <div class="grid min-w-0 gap-1">
            <div class="flex flex-wrap items-center gap-2">
              <Sparkles class="size-3.5 text-muted-foreground" />
              <span class="text-sm font-medium">{{ plan.name }}</span>
              <Badge v-if="plan.rate_limit_per_minute" variant="secondary">
                {{ plan.rate_limit_per_minute }}/min
              </Badge>
              <Badge
                v-for="{ cap, amount } in configuredCaps(plan)"
                :key="cap.field"
                :variant="
                  plan.budget_mode === 'block' ? 'destructive' : 'outline'
                "
              >
                {{ formatCost(amount) }}/{{ cap.suffix }}
              </Badge>
              <Badge v-if="configuredCaps(plan).length" variant="secondary">
                {{ plan.budget_mode }}
              </Badge>
              <Badge v-if="isExpired(plan.expires_at)" variant="destructive"
                >Expired</Badge
              >
              <span
                v-else-if="plan.expires_at"
                class="text-xs text-muted-foreground"
              >
                expires {{ formatDateTime(plan.expires_at) }}
              </span>
            </div>
            <p v-if="plan.description" class="text-xs text-muted-foreground">
              {{ plan.description }}
            </p>
            <div class="flex flex-wrap gap-1 pt-1">
              <template v-if="plan.allowed_models.length">
                <Badge
                  v-for="pattern in plan.allowed_models"
                  :key="pattern"
                  variant="outline"
                  class="font-mono text-[10px]"
                >
                  {{ pattern }}
                </Badge>
              </template>
              <span v-else class="text-xs text-muted-foreground"
                >All models</span
              >
            </div>
          </div>

          <div class="flex gap-1">
            <Button
              variant="ghost"
              size="icon-sm"
              :aria-label="`Edit ${plan.name}`"
              @click="emit('edit', plan)"
            >
              <Pencil />
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              class="text-destructive hover:text-destructive"
              :aria-label="`Delete ${plan.name}`"
              @click="emit('remove', plan)"
            >
              <Trash2 />
            </Button>
          </div>
        </li>
      </ul>
    </CardContent>
  </Card>
</template>
