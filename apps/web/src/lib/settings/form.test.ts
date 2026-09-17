import { describe, expect, it } from "vitest";

import {
  applyOutputDirective,
  blankForm,
  buildPatch,
  hydrateForm,
  parseOrigins,
  readOutputDirective,
  toInt,
  type SettingsForm,
} from "./form";
import type { SettingsResponse } from "@/types/api";

const response = (
  overrides: Partial<SettingsResponse> = {},
): SettingsResponse =>
  ({
    server: {
      require_api_key: false,
      readiness_upstream_checks: false,
      public_usage: true,
      store_key_secrets: true,
      lan_access: false,
      cors_origins: [],
      admin_token_set: false,
    },
    router: {
      default_connection: null,
      max_attempts: 5,
      max_retries_per_tier: 2,
      max_retry_delay_ms: 30000,
      catalog_ttl_ms: 1000,
      connect_timeout_ms: 10000,
      idle_timeout_ms: 60000,
    },
    limits: {
      max_concurrent: 0,
      max_concurrent_per_connection: 0,
      acquire_timeout_ms: 30000,
    },
    rate_limit: { requests_per_minute: 0, burst: 0 },
    pricing: { sync_enabled: false, sync_interval_secs: 86400, source_url: "" },
    token_saver: {
      slimmer_enabled: true,
      slimmer_level: "minimal",
      headroom_enabled: false,
      headroom_url: "http://localhost:8787",
      headroom_timeout_ms: 2000,
      terse_enabled: false,
      caveman_enabled: false,
      caveman_level: "full",
      ponytail_enabled: false,
      ponytail_level: "full",
    },
    overrides: [],
    deployment: {
      host: "127.0.0.1",
      port: 7878,
      binds_loopback: true,
      serve_dashboard: true,
      tray: false,
      allow_unauthenticated_admin: false,
      database_url: "sqlite:alnair.db",
      secrets_key_set: true,
    },
    ...overrides,
  }) as SettingsResponse;

/** A form hydrated from `current`, so a patch built from it is empty. */
const hydrated = (current: SettingsResponse): SettingsForm => {
  const form = blankForm();
  hydrateForm(form, current);
  return form;
};

describe("toInt", () => {
  it("truncates, clamps and falls back to the minimum", () => {
    expect(toInt("42")).toBe(42);
    expect(toInt(7.9)).toBe(7);
    expect(toInt(-3)).toBe(0);
    expect(toInt(-3, 1)).toBe(1);
    expect(toInt("nope")).toBe(0);
    expect(toInt("nope", 5)).toBe(5);
    expect(toInt("")).toBe(0);
  });
});

describe("parseOrigins", () => {
  it("splits on newlines and commas, trimming and deduplicating", () => {
    expect(
      parseOrigins("https://a.example\n https://b.example ,https://a.example"),
    ).toEqual(["https://a.example", "https://b.example"]);
  });

  it("is empty for blank input", () => {
    expect(parseOrigins("")).toEqual([]);
    expect(parseOrigins("  \n , \n")).toEqual([]);
  });
});

describe("buildPatch", () => {
  it("is empty right after hydrating", () => {
    const current = response();
    expect(buildPatch(hydrated(current), current)).toEqual({});
  });

  it("carries only the fields that changed", () => {
    const current = response();
    const form = hydrated(current);
    form.max_attempts = 9;

    expect(buildPatch(form, current)).toEqual({ max_attempts: 9 });
  });

  it("normalizes numeric fields that arrive as strings", () => {
    const current = response();
    const form = hydrated(current);
    form.max_attempts = "9" as unknown as number;
    form.max_retries_per_tier = "3.8" as unknown as number;

    expect(buildPatch(form, current)).toEqual({
      max_attempts: 9,
      max_retries_per_tier: 3,
    });
  });

  it("clamps a cleared number input instead of sending NaN", () => {
    const current = response();
    const form = hydrated(current);
    form.max_attempts = "" as unknown as number;

    // max_attempts has a floor of 1: the server would reject 0.
    expect(buildPatch(form, current)).toEqual({ max_attempts: 1 });
  });

  it("sends null to clear the default connection and the admin token", () => {
    const current = response({
      router: { ...response().router, default_connection: "openai-main" },
      server: { ...response().server, admin_token_set: true },
    });
    const form = hydrated(current);
    form.default_connection = "";
    form.admin_token_enabled = false;

    expect(buildPatch(form, current)).toEqual({
      default_connection: null,
      admin_token: null,
    });
  });

  it("does not echo the admin token when it is left blank", () => {
    const current = response({
      server: { ...response().server, admin_token_set: true },
    });
    const form = hydrated(current);
    form.admin_token_enabled = true;
    form.admin_token = "";

    expect(buildPatch(form, current)).toEqual({});
  });

  it("sends a rotated admin token once one is typed", () => {
    const current = response({
      server: { ...response().server, admin_token_set: true },
    });
    const form = hydrated(current);
    form.admin_token_enabled = true;
    form.admin_token = "  hunter2  ";

    expect(buildPatch(form, current)).toEqual({ admin_token: "hunter2" });
  });

  it("compares CORS origins as a set, ignoring order and spacing", () => {
    const current = response({
      server: {
        ...response().server,
        cors_origins: ["https://a.example", "https://b.example"],
      },
    });
    const form = hydrated(current);
    form.cors_origins = "https://b.example\nhttps://a.example\n";

    expect(buildPatch(form, current)).toEqual({});
  });

  it("trims a trailing slash on the Headroom URL", () => {
    const current = response();
    const form = hydrated(current);
    form.headroom_url = "http://headroom.internal:9000/";

    expect(buildPatch(form, current)).toEqual({
      headroom_url: "http://headroom.internal:9000",
    });
  });

  it("ignores a trailing slash that only differs cosmetically", () => {
    const current = response();
    const form = hydrated(current);
    form.headroom_url = "http://localhost:8787/";

    expect(buildPatch(form, current)).toEqual({});
  });

  it("floors the Headroom timeout at 1ms", () => {
    const current = response();
    const form = hydrated(current);
    form.headroom_timeout_ms = "0" as unknown as number;

    expect(buildPatch(form, current)).toEqual({ headroom_timeout_ms: 1 });
  });
});

describe("output directive", () => {
  it("reads the single active directive", () => {
    const form = blankForm();
    expect(readOutputDirective(form)).toBe("off");

    applyOutputDirective(form, "terse");
    expect(readOutputDirective(form)).toBe("terse");
    expect(form.caveman_enabled).toBe(false);

    applyOutputDirective(form, "caveman");
    expect(readOutputDirective(form)).toBe("caveman");
    expect(form.terse_enabled).toBe(false);

    applyOutputDirective(form, "off");
    expect(readOutputDirective(form)).toBe("off");
    expect(form.terse_enabled).toBe(false);
    expect(form.caveman_enabled).toBe(false);
  });

  it("prefers Caveman when a stored config has both set", () => {
    expect(
      readOutputDirective({ terse_enabled: true, caveman_enabled: true }),
    ).toBe("caveman");
  });
});

describe("hydrateForm", () => {
  it("never echoes a stored admin token back into the form", () => {
    const form = blankForm();
    hydrateForm(
      form,
      response({ server: { ...response().server, admin_token_set: true } }),
    );

    expect(form.admin_token_enabled).toBe(true);
    expect(form.admin_token).toBe("");
  });

  it("round-trips a loaded response into an empty patch", () => {
    const current = response({
      router: { ...response().router, default_connection: "openai-main" },
      token_saver: {
        ...response().token_saver,
        headroom_enabled: true,
        terse_enabled: true,
        ponytail_enabled: true,
      },
    });

    expect(buildPatch(hydrated(current), current)).toEqual({});
  });
});
