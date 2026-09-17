import {
  inject,
  provide,
  type ComputedRef,
  type InjectionKey,
  type Ref,
} from "vue";

import type { SettingsForm } from "@/lib/settings/form";
import type { SettingsPatch, SettingsResponse } from "@/types/api";

/**
 * Shared state for the settings sections. The six section components are
 * presentational: they read the form and the load state from here instead of
 * receiving three dozen props, and they never call the API themselves.
 */
export interface SettingsFormContext {
  form: SettingsForm;
  settings: Ref<SettingsResponse | null>;
  loading: Ref<boolean>;
  /** Unsaved diff against the last saved response. */
  patch: ComputedRef<SettingsPatch>;
  /** True while the initial load is in flight or has failed. */
  disabled: ComputedRef<boolean>;
  /** Admin token cannot be cleared on a non-loopback bind. */
  adminTokenLocked: ComputedRef<boolean>;
  /** True when the stored config has an admin token. */
  adminTokenSet: ComputedRef<boolean>;
  /** Re-reads settings; used by Data after a restore rewrites the database. */
  reload: () => Promise<void>;
}

const KEY: InjectionKey<SettingsFormContext> = Symbol("settings-form");

export function provideSettingsForm(
  context: SettingsFormContext,
): SettingsFormContext {
  provide(KEY, context);
  return context;
}

export function useSettingsForm(): SettingsFormContext {
  const context = inject(KEY);
  if (!context) {
    throw new Error(
      "useSettingsForm() must be called inside a settings section component",
    );
  }
  return context;
}

/** True when a patch carries no edits, so Save stays inert. */
export function patchIsEmpty(patch: SettingsPatch): boolean {
  return Object.keys(patch).length === 0;
}

/**
 * The admin token can only be cleared while the admin surface stays protected:
 * on a non-loopback bind without `allow_unauthenticated_admin`, clearing it
 * would reopen `/api/*` to the network.
 */
export function adminTokenIsLocked(settings: SettingsResponse | null): boolean {
  const info = settings?.deployment;
  if (!info) return false;
  return !info.binds_loopback && !info.allow_unauthenticated_admin;
}
