<script setup lang="ts">
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';

const adminEndpoints = [
  ['GET /api/health', 'Liveness probe plus service version.'],
  ['GET /api/ready', 'Readiness: database check plus optional upstream reachability.'],
  ['GET /api/metrics', 'Prometheus text counters for routing and usage.'],
  ['GET /api/init', 'Whether at least one enabled connection exists.'],
  ['GET·POST /api/connections', 'Upstream endpoints and their credentials.'],
  ['PATCH·DELETE /api/connections/{id}', 'Update or remove a connection.'],
  ['GET·POST /api/aliases', 'Prefix → connection mappings.'],
  ['PATCH·DELETE /api/aliases/{id}', 'Update or remove an alias.'],
  ['GET·POST /api/combos', 'Named fallback chains (with ordered entries).'],
  ['PATCH·DELETE /api/combos/{id}', 'Update entries wholesale or remove a combo.'],
  ['GET·POST /api/keys', 'Mint router-issued client keys.'],
  ['DELETE /api/keys/{id}', 'Revoke a client key.'],
  ['GET /api/usage', 'Attempt log with limit/offset pagination.'],
  ['GET /api/usage/summary', 'Rollup aggregates, optionally since a timestamp.'],
];

const stack = ['Vue 3', 'Vite', 'TypeScript', 'Tailwind CSS v4', 'shadcn-vue (reka-ui)', 'Vue Router', 'Pinia'];
</script>

<template>
  <div class="flex max-w-3xl flex-col gap-6">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">About this dashboard</h1>
      <p class="mt-2 text-sm text-muted-foreground">
        The admin UI for <strong>alnair-router</strong> — a standalone, OpenAI-compatible AI router
        that resolves prefixed model IDs and expands combos into ordered fallback chains.
      </p>
    </div>

    <Card>
      <CardHeader>
        <CardTitle>Admin API surface</CardTitle>
        <CardDescription>
          Everything this UI calls. Admin routes are unauthenticated on loopback; a non-loopback
          bind requires <code>server.admin_token</code>.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="grid gap-2">
          <li
            v-for="[endpoint, description] in adminEndpoints"
            :key="endpoint"
            class="flex flex-col gap-0.5 rounded-md border px-3 py-2 sm:flex-row sm:items-center sm:gap-3"
          >
            <code class="shrink-0 text-xs font-medium">{{ endpoint }}</code>
            <span class="text-xs text-muted-foreground">{{ description }}</span>
          </li>
        </ul>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Running it</CardTitle>
        <CardDescription>Development workflow against a local router.</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-2 text-sm">
        <p>
          Start the router (<code class="rounded bg-muted px-1.5 py-0.5">cargo run -p alnair-router</code>),
          then run this app from <code class="rounded bg-muted px-1.5 py-0.5">apps/web</code>:
        </p>
        <pre class="overflow-x-auto rounded-md bg-muted p-3 text-xs">pnpm install
pnpm dev</pre>
        <p class="text-xs text-muted-foreground">
          Vite proxies <code>/api</code> and <code>/v1</code> to
          <code>http://127.0.0.1:7878</code> by default; override with
          <code>ALNAIR_ROUTER_URL</code>.
        </p>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Stack</CardTitle>
        <CardDescription>
          Scaffolded from the EvoFast <code>vue-tailwind-vite</code> template (MIT).
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
          <li v-for="item in stack" :key="item" class="rounded-md border bg-muted/30 px-3 py-2 text-sm">
            {{ item }}
          </li>
        </ul>
      </CardContent>
    </Card>
  </div>
</template>
