import { describe, expect, it } from "vitest";
import { catalogMatch, combineCapabilities } from "./capabilities";
import type { CapabilityCatalog, ModelCapabilities } from "@/types/api";

const info = (context: number): ModelCapabilities => ({
  context,
  reasoning: null,
  tool_call: true,
  reasoning_levels: null,
  input_formats: ["text"],
  output_formats: null,
});
const catalog: CapabilityCatalog = {
  entries: [
    { provider: "a", model: "example", capabilities: info(100) },
    { provider: "b", model: "example", capabilities: info(200) },
  ],
  overrides: [],
  updated_at: null,
};

describe("model capabilities", () => {
  it("matches the provider and refuses ambiguous model names", () => {
    expect(
      catalogMatch(catalog, { label: "model", model: "example", provider: "b" })
        ?.context,
    ).toBe(200);
    expect(
      catalogMatch(catalog, {
        label: "model",
        model: "example",
        provider: null,
      }),
    ).toBeNull();
  });
  it("merges manual fields while keeping unknowns and explicit false", () => {
    const override = { ...info(300), tool_call: false, input_formats: null };
    expect(combineCapabilities(info(100), override)).toMatchObject({
      context: 300,
      tool_call: false,
      input_formats: ["text"],
    });
  });
});
