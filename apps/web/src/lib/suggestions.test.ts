import { describe, expect, it } from "vitest";

import { buildSuggestions } from "./suggestions";
import type { Alias, ComboWithEntries } from "@/types/api";

function alias(
  id: string,
  prefix: string,
  model: string | null = null,
  enabled = 1,
): Alias {
  return {
    id,
    prefix,
    connection_id: "conn-1",
    model_override: model,
    enabled,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

function combo(
  id: string,
  name: string,
  tiers: number,
  enabled = 1,
): ComboWithEntries {
  return {
    combo: {
      id,
      name,
      description: null,
      enabled,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    entries: Array.from({ length: tiers }, (_, index) => ({
      id: `${id}-${index}`,
      combo_id: id,
      model_ref: `tier-${index}`,
      position: index,
      enabled: 1,
    })),
  };
}

describe("buildSuggestions", () => {
  it("keeps enabled aliases and combos, sorted and annotated", () => {
    const suggestions = buildSuggestions({
      aliases: [
        alias("a2", "oa", "gpt-4o-mini"),
        alias("a1", "kr"),
        alias("a3", "off", null, 0),
      ],
      combos: [
        combo("c2", "smart", 2),
        combo("c1", "free", 0),
        combo("c3", "off", 1, 0),
      ],
    });

    expect(suggestions.aliases.map((item) => item.pattern)).toEqual([
      "kr",
      "oa",
    ]);
    expect(suggestions.aliases[1]).toMatchObject({
      id: "alias:a2",
      detail: "gpt-4o-mini",
    });
    expect(suggestions.combos.map((item) => item.pattern)).toEqual([
      "free",
      "smart",
    ]);
    expect(suggestions.combos[0]?.detail).toBe("");
    expect(suggestions.combos[1]?.detail).toBe("2 tiers");
  });

  it("hides taken patterns and the combo being edited", () => {
    const suggestions = buildSuggestions({
      aliases: [alias("a1", "oa"), alias("a2", "kr")],
      combos: [combo("c1", "self", 1), combo("c2", "other", 1)],
      taken: ["kr"],
      excludeComboId: "c1",
    });

    expect(suggestions.aliases.map((item) => item.pattern)).toEqual(["oa"]);
    expect(suggestions.combos.map((item) => item.pattern)).toEqual(["other"]);
  });

  it("drops blank identifiers", () => {
    const suggestions = buildSuggestions({
      aliases: [alias("a1", "")],
      combos: [combo("c1", "", 0)],
    });

    expect(suggestions.aliases).toHaveLength(0);
    expect(suggestions.combos).toHaveLength(0);
  });
});
