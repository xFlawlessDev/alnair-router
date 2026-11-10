/** Tier-reference helpers for combo fallback chains. */

import type { Alias } from "@/types/api";

function aliasFor(prefix: string, aliases: Alias[]): Alias | undefined {
  const needle = prefix.toLowerCase();
  return aliases.find(
    (alias) => alias.enabled !== 0 && alias.prefix.toLowerCase() === needle,
  );
}

/**
 * Collapses a tier reference whose model segment restates what the alias
 * already pins: `claude-haiku-5-5/claude-haiku-5-5` becomes the bare prefix.
 *
 * Bare prefixes only resolve when the alias pins a model, and a pinned alias
 * routes to that model whatever follows the slash — so the segment is dead
 * weight either way. References the router resolves differently (an alias that
 * keeps the caller's model, or no alias at all) are left untouched.
 */
export function normalizeTierRef(ref: string, aliases: Alias[]): string {
  const trimmed = ref.trim();
  const slash = trimmed.indexOf("/");
  if (slash <= 0 || slash === trimmed.length - 1) return trimmed;

  const prefix = trimmed.slice(0, slash).trim();
  const model = trimmed.slice(slash + 1).trim();
  if (!prefix || !model) return trimmed;

  const pinned = aliasFor(prefix, aliases)?.model_override?.trim();
  if (!pinned) return trimmed;

  const segment = model.toLowerCase();
  const redundant = segment === prefix.toLowerCase() || segment === pinned.toLowerCase();
  // Rebuilt from trimmed segments: the resolver trims both halves anyway, so
  // this only strips padding around the slash.
  return redundant ? prefix : `${prefix}/${model}`;
}

/**
 * Normalizes every tier and drops repeats, keeping the first occurrence. A
 * fallback chain that tries the same target twice is never what the author
 * meant, and collapsing refs can create such a pair.
 */
export function normalizeTierRefs(refs: string[], aliases: Alias[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const ref of refs) {
    const normalized = normalizeTierRef(ref, aliases);
    if (!normalized) continue;
    const key = normalized.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(normalized);
  }
  return out;
}
