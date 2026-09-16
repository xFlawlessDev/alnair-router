import { createApp, h, nextTick } from "vue";
import { describe, expect, it } from "vitest";

import ProviderIcon from "./ProviderIcon.vue";

interface Props {
  id?: string | null;
  type?: "openai-compatible" | "anthropic-native" | "command-code" | null;
  label: string;
  title?: string;
}

const mount = (props: Props) => {
  const app = createApp({ setup: () => () => h(ProviderIcon, props) });
  const container = document.createElement("div");
  document.body.appendChild(container);
  app.mount(container);

  return {
    container,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

describe("ProviderIcon", () => {
  it("inlines the bundled glyph for a known provider", async () => {
    const { container, unmount } = mount({ id: "openai", label: "OpenAI" });
    await nextTick();

    const svg = container.querySelector("svg");
    expect(svg, "glyph should render as inline SVG").toBeTruthy();
    expect(svg!.innerHTML.length).toBeGreaterThan(0);
    expect(
      container.querySelector(".bg-muted"),
      "no monogram fallback",
    ).toBeNull();

    unmount();
  });

  it("falls back to a monogram tile for providers without a glyph", async () => {
    const { container, unmount } = mount({
      id: "commandcode",
      label: "Command Code",
    });
    await nextTick();

    expect(container.querySelector("svg")).toBeNull();
    expect(container.textContent?.trim()).toBe("CC");

    unmount();
  });

  it("uses the wire family when a connection has no preset", async () => {
    const openai = mount({
      id: null,
      type: "openai-compatible",
      label: "openai-compatible",
    });
    await nextTick();
    expect(
      openai.container.querySelector("svg"),
      "openai-compatible",
    ).toBeTruthy();
    openai.unmount();

    const anthropic = mount({
      id: null,
      type: "anthropic-native",
      label: "anthropic-native",
    });
    await nextTick();
    expect(
      anthropic.container.querySelector("svg"),
      "anthropic-native",
    ).toBeTruthy();
    anthropic.unmount();

    // Command Code has no glyph yet, so it falls through to the monogram.
    const commandCode = mount({
      id: null,
      type: "command-code",
      label: "Command Code",
    });
    await nextTick();
    expect(commandCode.container.querySelector("svg")).toBeNull();
    expect(commandCode.container.textContent?.trim()).toBe("CC");
    commandCode.unmount();
  });

  it("is decorative unless a title is given", async () => {
    const plain = mount({ id: "openai", label: "OpenAI" });
    await nextTick();
    expect(plain.container.querySelector('[aria-hidden="true"]')).toBeTruthy();
    plain.unmount();

    const named = mount({ id: "openai", label: "OpenAI", title: "OpenAI" });
    await nextTick();
    expect(named.container.querySelector('[aria-hidden="true"]')).toBeNull();
    expect(named.container.querySelector('[aria-label="OpenAI"]')).toBeTruthy();
    named.unmount();
  });
});
