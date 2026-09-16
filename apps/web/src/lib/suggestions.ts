import type { Alias, ComboWithEntries, ID } from "@/types/api";

/** One selectable pattern: an alias prefix or a combo name. */
export interface Suggestion {
  id: string;
  pattern: string;
  detail: string;
}

function byPattern(left: Suggestion, right: Suggestion): number {
  return left.pattern.localeCompare(right.pattern);
}

/**
 * Builds allowlist and tier suggestions from the enabled aliases and combos:
 * blanks and disabled entries are skipped, `taken` patterns are hidden, and
 * both groups are sorted A→Z. `excludeComboId` keeps a combo from listing
 * itself in its own fallback chain.
 */
export function buildSuggestions(input: {
  aliases: Alias[];
  combos: ComboWithEntries[];
  taken?: string[];
  excludeComboId?: ID | null;
}): { aliases: Suggestion[]; combos: Suggestion[] } {
  const taken = new Set(input.taken ?? []);

  const aliases = input.aliases
    .filter(
      (alias) =>
        alias.enabled !== 0 && alias.prefix && !taken.has(alias.prefix),
    )
    .map((alias) => ({
      id: `alias:${alias.id}`,
      pattern: alias.prefix,
      detail: alias.model_override ?? "",
    }))
    .sort(byPattern);

  const combos = input.combos
    .filter(
      ({ combo }) =>
        combo.enabled !== 0 &&
        combo.name &&
        combo.id !== input.excludeComboId &&
        !taken.has(combo.name),
    )
    .map(({ combo, entries }) => ({
      id: `combo:${combo.id}`,
      pattern: combo.name,
      detail: entries.length
        ? `${entries.length} tier${entries.length > 1 ? "s" : ""}`
        : "",
    }))
    .sort(byPattern);

  return { aliases, combos };
}
