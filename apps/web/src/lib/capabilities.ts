import { ref } from "vue";
import { api } from "@/lib/api";
import type {
  Alias,
  CapabilityCatalog,
  ComboWithEntries,
  Connection,
  ModelCapabilities,
} from "@/types/api";

export type CapabilityTarget = {
  label: string;
  model: string | null;
  provider: string | null;
  aliasId?: string;
};

export function useCapabilities() {
  const catalog = ref<CapabilityCatalog>({
    entries: [],
    overrides: [],
    updated_at: null,
  });
  const error = ref<string | null>(null);
  const syncing = ref(false);
  async function load() {
    catalog.value = await api.listCapabilities();
  }
  async function sync() {
    syncing.value = true;
    error.value = null;
    try {
      await api.syncCapabilities();
      await load();
    } catch {
      error.value =
        "Could not refresh models.dev. Saved information is still available.";
    } finally {
      syncing.value = false;
    }
  }
  async function initialize() {
    try {
      await load();
      if (
        !catalog.value.updated_at ||
        Date.now() - Date.parse(catalog.value.updated_at) > 86400000
      )
        await sync();
    } catch {
      error.value = "Could not load model capabilities.";
    }
  }
  return { catalog, error, syncing, load, sync, initialize };
}

export function aliasTarget(
  alias: Alias,
  connections: Connection[],
  suppliedModel?: string,
): CapabilityTarget {
  const connection = connections.find(
    (item) => item.id === alias.connection_id,
  );
  return {
    label: alias.prefix,
    model: alias.model_override || suppliedModel || null,
    provider: connection?.provider_id ?? null,
    aliasId: alias.id,
  };
}

export function comboTargets(
  combo: ComboWithEntries,
  combos: ComboWithEntries[],
  aliases: Alias[],
  connections: Connection[],
  visited = new Set<string>(),
): CapabilityTarget[] {
  if (visited.has(combo.combo.id))
    return [{ label: combo.combo.name, model: null, provider: null }];
  const next = new Set(visited).add(combo.combo.id);
  return combo.entries
    .filter((entry) => entry.enabled)
    .flatMap((entry) => {
      const nested = combos.find(
        (item) =>
          item.combo.name.toLowerCase() === entry.model_ref.toLowerCase(),
      );
      if (nested)
        return comboTargets(nested, combos, aliases, connections, next);
      const [prefix, ...rest] = entry.model_ref.split("/");
      const alias = aliases.find(
        (item) => item.prefix.toLowerCase() === prefix?.toLowerCase(),
      );
      if (alias)
        return [
          {
            ...aliasTarget(alias, connections, rest.join("/") || undefined),
            label: entry.model_ref,
          },
        ];
      return [
        { label: entry.model_ref, model: entry.model_ref, provider: null },
      ];
    });
}

export function catalogMatch(
  catalog: CapabilityCatalog,
  target: CapabilityTarget,
): ModelCapabilities | null {
  if (!target.model) return null;
  const matches = catalog.entries.filter(
    (entry) => entry.model.toLowerCase() === target.model!.toLowerCase(),
  );
  const exact = matches.find(
    (entry) => entry.provider.toLowerCase() === target.provider?.toLowerCase(),
  );
  if (exact) return exact.capabilities;
  if (target.provider) return null;
  if (
    matches.length &&
    matches.every(
      (entry) =>
        JSON.stringify(entry.capabilities) ===
        JSON.stringify(matches[0]!.capabilities),
    )
  )
    return matches[0]!.capabilities;
  return null;
}

export function combineCapabilities(
  base: ModelCapabilities | null,
  override: ModelCapabilities | null,
): ModelCapabilities | null {
  if (!base && !override) return null;
  const keys: (keyof ModelCapabilities)[] = [
    "context",
    "reasoning",
    "tool_call",
    "reasoning_levels",
    "input_formats",
    "output_formats",
  ];
  return Object.fromEntries(
    keys.map((key) => [key, override?.[key] ?? base?.[key] ?? null]),
  ) as unknown as ModelCapabilities;
}
