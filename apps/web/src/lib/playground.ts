import type { Suggestion } from "@/lib/suggestions";
import type { ModelCatalogEntry, TokenSaverSettings } from "@/types/api";

/** One per-run saver switch, as the playground renders it. */
export interface SaverToggle {
  /** Settings key the switch writes, e.g. `caveman_enabled`. */
  key: string;
  label: string;
  on: boolean;
}

/** Display order for the switches, matching the pipeline's own order. */
const SAVER_SWITCHES: { key: string; label: string }[] = [
  { key: "slimmer_enabled", label: "RTK / Slimmer" },
  { key: "headroom_enabled", label: "Headroom" },
  { key: "terse_enabled", label: "Terse" },
  { key: "caveman_enabled", label: "Caveman" },
  { key: "ponytail_enabled", label: "Ponytail" },
];

/**
 * Builds the switch row from the effective configuration.
 *
 * Returns nothing when neither the saved settings nor an override is known, so
 * the playground shows no switches rather than a row of misleading "off"s
 * before the settings load.
 */
export function buildToggles(
  config: Partial<TokenSaverSettings> | null,
): SaverToggle[] {
  if (!config) return [];
  return SAVER_SWITCHES.map(({ key, label }) => ({
    key,
    label,
    on: Boolean((config as Record<string, unknown>)[key]),
  }));
}

/**
 * Flips one saver in the per-run overrides.
 *
 * Terse and caveman both write a system directive, so they are mutually
 * exclusive: enabling one clears the other rather than letting the server reject
 * the run. The rest of the switches are independent.
 */
export function toggleSaver(
  overrides: Partial<TokenSaverSettings>,
  effective: Partial<TokenSaverSettings> | null,
  key: string,
): Partial<TokenSaverSettings> {
  if (!effective) return overrides;

  const next = { ...overrides } as Record<string, unknown>;
  next[key] = !(effective as Record<string, unknown>)[key];

  if (key === "terse_enabled" && next.terse_enabled)
    next.caveman_enabled = false;
  if (key === "caveman_enabled" && next.caveman_enabled)
    next.terse_enabled = false;

  return next as Partial<TokenSaverSettings>;
}

/**
 * Groups the model catalog into the pickable references.
 *
 * A combo is expanded to one row per tier, so its entries are collapsed back
 * into a single reference and described by its tier count. Aliases are listed
 * as themselves, with the upstream model they pin as the detail.
 */
export function buildModelSuggestions(entries: ModelCatalogEntry[]): {
  aliases: Suggestion[];
  combos: Suggestion[];
} {
  const aliases: Suggestion[] = [];
  const combos = new Map<string, number>();

  for (const entry of entries) {
    if (entry.kind === "combo") {
      combos.set(entry.id, (combos.get(entry.id) ?? 0) + 1);
      continue;
    }
    aliases.push({
      id: `alias:${entry.id}`,
      pattern: entry.id,
      detail: entry.upstream_model ?? entry.provider,
    });
  }

  const comboRows: Suggestion[] = [...combos].map(([id, tiers]) => ({
    id: `combo:${id}`,
    pattern: id,
    detail: `${tiers} tier${tiers === 1 ? "" : "s"}`,
  }));

  const byPattern = (left: Suggestion, right: Suggestion) =>
    left.pattern.localeCompare(right.pattern);

  return {
    aliases: aliases.sort(byPattern),
    combos: comboRows.sort(byPattern),
  };
}
