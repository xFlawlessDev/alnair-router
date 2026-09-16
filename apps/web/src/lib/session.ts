import { StorageSerializers, useStorage } from "@vueuse/core";

import type { AuthSession } from "@/types/api";

/**
 * The rotating access/refresh pair behind the dashboard password.
 *
 * Kept in localStorage so a refresh of the page does not sign the owner out;
 * the refresh token rotates on every use and a replayed token revokes the
 * whole family server-side. The serializer is explicit: a `null` default makes
 * vueuse infer its `any` serializer, which stores `String(value)` and would
 * persist the session as `"[object Object]"`.
 */
const STORAGE_KEY = "alnair-router.session";

const session = useStorage<AuthSession | null>(STORAGE_KEY, null, undefined, {
  serializer: StorageSerializers.object,
});

export function getSession(): AuthSession | null {
  return session.value;
}

export function getAccessToken(): string {
  return session.value?.access_token ?? "";
}

export function getRefreshToken(): string {
  return session.value?.refresh_token ?? "";
}

export function setSession(value: AuthSession | null): void {
  session.value = value;
}
