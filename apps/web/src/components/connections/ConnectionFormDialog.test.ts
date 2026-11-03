import { createApp, h, nextTick, ref } from "vue";
import { describe, expect, it, vi } from "vitest";

import ConnectionFormDialog from "./ConnectionFormDialog.vue";
import { api } from "@/lib/api";
import type { Connection } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: {
    listConnectionAccounts: vi.fn().mockResolvedValue([]),
    createConnectionAccount: vi.fn(),
    updateConnectionAccount: vi.fn(),
    deleteConnectionAccount: vi.fn(),
    updateConnection: vi.fn(),
    createConnection: vi.fn(),
  },
}));

const connection: Connection = {
  id: "c1",
  name: "openai-main",
  provider_type: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  api_key: "sk-test",
  custom_headers: "{}",
  enabled: 1,
  connect_timeout_ms: null,
  idle_timeout_ms: null,
  pricing_model: null,
  cache_retention: "none",
  auth_style: "api_key",
  web_fetch_mode: "local",
  web_fetch_path: null,
  search_path: null,
  search_tier: null,
  provider_id: "openai",
  account_count: 0,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
};

describe("ConnectionFormDialog", () => {
  it("renders the extra-keys section and closes cleanly", async () => {
    const open = ref(true);
    const app = createApp({
      setup() {
        return () =>
          h(ConnectionFormDialog, {
            open: open.value,
            connection,
            "onUpdate:open": (value: boolean) => {
              open.value = value;
            },
          });
      },
    });

    const container = document.createElement("div");
    document.body.appendChild(container);
    app.mount(container);
    await nextTick();

    expect(document.body.textContent).toContain("Extra API keys");

    open.value = false;
    await nextTick();
    await nextTick();

    app.unmount();
    container.remove();
  });

  it("shows the sign-in flow for a new OAuth (Codex) connection", async () => {
    const open = ref(true);
    const app = createApp({
      setup() {
        return () =>
          h(ConnectionFormDialog, {
            open: open.value,
            connection: null,
            oauth: true,
            preset: {
              id: "openai-codex",
              label: "ChatGPT Plus/Pro (Codex)",
              provider_type: "openai-codex",
              base_url: "https://chatgpt.com/backend-api/codex",
              category: "subscription",
              auth: "oauth",
              default_headers: {},
              api_key_url: null,
              docs_url: null,
              note: null,
              risk_notice: null,
              oauth_preset: "openai-codex",
              configured: 0,
            },
            "onUpdate:open": (value: boolean) => {
              open.value = value;
            },
          });
      },
    });

    const container = document.createElement("div");
    document.body.appendChild(container);
    app.mount(container);
    await nextTick();

    expect(document.body.textContent).toContain("Connect in browser");
    expect(document.body.textContent).not.toContain("Create connection");
    expect(document.querySelector("#connection-api-key")).toBeNull();

    app.unmount();
    container.remove();
  });

  it("locks an existing OAuth connection to sign-in only", async () => {
    const open = ref(true);
    const oauthConnection: Connection = {
      ...connection,
      id: "c2",
      provider_type: "openai-codex",
      provider_id: "openai-codex",
      api_key: null,
    };
    const app = createApp({
      setup() {
        return () =>
          h(ConnectionFormDialog, {
            open: open.value,
            connection: oauthConnection,
            oauth: true,
            "onUpdate:open": (value: boolean) => {
              open.value = value;
            },
          });
      },
    });

    const container = document.createElement("div");
    document.body.appendChild(container);
    app.mount(container);
    await nextTick();

    // Only the name stays editable; the credential fields are gone.
    expect(document.querySelector("#connection-name")).not.toBeNull();
    expect(document.querySelector("#connection-api-key")).toBeNull();
    expect(document.querySelector("#connection-base-url")).toBeNull();
    expect(document.body.textContent).toContain("Sign in again");
    expect(document.body.textContent).toContain("Save name");

    app.unmount();
    container.remove();
  });

  it("parses numeric timeout inputs bound to number fields", async () => {    vi.mocked(api.updateConnection).mockResolvedValue(undefined as never);

    // Start closed so the open-watcher seeds name/base_url, as it does in use.
    const open = ref(false);
    const app = createApp({
      setup() {
        return () =>
          h(ConnectionFormDialog, {
            open: open.value,
            connection,
            "onUpdate:open": (value: boolean) => {
              open.value = value;
            },
          });
      },
    });

    const container = document.createElement("div");
    document.body.appendChild(container);
    app.mount(container);
    await nextTick();
    open.value = true;
    await nextTick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    await nextTick();

    const connect = document.querySelector<HTMLInputElement>(
      "#connection-connect-timeout",
    )!;
    connect.value = "5000";
    connect.dispatchEvent(new Event("input", { bubbles: true }));
    const idle = document.querySelector<HTMLInputElement>(
      "#connection-idle-timeout",
    )!;
    idle.value = "30000";
    idle.dispatchEvent(new Event("input", { bubbles: true }));
    await nextTick();

    const save = [...document.querySelectorAll("button")].find((candidate) =>
      candidate.textContent?.includes("Save changes"),
    );
    save!.click();
    await nextTick();
    await new Promise((resolve) => setTimeout(resolve, 0));

    // The number field stays a `number`; parsing must not assume a string.
    expect(api.updateConnection).toHaveBeenCalledWith(
      "c1",
      expect.objectContaining({
        connect_timeout_ms: 5000,
        idle_timeout_ms: 30000,
      }),
    );

    app.unmount();
    container.remove();
  });
});
