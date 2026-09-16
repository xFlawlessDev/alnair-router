import { useStorage } from "@vueuse/core";

/**
 * The optional bearer token guarding `/api/*` on the router.
 *
 * Admin routes are unauthenticated on loopback, but a non-loopback deployment
 * requires `server.admin_token`; the UI attaches this token when set.
 */
const STORAGE_KEY = "alnair-router.admin-token";

const token = useStorage(STORAGE_KEY, "");

export function getAdminToken(): string {
  return token.value.trim();
}

export function setAdminToken(value: string): void {
  token.value = value.trim();
}

export function useAdminToken() {
  return { token, set: setAdminToken, clear: () => setAdminToken("") };
}
