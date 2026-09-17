<script setup lang="ts">
import { CircleAlert, Copy, Terminal } from "@lucide/vue";
import { toast } from "vue-sonner";

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

defineEmits<{ "open-savings": [] }>();

interface Step {
  title: string;
  detail: string;
  command?: string;
}

/** The setup order an operator follows; kept in step with the proxy's own CLI. */
const STEPS: Step[] = [
  {
    title: "Install the Python CLI",
    detail:
      "The proxy ships with the Python package. The npm package of the same name is a library only, so it cannot serve /v1/compress.",
    command: 'pipx install "headroom-ai[all]"',
  },
  {
    title: "Start the proxy",
    detail:
      "Run it alongside the router and leave it up. It is a separate process — the router never starts or supervises it — so pick a port and keep it.",
    command: "headroom proxy --port 8787",
  },
  {
    title: "Point the router at it",
    detail:
      "Set Proxy URL above — http://localhost:8787 matches the default, and the URL is trimmed of trailing slashes. Save, then press Test connection: the probe calls GET /readyz and reports the proxy version.",
  },
];

const CAVEATS: string[] = [
  "/v1/compress is loopback-only on the proxy side. A container or remote hostname answers 404 by design, so the probe error tells you to set HEADROOM_COMPRESS_ALLOW_REMOTE=1 on the proxy before that URL will work.",
  "Test connection reports a reachable proxy that is not ready yet (503) separately from one that is unreachable — the first means wait, the second means start it.",
  "Timeout (ms) defaults to 2000. Too low and the proxy skips compressing internally rather than slowing the request down.",
  "Fail-open is the contract: an unreachable, slow, or confused proxy leaves the request untouched, and nothing here can fail a request.",
  "A 200 is not proof of work. The proxy answers compression_skipped: true after an internal timeout, and the router discards any saving that shrank the payload by less than 5% as noise — so a flat Headroom row can be the proxy declining rather than a bad URL.",
];

async function copy(command: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(command);
    toast.success("Command copied");
  } catch {
    toast.error("Clipboard is not available");
  }
}
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle class="flex items-center gap-2 text-base">
        <Terminal class="size-4 text-muted-foreground" />
        Running Headroom
      </CardTitle>
      <CardDescription>
        The deeper-compression pass is a separate proxy, not part of the router
        binary. Start it before switching the saver on.
      </CardDescription>
    </CardHeader>
    <CardContent class="grid gap-4">
      <ol class="grid gap-3">
        <li
          v-for="(step, index) in STEPS"
          :key="step.title"
          class="grid gap-2 rounded-lg border border-border/70 px-4 py-3.5"
        >
          <div class="flex items-start gap-3">
            <span
              class="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-medium"
              aria-hidden="true"
            >
              {{ index + 1 }}
            </span>
            <div class="grid min-w-0 gap-1">
              <span class="text-sm leading-snug font-medium">{{
                step.title
              }}</span>
              <p
                class="text-xs leading-relaxed text-pretty text-muted-foreground"
              >
                {{ step.detail }}
              </p>
            </div>
          </div>

          <div
            v-if="step.command"
            class="flex items-center gap-2 rounded-md border bg-muted/40 py-1.5 ps-3 pe-1.5"
          >
            <code
              class="min-w-0 flex-1 overflow-x-auto font-mono text-xs whitespace-pre"
              >{{ step.command }}</code
            >
            <Button
              variant="ghost"
              size="icon-sm"
              :aria-label="`Copy command: ${step.command}`"
              @click="copy(step.command)"
            >
              <Copy />
            </Button>
          </div>
        </li>
      </ol>

      <div class="grid gap-2">
        <p class="text-xs font-medium">Worth knowing</p>
        <ul class="grid gap-2">
          <li
            v-for="caveat in CAVEATS"
            :key="caveat"
            class="flex items-start gap-2 text-xs leading-relaxed text-pretty text-muted-foreground"
          >
            <CircleAlert class="mt-px size-3.5 shrink-0" />
            <span>{{ caveat }}</span>
          </li>
        </ul>
      </div>

      <p class="text-xs text-muted-foreground">
        Headroom runs after RTK / Slimmer and before the output directives, and
        reports measured savings on the
        <button
          type="button"
          class="underline underline-offset-4"
          @click="$emit('open-savings')"
        >
          Savings
        </button>
        tab.
      </p>
    </CardContent>
  </Card>
</template>
