import { describe, expect, it } from "vitest";

import {
  ALL_SETTINGS_SECTIONS,
  SETTINGS_SECTIONS,
  TOKEN_SAVER_SECTION,
  changedKeys,
  sectionIsCustomized,
  sectionIsDirty,
  type SettingsSection,
  type SettingsSectionId,
} from "./sections";
import type { SettingsPatch } from "@/types/api";

const section = (id: SettingsSectionId): SettingsSection => {
  const found = SETTINGS_SECTIONS.find((entry) => entry.id === id);
  if (!found) throw new Error(`missing section ${id}`);
  return found;
};

describe("SETTINGS_SECTIONS", () => {
  it("covers every patch key exactly once", () => {
    const keys = ALL_SETTINGS_SECTIONS.flatMap((entry) => entry.keys);
    expect(new Set(keys).size).toBe(keys.length);
    expect(keys).toContain("require_api_key");
    expect(keys).toContain("ponytail_level");
    expect(keys).toContain("pricing_source_url");
  });

  it("gives every section a label and description", () => {
    for (const entry of ALL_SETTINGS_SECTIONS) {
      expect(entry.label.length).toBeGreaterThan(0);
      expect(entry.description.length).toBeGreaterThan(0);
    }
  });

  it("gives every patch key to exactly one section", () => {
    // The token-saving keys moved to the Token Saving page, so they must not
    // also be claimed by a Settings tab.
    const claimed = new Set(SETTINGS_SECTIONS.flatMap((entry) => entry.keys));
    for (const key of TOKEN_SAVER_SECTION.keys) {
      expect(claimed.has(key)).toBe(false);
    }
  });

  it("keeps token saving off the settings tabs", () => {
    expect(SETTINGS_SECTIONS.map((entry) => entry.id)).not.toContain(
      "token-saving",
    );
  });
});

describe("sectionIsDirty", () => {
  it("ignores edits belonging to another section", () => {
    const patch = { max_attempts: 9 };
    expect(sectionIsDirty(patch, section("routing"))).toBe(true);
    expect(sectionIsDirty(patch, section("security"))).toBe(false);
  });

  it("treats a cleared admin token as a security edit", () => {
    expect(sectionIsDirty({ admin_token: null }, section("security"))).toBe(
      true,
    );
  });

  it("never flags the data section, which has no settings", () => {
    expect(sectionIsDirty({ max_attempts: 9 }, section("data"))).toBe(false);
    expect(section("data").keys).toEqual([]);
  });

  it("counts the token saver keys as one section", () => {
    const patch: SettingsPatch = {
      slimmer_level: "aggressive",
      ponytail_level: "ultra",
    };
    expect(changedKeys(patch, TOKEN_SAVER_SECTION.keys)).toEqual([
      "slimmer_level",
      "ponytail_level",
    ]);
  });

  it("does not flag the token saver section for another tab's edit", () => {
    expect(sectionIsDirty({ max_attempts: 9 }, TOKEN_SAVER_SECTION)).toBe(
      false,
    );
  });
});

describe("sectionIsCustomized", () => {
  it("matches stored override keys by prefix", () => {
    const overrides = ["server.require_api_key", "token_saver.slimmer_level"];
    expect(sectionIsCustomized(overrides, section("security"))).toBe(true);
    expect(sectionIsCustomized(overrides, TOKEN_SAVER_SECTION)).toBe(true);
    expect(sectionIsCustomized(overrides, section("routing"))).toBe(false);
  });

  it("folds both limit prefixes into the one limits tab", () => {
    expect(
      sectionIsCustomized(["limits.max_concurrent"], section("limits")),
    ).toBe(true);
    expect(sectionIsCustomized(["rate_limit.burst"], section("limits"))).toBe(
      true,
    );
  });

  it("is false when nothing is overridden", () => {
    for (const entry of ALL_SETTINGS_SECTIONS) {
      expect(sectionIsCustomized([], entry)).toBe(false);
    }
  });
});
