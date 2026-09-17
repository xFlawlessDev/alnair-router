import { computed, reactive, ref, type ComputedRef, type Ref } from "vue";
import { toast } from "vue-sonner";

import { ApiError, api } from "@/lib/api";
import { setAdminToken } from "@/lib/adminToken";
import {
  adminTokenIsLocked,
  provideSettingsForm,
  type SettingsFormContext,
} from "@/lib/settings/context";
import {
  blankForm,
  buildPatch,
  hydrateForm,
  type SettingsForm,
} from "@/lib/settings/form";
import type { SettingsPatch, SettingsResponse } from "@/types/api";

/**
 * Load/save state for a page that edits dashboard settings and provides it to
 * its section components. Settings and the token-saving page both edit the same
 * `PATCH /api/settings` surface, so they share this instead of duplicating the
 * load, diff, save and reset cycle.
 */
export interface SettingsController {
  form: SettingsForm;
  settings: Ref<SettingsResponse | null>;
  loading: Ref<boolean>;
  saving: Ref<boolean>;
  resetting: Ref<boolean>;
  patch: ComputedRef<SettingsPatch>;
  /** True when the saved response has at least one stored override. */
  customizedCount: ComputedRef<number>;
  /** True when the load failed outright, so there is nothing to edit. */
  failed: ComputedRef<boolean>;
  error: Ref<string | null>;
  load: () => Promise<void>;
  save: () => Promise<void>;
  /** Resolves true when the overrides were cleared. */
  reset: () => Promise<boolean>;
}

export function useSettingsController(): SettingsController {
  const settings = ref<SettingsResponse | null>(null);
  const loading = ref(true);
  const saving = ref(false);
  const resetting = ref(false);
  const error = ref<string | null>(null);

  const form = reactive<SettingsForm>(blankForm());

  /** Unsaved diff against the last saved response. */
  const patch = computed(() =>
    settings.value ? buildPatch(form, settings.value) : {},
  );

  const failed = computed(
    () => error.value !== null && settings.value === null,
  );
  const customizedCount = computed(() => settings.value?.overrides.length ?? 0);

  const context: SettingsFormContext = {
    form,
    settings,
    loading,
    patch,
    disabled: computed(() => loading.value || settings.value === null),
    adminTokenLocked: computed(() => adminTokenIsLocked(settings.value)),
    adminTokenSet: computed(
      () => settings.value?.server.admin_token_set ?? false,
    ),
    reload: load,
  };

  provideSettingsForm(context);

  async function load(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      const response = await api.settings();
      settings.value = response;
      hydrateForm(form, response);
    } catch (caught) {
      error.value =
        caught instanceof ApiError ? caught.message : "Failed to load settings";
    } finally {
      loading.value = false;
    }
  }

  async function save(): Promise<void> {
    const body = patch.value;
    if (Object.keys(body).length === 0) {
      toast.info("No changes to save");
      return;
    }

    saving.value = true;
    try {
      const response = await api.updateSettings(body);
      if (typeof body.admin_token === "string") setAdminToken(body.admin_token);
      if (body.admin_token === null) setAdminToken("");
      settings.value = response;
      hydrateForm(form, response);
      toast.success("Settings saved");
    } catch (caught) {
      toast.error(
        caught instanceof ApiError ? caught.message : "Failed to save settings",
      );
    } finally {
      saving.value = false;
    }
  }

  async function reset(): Promise<boolean> {
    resetting.value = true;
    try {
      const response = await api.resetSettings();
      settings.value = response;
      hydrateForm(form, response);
      toast.success("Overrides cleared; file configuration restored");
      return true;
    } catch (caught) {
      toast.error(
        caught instanceof ApiError
          ? caught.message
          : "Failed to reset settings",
      );
      return false;
    } finally {
      resetting.value = false;
    }
  }

  return {
    form,
    settings,
    loading,
    saving,
    resetting,
    patch,
    customizedCount,
    failed,
    error,
    load,
    save,
    reset,
  };
}
