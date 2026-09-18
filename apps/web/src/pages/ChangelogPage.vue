<script setup lang="ts">
import {
  ArrowUpCircle,
  CircleAlert,
  CircleCheck,
  ExternalLink,
  GitCommitHorizontal,
  RefreshCw,
  ScrollText,
  Sparkles,
  Wand2,
} from "@lucide/vue";
import { computed, onMounted, ref } from "vue";

import EmptyState from "@/components/EmptyState.vue";
import PageHeader from "@/components/PageHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ApiError, api } from "@/lib/api";
import { changelog } from "@/lib/changelog";
import type { ChangelogRelease } from "@/lib/changelog";
import { formatDateTime } from "@/lib/format";
import type { UpdateStatus } from "@/types/api";

const update = ref<UpdateStatus | null>(null);
const checking = ref(false);
const updateError = ref<string | null>(null);
const search = ref("");

/** Icon and tint per conventional-commit group, in the file's own wording. */
function groupStyle(group: string): { icon: unknown; class: string } {
  const label = group.toLowerCase();
  if (label.includes("feature"))
    return { icon: Sparkles, class: "text-emerald-500" };
  if (label.includes("fix")) return { icon: Wand2, class: "text-amber-500" };
  return { icon: ScrollText, class: "text-muted-foreground" };
}

/**
 * Releases filtered by the search box. A match can be on the version, a change
 * line, or a scope. When the version itself matches the whole release is kept;
 * otherwise only the matching changes survive, and a group heading left with no
 * changes is dropped so the page shows no empty sections.
 */
const releases = computed<ChangelogRelease[]>(() => {
  const term = search.value.trim().toLowerCase();
  if (!term) return changelog;

  return changelog
    .map((release) => {
      if (release.version.toLowerCase().includes(term)) return release;
      const changes = release.changes.filter(
        (change) =>
          change.text.toLowerCase().includes(term) ||
          (change.scope ?? "").toLowerCase().includes(term) ||
          change.group.toLowerCase().includes(term),
      );
      const groups = release.groups.filter((group) =>
        changes.some((change) => change.group === group),
      );
      const paragraphs = release.paragraphs.filter((line) =>
        line.toLowerCase().includes(term),
      );
      return { ...release, changes, groups, paragraphs };
    })
    .filter(
      (release) => release.changes.length > 0 || release.paragraphs.length > 0,
    );
});

const currentVersion = computed(() => update.value?.version ?? "0.0.0");

/** True when the release heading names the version this router is running. */
function isRunning(release: ChangelogRelease): boolean {
  return release.version.replace(/^v/, "") === currentVersion.value;
}

async function checkForUpdates(refresh = false): Promise<void> {
  checking.value = true;
  updateError.value = null;
  try {
    update.value = await api.update(refresh);
  } catch (caught) {
    updateError.value =
      caught instanceof ApiError
        ? caught.message
        : "Could not check for updates";
  } finally {
    checking.value = false;
  }
}

onMounted(() => checkForUpdates());
</script>

<template>
  <div class="flex max-w-4xl flex-col gap-6">
    <PageHeader
      title="Changelog"
      description="Release notes for alnair-router, bundled with this build."
    >
      <template #actions>
        <Button
          variant="outline"
          :disabled="checking"
          @click="checkForUpdates(true)"
        >
          <RefreshCw :class="checking ? 'animate-spin' : ''" /> Check for
          updates
        </Button>
      </template>
    </PageHeader>

    <Card>
      <CardHeader class="pb-3">
        <CardTitle class="flex items-center gap-2 text-base">
          <ArrowUpCircle class="size-4" /> Version
        </CardTitle>
        <CardDescription>
          This router reports its version from
          <code class="rounded bg-muted px-1.5 py-0.5">/api/version</code>.
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-3 text-sm">
        <div class="flex flex-wrap items-center gap-2">
          <Badge variant="secondary">
            Running v{{ update?.version ?? "…" }}
          </Badge>

          <template v-if="update?.check_enabled === false">
            <Badge variant="outline">Update checks off</Badge>
            <span class="text-xs text-muted-foreground">
              Set <code>update.check_enabled = true</code> to be told about new
              releases.
            </span>
          </template>

          <template v-else-if="update?.update_available">
            <Badge variant="destructive">
              <ArrowUpCircle class="size-3" />
              v{{ update.latest_version }} available
            </Badge>
            <a
              v-if="update.release_url"
              :href="update.release_url"
              target="_blank"
              rel="noopener noreferrer"
              class="inline-flex items-center gap-1 text-xs underline underline-offset-4"
            >
              Release notes <ExternalLink class="size-3" />
            </a>
          </template>

          <template v-else-if="update?.latest_version">
            <Badge variant="outline">
              <CircleCheck class="size-3 text-emerald-500" /> Up to date
            </Badge>
          </template>

          <Badge v-else-if="update?.error" variant="outline">
            <CircleAlert class="size-3 text-amber-500" /> Check failed
          </Badge>
        </div>

        <p
          v-if="update?.error"
          class="text-xs text-muted-foreground"
          role="status"
        >
          {{ update.error }}
        </p>
        <p v-else-if="update?.checked_at" class="text-xs text-muted-foreground">
          Last checked {{ formatDateTime(update.checked_at) }}.
        </p>

        <p v-if="updateError" class="text-xs text-destructive" role="alert">
          {{ updateError }}
        </p>

        <p
          v-if="update?.update_available && update.release_notes"
          class="whitespace-pre-wrap rounded-md border p-3 text-xs text-muted-foreground"
        >
          {{ update.release_notes }}
        </p>
      </CardContent>
    </Card>

    <div class="flex flex-wrap items-center gap-3">
      <Input
        v-model="search"
        placeholder="Filter releases and changes…"
        class="sm:max-w-sm"
        aria-label="Filter the changelog"
      />
      <Badge variant="outline">{{ releases.length }} releases</Badge>
    </div>

    <p v-if="!changelog.length" class="text-sm text-muted-foreground">
      No release notes are bundled with this build.
    </p>

    <p
      v-else-if="search.trim() && !releases.length"
      class="text-sm text-muted-foreground"
    >
      Nothing matches "{{ search.trim() }}".
    </p>

    <EmptyState
      v-else-if="!releases.length"
      title="No changelog yet"
      description="Release notes appear here once the first version is tagged."
    >
      <template #icon><ScrollText class="size-5" /></template>
    </EmptyState>

    <template v-else>
      <Card v-for="release in releases" :key="release.version">
        <CardHeader class="pb-3">
          <CardTitle class="flex flex-wrap items-center gap-2 text-base">
            {{
              release.version === "Unreleased"
                ? "Unreleased"
                : `v${release.version}`
            }}
            <Badge
              v-if="isRunning(release) && release.version !== 'Unreleased'"
              variant="secondary"
            >
              Running
            </Badge>
            <Badge v-if="release.kind" variant="outline">{{
              release.kind
            }}</Badge>
            <span
              v-if="release.date"
              class="text-xs font-normal text-muted-foreground"
            >
              {{ release.date }}
            </span>
          </CardTitle>
        </CardHeader>
        <CardContent class="grid gap-4">
          <p
            v-for="(paragraph, index) in release.paragraphs"
            :key="`p-${index}`"
            class="text-sm text-muted-foreground"
          >
            {{ paragraph }}
          </p>

          <div v-for="group in release.groups" :key="group" class="grid gap-2">
            <h3
              class="flex items-center gap-2 text-sm font-medium"
              :class="groupStyle(group).class"
            >
              <component :is="groupStyle(group).icon" class="size-3.5" />
              {{ group }}
            </h3>
            <ul class="grid gap-1.5">
              <li
                v-for="(change, index) in release.changes.filter(
                  (item) => item.group === group,
                )"
                :key="index"
                class="flex items-start gap-2 text-sm"
              >
                <span
                  class="mt-1.5 size-1.5 shrink-0 rounded-full bg-muted-foreground/50"
                />
                <span>
                  <code
                    v-if="change.scope"
                    class="mr-1 rounded bg-muted px-1.5 py-0.5 text-xs"
                    >{{ change.scope }}</code
                  >
                  {{ change.text }}
                  <a
                    v-if="change.commit"
                    :href="change.commit.url"
                    target="_blank"
                    rel="noopener noreferrer"
                    class="ml-1 inline-flex items-center gap-0.5 font-mono text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
                    :title="`Commit ${change.commit.hash}`"
                  >
                    <GitCommitHorizontal class="size-3" />{{
                      change.commit.hash
                    }}
                  </a>
                </span>
              </li>
            </ul>
          </div>

          <ul
            v-if="release.changes.some((change) => !change.group)"
            class="grid gap-1.5"
          >
            <li
              v-for="(change, index) in release.changes.filter(
                (item) => !item.group,
              )"
              :key="index"
              class="flex items-start gap-2 text-sm"
            >
              <span
                class="mt-1.5 size-1.5 shrink-0 rounded-full bg-muted-foreground/50"
              />
              <span>{{ change.text }}</span>
            </li>
          </ul>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
