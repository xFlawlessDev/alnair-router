/**
 * Brand glyphs for the provider presets.
 *
 * The SVGs are inlined as text rather than served as files: the glyphs fill with
 * `currentColor`, and a `currentColor` inside an `<img>` resolves against the
 * image document (always black), which would make them invisible in dark mode.
 *
 * Files land here as `<preset-id>.svg` via `node scripts/sync-provider-icons.mjs`,
 * so no name translation happens at runtime. Providers without a file get a
 * monogram tile instead — see `ProviderIcon.vue`.
 */
import type { ProviderType } from "@/types/api";

const files = import.meta.glob("../assets/providers/*.svg", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Preset id → SVG source, keyed by file name. */
export const providerIcons: Record<string, string> = Object.fromEntries(
  Object.entries(files).map(([path, svg]) => [
    path.slice(path.lastIndexOf("/") + 1, -".svg".length),
    svg,
  ]),
);

/** The glyph for a preset, or `null` when the provider has none. */
export function providerIcon(id?: string | null): string | null {
  if (!id) return null;
  return providerIcons[id.trim().toLowerCase()] ?? null;
}

/**
 * Glyphs standing in for the wire families, so a connection added by hand (no
 * preset behind it) still shows a mark instead of an empty column.
 */
const PROVIDER_TYPE_ICONS: Record<ProviderType, string> = {
  "openai-compatible": "openai",
  "anthropic-native": "anthropic",
  "command-code": "commandcode",
};

/** The glyph for a wire family, or `null` when it has none (monogram). */
export function providerTypeIcon(type?: ProviderType | null): string | null {
  if (!type) return null;
  return providerIcon(PROVIDER_TYPE_ICONS[type]);
}

/** One or two letters standing in for a provider without a glyph. */
export function providerInitials(label: string): string {
  // Hyphens separate words too, so a wire family like `command-code` reads as CC.
  const words = label
    .trim()
    .split(/[\s()-]+/)
    .filter(Boolean);
  if (!words.length) return "?";
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return (words[0][0] + words[1][0]).toUpperCase();
}
