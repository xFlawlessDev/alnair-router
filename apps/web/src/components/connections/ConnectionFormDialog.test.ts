import { createApp, h, nextTick, ref } from "vue";
import { describe, expect, it, vi } from "vitest";

import ConnectionFormDialog from "./ConnectionFormDialog.vue";
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
});
