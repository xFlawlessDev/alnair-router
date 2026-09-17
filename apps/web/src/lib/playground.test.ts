import { describe, expect, it } from "vitest";

import { buildModelSuggestions, buildToggles, toggleSaver } from "./playground";
import type { ModelCatalogEntry } from "@/types/api";

function entry(
  id: string,
  kind: ModelCatalogEntry["kind"],
  overrides: Partial<ModelCatalogEntry> = {},
): ModelCatalogEntry {
  return {
    id,
    kind,
    provider: "acme",
    provider_type: "openai-compatible",
    provider_id: null,
    upstream_model: null,
    tier: null,
    price: null,
    price_matched: null,
    price_source: null,
    ...overrides,
  };
}

describe("buildToggles", () => {
  it("reports nothing before the settings load", () => {
    expect(buildToggles(null)).toEqual([]);
  });

  it("reads each switch off the effective configuration", () => {
    const toggles = buildToggles({
      slimmer_enabled: true,
      headroom_enabled: false,
      caveman_enabled: true,
    });

    expect(toggles.map((toggle) => [toggle.key, toggle.on])).toEqual([
      ["slimmer_enabled", true],
      ["headroom_enabled", false],
      ["terse_enabled", false],
      ["caveman_enabled", true],
      ["ponytail_enabled", false],
    ]);
  });
});

describe("toggleSaver", () => {
  it("flips one switch into the overrides without touching the rest", () => {
    expect(
      toggleSaver({}, { slimmer_enabled: true }, "slimmer_enabled"),
    ).toEqual({ slimmer_enabled: false });
  });

  it("clears caveman when terse is switched on", () => {
    expect(
      toggleSaver(
        { caveman_enabled: true },
        { terse_enabled: false },
        "terse_enabled",
      ),
    ).toEqual({ terse_enabled: true, caveman_enabled: false });
  });

  it("clears terse when caveman is switched on", () => {
    expect(
      toggleSaver(
        { terse_enabled: true },
        { caveman_enabled: false },
        "caveman_enabled",
      ),
    ).toEqual({ caveman_enabled: true, terse_enabled: false });
  });

  it("leaves the overrides alone when nothing is loaded", () => {
    expect(toggleSaver({}, null, "slimmer_enabled")).toEqual({});
  });
});

describe("buildModelSuggestions", () => {
  it("collapses a combo's per-tier rows into one reference", () => {
    const suggestions = buildModelSuggestions([
      entry("smart", "combo", { tier: 1, upstream_model: "gpt-4o" }),
      entry("smart", "combo", { tier: 2, upstream_model: "gpt-4o-mini" }),
    ]);

    expect(suggestions.combos).toHaveLength(1);
    expect(suggestions.combos[0]).toMatchObject({
      id: "combo:smart",
      pattern: "smart",
      detail: "2 tiers",
    });
    expect(suggestions.aliases).toHaveLength(0);
  });

  it("describes an alias by the upstream model it pins", () => {
    const suggestions = buildModelSuggestions([
      entry("oa", "alias", { upstream_model: "gpt-4o-mini" }),
      entry("kr", "alias", { provider: "krouter", upstream_model: null }),
    ]);

    expect(suggestions.aliases.map((item) => item.pattern)).toEqual([
      "kr",
      "oa",
    ]);
    expect(suggestions.aliases[1]?.detail).toBe("gpt-4o-mini");
    // An alias that pins nothing falls back to the connection name.
    expect(suggestions.aliases[0]?.detail).toBe("krouter");
  });

  it("sorts both groups A to Z", () => {
    const suggestions = buildModelSuggestions([
      entry("zeta", "alias"),
      entry("alpha", "alias"),
      entry("yankee", "combo", { tier: 1 }),
      entry("bravo", "combo", { tier: 1 }),
    ]);

    expect(suggestions.aliases.map((item) => item.pattern)).toEqual([
      "alpha",
      "zeta",
    ]);
    expect(suggestions.combos.map((item) => item.pattern)).toEqual([
      "bravo",
      "yankee",
    ]);
  });

  it("reports a single-tier combo in the singular", () => {
    expect(
      buildModelSuggestions([entry("solo", "combo", { tier: 1 })]).combos[0]
        ?.detail,
    ).toBe("1 tier");
  });

  it("returns nothing for an empty catalog", () => {
    expect(buildModelSuggestions([])).toEqual({ aliases: [], combos: [] });
  });
});
