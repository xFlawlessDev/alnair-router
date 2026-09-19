import { beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_FILTERS,
  filterConnections,
  groupConnections,
  hasActiveFilters,
  loadViewPreferences,
  saveViewPreferences,
  type ConnectionFilters,
} from "./connectionsView";
import type { Connection } from "@/types/api";

const connection = (overrides: Partial<Connection>): Connection => ({
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
  provider_id: "openai",
  account_count: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...overrides,
});

const LABELS: Record<string, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  groq: "Groq",
};

/** Mirrors the page's `providerLabel` for preset-backed connections. */
const labelOf = (row: Connection): string =>
  row.provider_id ? (LABELS[row.provider_id] ?? row.provider_id) : "";

const openai = connection({
  id: "a",
  name: "openai-main",
  provider_id: "openai",
});
const anthropic = connection({
  id: "b",
  name: "claude-main",
  provider_id: "anthropic",
  provider_type: "anthropic-native",
  base_url: "https://api.anthropic.com/v1",
});
const custom = connection({
  id: "c",
  name: "local-gateway",
  provider_id: null,
  provider_type: "command-code",
  base_url: "https://api.commandcode.ai",
  enabled: 0,
});

const filters = (
  overrides: Partial<ConnectionFilters> = {},
): ConnectionFilters => ({
  ...DEFAULT_FILTERS,
  ...overrides,
});

describe("connection filters", () => {
  const all = [openai, anthropic, custom];

  it("matches the search term against name, url, type and provider label", () => {
    expect(
      filterConnections(all, filters({ search: "claude" }), labelOf),
    ).toEqual([anthropic]);
    expect(
      filterConnections(all, filters({ search: "anthropic.com" }), labelOf),
    ).toEqual([anthropic]);
    expect(
      filterConnections(all, filters({ search: "command-code" }), labelOf),
    ).toEqual([custom]);
    expect(filterConnections(all, filters({ search: "gr" }), labelOf)).toEqual(
      [],
    );
  });

  it("is case-insensitive and ignores surrounding whitespace", () => {
    expect(
      filterConnections(all, filters({ search: "  OPENAI " }), labelOf),
    ).toEqual([openai]);
  });

  it("filters by provider type and status", () => {
    expect(
      filterConnections(all, filters({ type: "anthropic-native" }), labelOf),
    ).toEqual([anthropic]);
    expect(
      filterConnections(all, filters({ type: "command-code" }), labelOf),
    ).toEqual([custom]);
    expect(
      filterConnections(all, filters({ status: "enabled" }), labelOf),
    ).toEqual([openai, anthropic]);
    expect(
      filterConnections(all, filters({ status: "disabled" }), labelOf),
    ).toEqual([custom]);
  });

  it("reports whether any filter is active", () => {
    expect(hasActiveFilters(DEFAULT_FILTERS)).toBe(false);
    expect(hasActiveFilters(filters({ search: "   " }))).toBe(false);
    expect(hasActiveFilters(filters({ search: "openai" }))).toBe(true);
    expect(hasActiveFilters(filters({ type: "openai-compatible" }))).toBe(true);
    expect(hasActiveFilters(filters({ status: "disabled" }))).toBe(true);
  });
});

describe("connection grouping", () => {
  const all = [openai, anthropic, custom];

  it("keeps one unlabelled group when flat", () => {
    expect(groupConnections(all, "none", labelOf)).toEqual([
      { key: "all", label: "", items: all },
    ]);
  });

  it("groups by provider and sorts custom endpoints last", () => {
    const groups = groupConnections(all, "provider", labelOf);

    expect(groups.map((group) => group.label)).toEqual([
      "Anthropic",
      "OpenAI",
      "Custom endpoints",
    ]);
    expect(groups[0].items).toEqual([anthropic]);
    expect(groups[2].items).toEqual([custom]);
  });

  it("groups by wire family", () => {
    const groups = groupConnections(all, "type", labelOf);

    expect(groups.map((group) => group.label)).toEqual([
      "anthropic-native",
      "command-code",
      "openai-compatible",
    ]);
  });
});

describe("view preferences", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("defaults to the list view, flat", () => {
    expect(loadViewPreferences()).toEqual({ view: "list", group: "none" });
  });

  it("round-trips a saved preference", () => {
    saveViewPreferences({ view: "grid", group: "provider" });

    expect(loadViewPreferences()).toEqual({ view: "grid", group: "provider" });
  });

  it("falls back when the stored values are unknown", () => {
    window.localStorage.setItem("connections.view", "carousel");
    window.localStorage.setItem("connections.group", "phase-of-moon");

    expect(loadViewPreferences()).toEqual({ view: "list", group: "none" });
  });
});
