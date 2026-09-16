import { useSessionStorage } from "@vueuse/core";

/**
 * The client API key used by the self-service usage page.
 *
 * Kept in sessionStorage, not localStorage: closing the tab clears it, so a
 * customer machine does not retain a full `/v1` credential.
 */
const STORAGE_KEY = "alnair-router.client-key";

const key = useSessionStorage(STORAGE_KEY, "");

export function getClientKey(): string {
  return key.value.trim();
}

export function setClientKey(value: string): void {
  key.value = value.trim();
}

export function useClientKey() {
  return { key, set: setClientKey, clear: () => setClientKey("") };
}
