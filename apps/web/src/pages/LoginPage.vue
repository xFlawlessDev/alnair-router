<script setup lang="ts">
import { KeyRound } from "@lucide/vue";
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { toast } from "vue-sonner";

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { ApiError, api } from "@/lib/api";
import { clearAuthStatus, loadAuthStatus } from "@/lib/authState";
import { setSession } from "@/lib/session";
import type { AuthStatus } from "@/types/api";

const route = useRoute();
const router = useRouter();

const status = ref<AuthStatus | null>(null);
const password = ref("");
const confirm = ref("");
const setupCode = ref("");
const busy = ref(false);
const error = ref<string | null>(null);

const setupMode = computed(() => status.value?.setup_required === true);

function redirectTarget(): string {
  const redirect = route.query.redirect;
  return typeof redirect === "string" && redirect.startsWith("/")
    ? redirect
    : "/";
}

onMounted(async () => {
  status.value = await loadAuthStatus(true);
  if (status.value?.authenticated) await router.replace(redirectTarget());
});

async function submit(): Promise<void> {
  if (setupMode.value) {
    if (!setupCode.value.trim()) {
      error.value = "Enter the setup code printed in the router log";
      return;
    }
    if (password.value !== confirm.value) {
      error.value = "Passwords do not match";
      return;
    }
  }

  busy.value = true;
  error.value = null;
  try {
    const session = setupMode.value
      ? await api.setup(setupCode.value.trim(), password.value)
      : await api.login(password.value);
    setSession(session);
    clearAuthStatus();
    toast.success(setupMode.value ? "Password set — signed in" : "Signed in");
    await router.replace(redirectTarget());
  } catch (caught) {
    error.value =
      caught instanceof ApiError ? caught.message : "Sign-in failed";
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="mx-auto flex w-full max-w-md flex-col gap-4 py-6 sm:py-16">
    <Card>
      <CardHeader class="items-center gap-3 text-center">
        <div
          class="flex size-12 items-center justify-center rounded-full bg-primary/10 text-primary"
        >
          <KeyRound class="size-6" />
        </div>
        <CardTitle class="text-lg">
          {{ setupMode ? "Set the dashboard password" : "Sign in" }}
        </CardTitle>
        <CardDescription>
          <template v-if="setupMode">
            No dashboard password exists yet. Copy the setup code from the
            router log, then choose a password of at least 8 characters.
          </template>
          <template v-else
            >Enter the dashboard password to manage this router.</template
          >
        </CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div v-if="setupMode" class="grid gap-2">
          <Label for="setup-code">Setup code</Label>
          <Input
            id="setup-code"
            v-model="setupCode"
            class="font-mono"
            autocomplete="off"
            placeholder="8f3a-2b91-c4d7-e5f6"
            spellcheck="false"
          />
        </div>
        <div class="grid gap-2">
          <Label for="login-password">Password</Label>
          <Input
            id="login-password"
            v-model="password"
            type="password"
            :autocomplete="setupMode ? 'new-password' : 'current-password'"
            @keyup.enter="submit"
          />
        </div>
        <div v-if="setupMode" class="grid gap-2">
          <Label for="login-password-confirm">Confirm password</Label>
          <Input
            id="login-password-confirm"
            v-model="confirm"
            type="password"
            autocomplete="new-password"
            @keyup.enter="submit"
          />
        </div>
        <p v-if="error" class="text-xs text-destructive">{{ error }}</p>
        <Button class="w-full" :disabled="busy || !password" @click="submit">
          <Spinner v-if="busy" />
          {{ setupMode ? "Set password" : "Sign in" }}
        </Button>
      </CardContent>
    </Card>
  </div>
</template>
