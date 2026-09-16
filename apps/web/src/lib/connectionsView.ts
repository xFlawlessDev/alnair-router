/**
 * Connections list shaping: filtering, grouping and the view preferences the
 * page remembers. Kept free of Vue so the behaviour is testable on its own.
 */
import { isEnabled } from '@/lib/format';
import type { Connection, ProviderType } from '@/types/api';

export type ConnectionsView = 'list' | 'grid';
export type ConnectionsGroup = 'none' | 'provider' | 'type';
export type ConnectionStatus = 'all' | 'enabled' | 'disabled';

export interface ConnectionFilters {
  search: string;
  type: ProviderType | 'all';
  status: ConnectionStatus;
}

export const DEFAULT_FILTERS: ConnectionFilters = { search: '', type: 'all', status: 'all' };

/** Resolves a connection's preset label; the page passes `providerLabel`. */
export type LabelResolver = (connection: Connection) => string;

export interface ConnectionGroupView {
  key: string;
  /** Empty for the flat view, which renders no heading. */
  label: string;
  items: Connection[];
}

/** Connections added by hand have no preset; they group under this. */
const CUSTOM_GROUP_KEY = '__custom__';
const CUSTOM_GROUP_LABEL = 'Custom endpoints';

const VIEWS: ConnectionsView[] = ['list', 'grid'];
const GROUPS: ConnectionsGroup[] = ['none', 'provider', 'type'];
const VIEW_KEY = 'connections.view';
const GROUP_KEY = 'connections.group';

export function hasActiveFilters(filters: ConnectionFilters): boolean {
  return filters.search.trim() !== '' || filters.type !== 'all' || filters.status !== 'all';
}

export function filterConnections(
  connections: Connection[],
  filters: ConnectionFilters,
  labelOf: LabelResolver,
): Connection[] {
  const term = filters.search.trim().toLowerCase();

  return connections.filter((connection) => {
    if (filters.type !== 'all' && connection.provider_type !== filters.type) return false;
    if (filters.status !== 'all' && isEnabled(connection.enabled) !== (filters.status === 'enabled')) {
      return false;
    }
    if (!term) return true;

    return [connection.name, connection.base_url, connection.provider_type, labelOf(connection)].some(
      (value) => value.toLowerCase().includes(term),
    );
  });
}

export function groupConnections(
  connections: Connection[],
  group: ConnectionsGroup,
  labelOf: LabelResolver,
): ConnectionGroupView[] {
  if (group === 'none') return [{ key: 'all', label: '', items: connections }];

  const buckets = new Map<string, ConnectionGroupView>();

  for (const connection of connections) {
    const key =
      group === 'provider' ? (connection.provider_id ?? CUSTOM_GROUP_KEY) : connection.provider_type;
    const label =
      group === 'provider'
        ? connection.provider_id
          ? labelOf(connection) || connection.provider_id
          : CUSTOM_GROUP_LABEL
        : connection.provider_type;

    const bucket = buckets.get(key) ?? { key, label, items: [] };
    bucket.items.push(connection);
    buckets.set(key, bucket);
  }

  return [...buckets.values()].sort((left, right) => {
    if (left.key === CUSTOM_GROUP_KEY) return 1;
    if (right.key === CUSTOM_GROUP_KEY) return -1;
    return left.label.localeCompare(right.label);
  });
}

export function loadViewPreferences(): { view: ConnectionsView; group: ConnectionsGroup } {
  const fallback: { view: ConnectionsView; group: ConnectionsGroup } = {
    view: 'list',
    group: 'none',
  };

  try {
    const view = window.localStorage.getItem(VIEW_KEY);
    const group = window.localStorage.getItem(GROUP_KEY);

    return {
      view: VIEWS.includes(view as ConnectionsView) ? (view as ConnectionsView) : fallback.view,
      group: GROUPS.includes(group as ConnectionsGroup) ? (group as ConnectionsGroup) : fallback.group,
    };
  } catch {
    return fallback;
  }
}

export function saveViewPreferences(preferences: {
  view: ConnectionsView;
  group: ConnectionsGroup;
}): void {
  try {
    window.localStorage.setItem(VIEW_KEY, preferences.view);
    window.localStorage.setItem(GROUP_KEY, preferences.group);
  } catch {
    // Storage can be unavailable (private mode); the page works without it.
  }
}
