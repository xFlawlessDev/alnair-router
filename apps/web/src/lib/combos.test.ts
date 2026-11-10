import { describe, expect, it } from "vitest";

import { normalizeTierRef, normalizeTierRefs } from "./combos";
import type { Alias } from "@/types/api";

function alias(
  prefix: string,
  model: string | null = null,
  enabled = 1,
): Alias {
  return {
    id: `alias-${prefix}`,
    prefix,
    connection_id: "conn-1",
    model_override: model,
    pricing_model: null,
    enabled,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

const aliases = [
  alias("claude-haiku-5-5", "claude-haiku-5-5"),
  alias("kr", "claude-4.5-sonnet"),
  alias("oa"),
  alias("glm", "glm-4.6"),
  alias("off", "off", 0),
];

describe("normalizeTierRef", () => {
  it("collapses a model segment that repeats the prefix", () => {
    expect(
      normalizeTierRef("claude-haiku-5-5/claude-haiku-5-5", aliases),
    ).toBe("claude-haiku-5-5");
  });

  it("collapses a segment that repeats the pinned override", () => {
    expect(normalizeTierRef("kr/claude-4.5-sonnet", aliases)).toBe("kr");
  });

  it("keeps a segment that pins a different model", () => {
    expect(normalizeTierRef("glm/glm-5.1", aliases)).toBe("glm/glm-5.1");
  });

  it("keeps references for aliases that pin nothing", () => {
    expect(normalizeTierRef("oa/gpt-4o-mini", aliases)).toBe("oa/gpt-4o-mini");
  });

  it("keeps references that name no alias", () => {
    expect(normalizeTierRef("ghost/ghost", aliases)).toBe("ghost/ghost");
  });

  it("keeps references to a disabled alias", () => {
    expect(normalizeTierRef("off/off", aliases)).toBe("off/off");
  });

  it("leaves bare names, combos, and blank input alone", () => {
    expect(normalizeTierRef("free-forever", aliases)).toBe("free-forever");
    expect(normalizeTierRef("  kr  ", aliases)).toBe("kr");
    expect(normalizeTierRef("kr/", aliases)).toBe("kr/");
    expect(normalizeTierRef("/claude-4.5", aliases)).toBe("/claude-4.5");
    expect(normalizeTierRef("   ", aliases)).toBe("");
  });

  it("trims the segments it keeps", () => {
    expect(normalizeTierRef(" glm / glm-5.1 ", aliases)).toBe("glm/glm-5.1");
  });
});

describe("normalizeTierRefs", () => {
  it("normalizes, drops blanks, and removes repeats", () => {
    expect(
      normalizeTierRefs(
        ["claude-haiku-5-5/claude-haiku-5-5", "", "  ", "Claude-Haiku-5-5"],
        aliases,
      ),
    ).toEqual(["claude-haiku-5-5"]);
  });

  it("keeps the fallback order of distinct tiers", () => {
    expect(
      normalizeTierRefs(["glm/glm-5.1", "kr/claude-4.5-sonnet", "oa/gpt-4o"], aliases),
    ).toEqual(["glm/glm-5.1", "kr", "oa/gpt-4o"]);
  });

  it("dropping a tier can leave an empty chain, and that is reported as empty", () => {
    expect(normalizeTierRefs(["", "   "], aliases)).toEqual([]);
  });
});
