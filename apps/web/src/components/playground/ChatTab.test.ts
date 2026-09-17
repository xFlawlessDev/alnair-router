import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import ChatTab from "./ChatTab.vue";
import { ApiError, api, streamPlaygroundChat } from "@/lib/api";
import type { PlaygroundChatHandlers } from "@/lib/api";
import type { SaverToggle } from "@/lib/playground";

vi.mock("vue-sonner", () => ({ toast: { error: vi.fn() } }));

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {
    readonly status = 400;
    readonly type = "invalid_request_error";
  },
  api: {
    modelCatalog: vi.fn(),
  },
  streamPlaygroundChat: vi.fn(),
}));

const toggles: SaverToggle[] = [
  { key: "slimmer_enabled", label: "RTK / Slimmer", on: true },
  { key: "caveman_enabled", label: "Caveman", on: false },
];

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mount = async (overrides = {}) => {
  const app = createApp(ChatTab, { toggles, overrides });
  const container = document.createElement("div");
  document.body.appendChild(container);
  app.mount(container);
  await settle();

  return {
    container,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

const button = (container: HTMLElement, label: string) =>
  [...container.querySelectorAll("button")].find((candidate) =>
    candidate.textContent?.includes(label),
  );

/** Types into the message box and sends, as a user would. */
const send = async (container: HTMLElement, text: string) => {
  const input = container.querySelector<HTMLTextAreaElement>(
    "#playground-chat-input",
  );
  input!.value = text;
  input!.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();

  button(container, "Send")!.click();
  await settle();
};

describe("ChatTab", () => {
  beforeEach(() => {
    vi.mocked(api.modelCatalog).mockResolvedValue({
      object: "list",
      data: [
        { id: "chatty", kind: "alias", upstream_model: "deepseek-v4.1-flash" },
        { id: "combo-a", kind: "combo", tier: 1 },
      ],
    } as never);
    vi.mocked(streamPlaygroundChat).mockReset();
  });

  it("streams a reply and shows routing and usage", async () => {
    vi.mocked(streamPlaygroundChat).mockImplementation(async (_body, on) => {
      on.onRouter?.({
        model: "deepseek-v4.1-flash",
        source: "alias:chatty",
        provider_type: "openai-compatible",
        attempts: 1,
      });
      on.onDelta?.("pon");
      on.onDelta?.("g");
      on.onUsage?.({
        prompt_tokens: 12,
        completion_tokens: 3,
        cost_usd: 0.0001,
        savings: {
          saved_rtk_tokens: 0,
          saved_headroom_tokens: 0,
          saved_terse_tokens: 0,
          saved_caveman_tokens: 0,
          saved_ponytail_tokens: 0,
          saved_cost_usd: 0,
        },
      });
    });

    const { container, unmount } = await mount();
    await send(container, "hi");

    expect(container.textContent).toContain("hi");
    expect(container.textContent).toContain("pong");
    // The tier that answered, plus the token/cost totals.
    expect(container.textContent).toContain("deepseek-v4.1-flash");
    expect(container.textContent).toContain("alias:chatty");
    expect(container.textContent).toContain("12 in");
    expect(container.textContent).toContain("3 out");

    // The request carries the conversation and the shared overrides.
    const [body] = vi.mocked(streamPlaygroundChat).mock.calls[0]!;
    expect(body.model).toBe("chatty");
    expect(body.messages.at(-1)).toEqual({ role: "user", content: "hi" });
    // The empty assistant placeholder is never sent.
    expect(body.messages.some((message) => message.content === "")).toBe(false);

    unmount();
  });

  it("passes per-run overrides through to the endpoint", async () => {
    vi.mocked(streamPlaygroundChat).mockResolvedValue(undefined);

    const { container, unmount } = await mount({ caveman_enabled: true });
    await send(container, "hi");

    const [body] = vi.mocked(streamPlaygroundChat).mock.calls[0]!;
    expect(body.overrides).toMatchObject({ caveman_enabled: true });

    unmount();
  });

  it("surfaces a streamed error on the turn that failed", async () => {
    vi.mocked(streamPlaygroundChat).mockImplementation(
      async (_body: unknown, on: PlaygroundChatHandlers) => {
        on.onDelta?.("partial");
        on.onError?.("upstream refused the request");
      },
    );

    const { container, unmount } = await mount();
    await send(container, "hi");

    expect(container.textContent).toContain("upstream refused the request");
    // The partial answer is kept rather than discarded.
    expect(container.textContent).toContain("partial");

    unmount();
  });

  it("keeps the system prompt at the head of the request", async () => {
    vi.mocked(streamPlaygroundChat).mockResolvedValue(undefined);

    const { container, unmount } = await mount();
    const system = container.querySelector<HTMLTextAreaElement>(
      "#playground-chat-system",
    );
    system!.value = "Be terse.";
    system!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    await send(container, "hi");

    const [body] = vi.mocked(streamPlaygroundChat).mock.calls[0]!;
    expect(body.messages[0]).toEqual({ role: "system", content: "Be terse." });

    unmount();
  });

  it("shows reasoning apart from the answer and reports prompt savings", async () => {
    vi.mocked(streamPlaygroundChat).mockImplementation(async (_body, on) => {
      on.onThinking?.("let me think");
      on.onDelta?.("answer");
      on.onUsage?.({
        prompt_tokens: 100,
        completion_tokens: 5,
        cost_usd: 0.0002,
        savings: {
          saved_rtk_tokens: 40,
          saved_headroom_tokens: 10,
          saved_terse_tokens: 0,
          saved_caveman_tokens: 0,
          saved_ponytail_tokens: 0,
          saved_cost_usd: 0.0001,
        },
      });
    });

    const { container, unmount } = await mount();
    await send(container, "hi");

    // Reasoning is kept, just styled apart from the answer.
    expect(container.textContent).toContain("let me think");
    expect(container.textContent).toContain("answer");
    // 40 + 10 measured input tokens removed.
    expect(container.textContent).toContain("50");

    unmount();
  });

  it("reports a failed catalog load without blocking the chat", async () => {
    vi.mocked(api.modelCatalog).mockRejectedValue(
      new ApiError("cannot list models", 500),
    );

    const { container, unmount } = await mount();

    // The field is free text, so a failure here informs rather than disables.
    expect(container.textContent).toContain("cannot list models");
    expect(container.textContent).toContain("Send a message");

    unmount();
  });

  it("falls back to a generic message when the catalog fails uncontrollably", async () => {
    vi.mocked(api.modelCatalog).mockRejectedValue(new Error("boom"));

    const { container, unmount } = await mount();

    expect(container.textContent).toContain("Could not load the catalog");

    unmount();
  });

  it("offers the catalog in the model field", async () => {
    const { container, unmount } = await mount();

    // The first catalog entry seeds the field, so a send needs no typing.
    const field = container.querySelector<HTMLInputElement>(
      "#playground-chat-model",
    );
    expect(field?.value).toBe("chatty");
    // The field is a combobox, so the catalog is offered as a pick list. The
    // popover itself needs a real browser, so its contents are covered by the
    // buildModelSuggestions tests instead.
    expect(field?.getAttribute("role")).toBe("combobox");

    unmount();
  });

  it("drops the empty placeholder when a stream is stopped before any token", async () => {
    // Aborts surface as a DOMException, which must not be reported as failure.
    vi.mocked(streamPlaygroundChat).mockImplementation(
      async (_body, _on, signal) => {
        await new Promise((resolve, reject) => {
          signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
          setTimeout(resolve, 50);
        });
      },
    );

    const { container, unmount } = await mount();

    const input = container.querySelector<HTMLTextAreaElement>(
      "#playground-chat-input",
    );
    input!.value = "hi";
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    button(container, "Send")!.click();
    await settle();

    button(container, "Stop")!.click();
    await settle();

    // The user turn stays; the empty assistant bubble does not.
    expect(container.textContent).toContain("hi");
    expect(container.textContent).not.toContain("upstream");

    unmount();
  });
});
