<script setup lang="ts">
import PageHeader from '@/components/PageHeader.vue';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';

const modelForms = [
  {
    ref: 'oa/gpt-4o-mini',
    kind: 'Alias',
    detail: 'The prefix oa routes to its connection and asks the upstream for gpt-4o-mini.',
  },
  {
    ref: 'oa',
    kind: 'Alias',
    detail: 'A bare prefix works when the alias pins a model_override.',
  },
  {
    ref: 'free-forever',
    kind: 'Combo',
    detail: 'Tries each tier in order until one produces a first chunk.',
  },
  {
    ref: 'gpt-4o-mini',
    kind: 'Default',
    detail: 'A bare model name goes to the default connection.',
  },
];

const openAiEndpoints = [
  ['POST /v1/chat/completions', 'Chat completions: streaming (SSE) or plain JSON.'],
  ['POST /v1/responses', 'OpenAI Responses shape.'],
  ['GET /v1/models', 'Lists aliases, combos and connections as model ids.'],
  ['GET /v1/models/info', 'Per-reference metadata, including combo tiers.'],
  ['POST /v1/embeddings', 'Proxied to the resolved connection.'],
  ['POST /v1/images/generations', 'Proxied.'],
  ['POST /v1/audio/speech · /v1/audio/transcriptions', 'Proxied, multipart pass-through.'],
  ['POST /v1/videos/generations · GET /v1/videos/{id}', 'Proxied, async polling included.'],
  ['POST /v1/search · POST /v1/web/fetch', 'Proxied search and SSRF-guarded server-side fetch.'],
];

const anthropicEndpoints = [
  ['POST /v1/messages', 'Translates to the shared executor with proper event ordering.'],
  ['POST /v1/messages/count_tokens', 'Heuristic estimate, no tokenizer dependency.'],
];

const responseHeaders = [
  ['x-router-model', 'Upstream model that served the request.'],
  ['x-router-provider', 'Wire protocol: openai-compatible or anthropic-native.'],
  ['x-router-attempt', 'Number of attempts made before a tier answered.'],
  ['x-router-source', 'Provenance, e.g. alias:oa or combo:free-forever#2.'],
];

const errors = [
  ['401', 'authentication_error', 'Missing, unknown or disabled router key.'],
  ['403', 'permission_error', "The model is not in the key's allowlist."],
  ['404', 'not_found_error', 'The model reference resolves to nothing.'],
  ['402', 'insufficient_quota', 'Monthly budget reached with budget_mode = block.'],
  ['429', 'rate_limit_error', 'Token bucket or concurrency cap; honors Retry-After.'],
  ['502', 'upstream_error', 'Every tier failed; the last upstream error is reported.'],
];
</script>

<template>
  <div class="flex max-w-4xl flex-col gap-6">
    <PageHeader
      title="API Guide"
      description="Point any OpenAI- or Anthropic-compatible client at the router and use it like a provider."
    />

    <Card>
      <CardHeader>
        <CardTitle>Quick start</CardTitle>
        <CardDescription>
          The default bind is <code>http://127.0.0.1:7878</code>, so the OpenAI-compatible base
          URL is <code>http://127.0.0.1:7878/v1</code>. Mint a key on the API Keys page and send
          it as a bearer token; when <code>server.require_api_key = false</code> the header can be
          omitted.
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-3 text-sm">
        <pre class="overflow-x-auto rounded-md bg-muted p-3 text-xs">curl http://127.0.0.1:7878/v1/chat/completions \
  -H 'authorization: Bearer &lt;router-key&gt;' \
  -H 'content-type: application/json' \
  -d '{"model":"free-forever","messages":[{"role":"user","content":"hi"}]}'</pre>
        <p>
          SDKs work unchanged: set <code>base_url</code> to the router and use the router key as
          the API key.
        </p>
        <pre class="overflow-x-auto rounded-md bg-muted p-3 text-xs">from openai import OpenAI

client = OpenAI(base_url="http://127.0.0.1:7878/v1", api_key="&lt;router-key&gt;")
stream = client.chat.completions.create(
    model="free-forever",
    messages=[{"role": "user", "content": "hi"}],
    stream=True,
)</pre>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Model references</CardTitle>
        <CardDescription>
          The <code>model</code> field resolves against aliases, combos and the default
          connection.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="grid gap-2">
          <li
            v-for="form in modelForms"
            :key="form.ref"
            class="flex flex-col gap-0.5 rounded-md border px-3 py-2 sm:flex-row sm:items-center sm:gap-3"
          >
            <code class="w-32 shrink-0 text-xs font-medium">{{ form.ref }}</code>
            <span class="w-16 shrink-0 text-xs font-medium text-muted-foreground">
              {{ form.kind }}
            </span>
            <span class="text-xs text-muted-foreground">{{ form.detail }}</span>
          </li>
        </ul>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Endpoints</CardTitle>
        <CardDescription>
          The router speaks both wire protocols, so existing clients keep working.
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="grid gap-2">
          <h3 class="text-sm font-medium">OpenAI-compatible</h3>
          <ul class="grid gap-2">
            <li
              v-for="[endpoint, description] in openAiEndpoints"
              :key="endpoint"
              class="flex flex-col gap-0.5 rounded-md border px-3 py-2 sm:flex-row sm:items-center sm:gap-3"
            >
              <code class="shrink-0 text-xs font-medium">{{ endpoint }}</code>
              <span class="text-xs text-muted-foreground">{{ description }}</span>
            </li>
          </ul>
        </div>
        <div class="grid gap-2">
          <h3 class="text-sm font-medium">Anthropic-compatible</h3>
          <ul class="grid gap-2">
            <li
              v-for="[endpoint, description] in anthropicEndpoints"
              :key="endpoint"
              class="flex flex-col gap-0.5 rounded-md border px-3 py-2 sm:flex-row sm:items-center sm:gap-3"
            >
              <code class="shrink-0 text-xs font-medium">{{ endpoint }}</code>
              <span class="text-xs text-muted-foreground">{{ description }}</span>
            </li>
          </ul>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Streaming, failover and headers</CardTitle>
        <CardDescription>What happens after the router resolves the model reference.</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-3 text-sm">
        <p>
          A combo walks its tiers until one produces a first chunk; failover only happens before
          the first byte, so an error after content has started surfaces to the client instead of
          silently switching upstreams.
        </p>
        <p>Every successful response reports the routing decision:</p>
        <ul class="grid gap-2">
          <li
            v-for="[name, description] in responseHeaders"
            :key="name"
            class="flex flex-col gap-0.5 rounded-md border px-3 py-2 sm:flex-row sm:items-center sm:gap-3"
          >
            <code class="w-44 shrink-0 text-xs font-medium">{{ name }}</code>
            <span class="text-xs text-muted-foreground">{{ description }}</span>
          </li>
        </ul>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Errors and limits</CardTitle>
        <CardDescription>
          Failures return an OpenAI-style envelope: <code>{ "error": { message, type, code } }</code>.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="grid gap-2">
          <li
            v-for="[status, type, description] in errors"
            :key="status"
            class="flex flex-col gap-0.5 rounded-md border px-3 py-2 sm:flex-row sm:items-center sm:gap-3"
          >
            <code class="w-10 shrink-0 text-xs font-medium">{{ status }}</code>
            <code class="w-40 shrink-0 text-xs text-muted-foreground">{{ type }}</code>
            <span class="text-xs text-muted-foreground">{{ description }}</span>
          </li>
        </ul>
      </CardContent>
    </Card>
  </div>
</template>
