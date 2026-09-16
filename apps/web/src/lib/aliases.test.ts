import { describe, expect, it } from "vitest";

import { fallbackPrefix, modelPrefixSlug, uniquePrefix } from "./aliases";

describe("modelPrefixSlug", () => {
  it("lowercases and replaces separators", () => {
    expect(modelPrefixSlug("GPT-4o")).toBe("gpt-4o");
    expect(modelPrefixSlug("claude-3.5-sonnet")).toBe("claude-3-5-sonnet");
    expect(modelPrefixSlug("Qwen/Qwen3-235B")).toBe("qwen-qwen3-235b");
    expect(modelPrefixSlug("  spaced  name  ")).toBe("spaced-name");
  });

  it("trims leading and trailing separators", () => {
    expect(modelPrefixSlug("__weird__")).toBe("weird");
    expect(modelPrefixSlug("a---b")).toBe("a-b");
  });
});

describe("fallbackPrefix", () => {
  it("falls back when the slug is empty", () => {
    expect(fallbackPrefix("", 0)).toBe("model-1");
    expect(fallbackPrefix("", 2)).toBe("model-3");
    expect(fallbackPrefix("ok", 0)).toBe("ok");
  });
});

describe("uniquePrefix", () => {
  it("returns the base when free and reserves it", () => {
    const taken = new Set(["other"]);
    expect(uniquePrefix("gpt-4o", taken)).toBe("gpt-4o");
    expect(taken.has("gpt-4o")).toBe(true);
  });

  it("appends numeric suffixes on conflict", () => {
    const taken = new Set(["gpt-4o", "gpt-4o-2"]);
    expect(uniquePrefix("gpt-4o", taken)).toBe("gpt-4o-3");
  });
});
