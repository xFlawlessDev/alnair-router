import { ref } from "vue";

import { ApiError, api } from "@/lib/api";
import type { AuthStatus } from "@/types/api";

/** Cached `/api/auth/status`, shared by the router guard and the login page. */
const status = ref<AuthStatus | null>(null);

export function authStatus(): AuthStatus | null {
  return status.value;
}

/** Loads the status once; `force` re-reads it after login or logout. */
export async function loadAuthStatus(
  force = false,
): Promise<AuthStatus | null> {
  if (status.value !== null && !force) return status.value;
  try {
    status.value = await api.authStatus();
  } catch (caught) {
    // An unreachable router must not trap the user on the login screen.
    if (!(caught instanceof ApiError)) throw caught;
    status.value = null;
  }
  return status.value;
}

export function clearAuthStatus(): void {
  status.value = null;
}
