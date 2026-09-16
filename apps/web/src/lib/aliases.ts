/** Prefix-generation helpers for importing upstream models as aliases. */

/** Slugifies a model id into a valid alias prefix. */
export function modelPrefixSlug(model: string): string {
  return model
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/-{2,}/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** Prefixes must not be empty; fall back to a generic name. */
export function fallbackPrefix(slug: string, index: number): string {
  return slug || `model-${index + 1}`;
}

/** Appends `-2`, `-3`, ... until the prefix is unused, and reserves it. */
export function uniquePrefix(base: string, taken: Set<string>): string {
  if (!taken.has(base)) {
    taken.add(base);
    return base;
  }
  let suffix = 2;
  while (taken.has(`${base}-${suffix}`)) suffix += 1;
  const unique = `${base}-${suffix}`;
  taken.add(unique);
  return unique;
}
